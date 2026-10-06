//! The harvester self-test's filesystem cases (Harvest-UpgradeState.ps1
//! -SelfTest, the 12 in tests/owed-selftest.txt), on a real Windows
//! filesystem: a generated tree, a real junction, a folder this account may
//! not list, the offline attribute, allocated bytes, the read-through.
//! Windows only; run through evaluate/harvest/windows-tests.sh.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;
use upgrade_harvest::folders::{allocated_bytes, folder_stats, materialize, materialize_file};

const OFFLINE: u32 = 0x1000;

struct Work(PathBuf);

impl Work {
    fn new(name: &str) -> Work {
        let dir = std::env::temp_dir().join(format!("upgrade-harvest-rust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Work(dir)
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
    fn file(&self, rel: &str, bytes: usize, fill: u8) -> PathBuf {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![fill; bytes]).unwrap();
        p
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = Command::new("icacls").args([self.path("lt\\locked").to_str().unwrap_or(""), "/remove:d", &std::env::var("USERNAME").unwrap_or_default()]).output();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tree(w: &Work) -> PathBuf {
    w.file("tree\\a.txt", 10, b'a');
    w.file("tree\\b.txt", 20, b'b');
    w.file("tree\\sub\\c.txt", 30, b'c');
    w.path("tree")
}

fn set_offline(p: &Path) {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetFileAttributesW, SetFileAttributesW, FILE_FLAGS_AND_ATTRIBUTES};
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let a = GetFileAttributesW(PCWSTR(w.as_ptr()));
        SetFileAttributesW(PCWSTR(w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(a | OFFLINE)).unwrap();
    }
}

#[test]
fn folder_stats_counts_files_and_bytes_recursively() {
    let w = Work::new("count");
    let s = folder_stats(&tree(&w), 250000, 0, "");
    assert_eq!((s.files, s.bytes, s.truncated, s.cloud_only_files), (3, 60, false, 0), "{s:?}");
}

#[test]
fn folder_stats_file_cap_truncates_and_says_so() {
    let w = Work::new("cap");
    let s = folder_stats(&tree(&w), 2, 0, "");
    assert_eq!((s.files, s.truncated), (2, true), "{s:?}");
}

#[test]
fn folder_stats_on_stick_size_is_clusters_plus_directories_plus_manifest_lines() {
    // 3 files x one 4 KB cluster, two directories x one cluster, and the
    // three manifest lines (64 + 2 + len("./staging/Documents/a.txt") + 1 = 92, 92, 96)
    let w = Work::new("stick");
    let s = folder_stats(&tree(&w), 250000, 4096, "Documents");
    assert_eq!((s.stick_bytes, s.max_file_bytes, s.files_over_4gib, s.unreadable), (20760, 30, 0, 0), "{s:?}");
}

#[test]
fn folder_stats_no_cluster_size_no_on_stick_size() {
    let w = Work::new("nocluster");
    assert_eq!(folder_stats(&tree(&w), 250000, 0, "").stick_bytes, 0);
}

#[test]
fn folder_stats_a_junction_is_not_followed() {
    let w = Work::new("junction");
    w.file("jt\\own.txt", 1, b'o');
    w.file("outside\\big.txt", 1000, b'y');
    let link = w.path("jt\\link");
    let made = Command::new("cmd").args(["/c", "mklink", "/J", link.to_str().unwrap(), w.path("outside").to_str().unwrap()]).output().unwrap();
    assert!(made.status.success(), "mklink: {}", String::from_utf8_lossy(&made.stderr));
    let s = folder_stats(&w.path("jt"), 250000, 0, "");
    let _ = std::fs::remove_dir(&link);
    assert_eq!((s.files, s.bytes, s.unreadable), (1, 1, 0), "{s:?}");
}

#[test]
fn folder_stats_a_directory_windows_will_not_list_is_counted_as_unreadable() {
    let w = Work::new("locked");
    w.file("lt\\open.txt", 1, b'o');
    w.file("lt\\locked\\hidden.txt", 1, b'h');
    let locked = w.path("lt\\locked");
    let me = std::env::var("USERNAME").unwrap();
    let denied = Command::new("icacls").args([locked.to_str().unwrap(), "/deny", &format!("{me}:(RD)")]).output().unwrap();
    assert!(denied.status.success(), "icacls: {}", String::from_utf8_lossy(&denied.stdout));
    let s = folder_stats(&w.path("lt"), 250000, 0, "");
    let _ = Command::new("icacls").args([locked.to_str().unwrap(), "/remove:d", &me]).output();
    assert!(s.unreadable >= 1 && s.files == 1 && s.unreadable_first.iter().any(|f| f.contains("locked")), "{s:?}");
}

#[test]
fn folder_stats_offline_attribute_file_counts_as_cloud_only() {
    let w = Work::new("offline");
    let t = tree(&w);
    set_offline(&w.file("tree\\placeholder.txt", 1, b'x'));
    assert_eq!(folder_stats(&t, 250000, 0, "").cloud_only_files, 1);
}

#[test]
fn materialize_a_real_10_byte_file_reports_allocated_bytes() {
    let w = Work::new("alloc");
    tree(&w);
    assert!(allocated_bytes(&w.path("tree\\a.txt")).unwrap() > 0);
}

#[test]
fn materialize_an_ordinary_file_passes_through_as_materialized() {
    let w = Work::new("ordinary");
    tree(&w);
    let m = materialize_file(&w.path("tree\\b.txt"), 20, 30);
    assert!(m.materialized && m.bytes_read == 20 && m.error.is_none(), "{m:?}");
}

#[test]
fn materialize_a_vanished_file_is_a_failure_with_the_reason_not_a_crash() {
    let w = Work::new("vanished");
    tree(&w);
    let m = materialize_file(&w.path("tree\\missing.txt"), 5, 30);
    assert!(!m.materialized && m.error.is_some(), "{m:?}");
}

#[test]
fn materialize_a_stub_no_provider_will_fill_is_reported_failed() {
    // the OFFLINE attribute set by hand is a placeholder the read cannot
    // clear: the refuse arm
    let w = Work::new("stub");
    tree(&w);
    let ph = w.file("tree\\placeholder.txt", 1, b'x');
    set_offline(&ph);
    let m = materialize_file(&ph, 1, 30);
    assert!(!m.materialized, "{m:?}");
}

#[test]
fn materialize_summary_counts_the_stub_as_found_and_failed_result_refused() {
    let w = Work::new("summary");
    let t = tree(&w);
    set_offline(&w.file("tree\\placeholder.txt", 1, b'x'));
    let s = materialize(&[&t], 30, 250000);
    assert_eq!((s.placeholders_found, s.failed, s.result), (1, 1, "refused"), "{s:?}");
}
