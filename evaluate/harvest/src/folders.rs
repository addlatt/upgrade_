//! The harvester's filesystem reads, Windows only: a folder's size as it
//! stands and as staging would lay it on the stick (Get-HarvestFolderStats),
//! a file's allocated bytes, and the read-through that brings a cloud
//! placeholder home (Invoke-HarvestMaterializeFile, Invoke-HarvestMaterialize).
//!
//! Three rules from the PowerShell, kept: a junction or a directory symlink
//! is not followed (a folder's size never includes what a link points at);
//! a cloud directory is (it is a reparse point too); a directory Windows
//! will not list is counted as unreadable, never skipped in silence (R6).

use crate::cloud::{is_materialized, is_placeholder};
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::Storage::FileSystem::{FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, GetCompressedFileSizeW, GetFileAttributesW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH, INVALID_FILE_ATTRIBUTES, WIN32_FIND_DATAW};

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderStats {
    pub files: i64,
    pub bytes: i64,
    pub cloud_only_files: i64,
    /// the file cap stopped the count; the size is a low estimate
    pub truncated: bool,
    /// directories Windows would not list
    pub unreadable: i64,
    /// the first three of them
    pub unreadable_first: Vec<String>,
    pub max_file_bytes: i64,
    pub files_over_4gib: i64,
    /// the folder's size on the stick: clusters, directories, manifest lines (0 when no cluster size was given)
    pub stick_bytes: i64,
}

/// Whole clusters: how many of `unit` cover `n`.
fn ceil_div(n: i64, unit: i64) -> i64 {
    if unit <= 0 { 0 } else { (n + unit - 1) / unit }
}

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn name_of(d: &WIN32_FIND_DATAW) -> String {
    let end = d.cFileName.iter().position(|u| *u == 0).unwrap_or(d.cFileName.len());
    String::from_utf16_lossy(&d.cFileName[..end])
}

struct Entry {
    name: String,
    attributes: u32,
    reparse_tag: u32,
    size: i64,
}

/// One directory's entries, or the error listing it gave.
fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let pattern = wide(&dir.join("*"));
    let mut data = WIN32_FIND_DATAW::default();
    let handle: HANDLE = unsafe { FindFirstFileExW(PCWSTR(pattern.as_ptr()), FindExInfoBasic, &mut data as *mut _ as *mut _, FindExSearchNameMatch, None, FIND_FIRST_EX_LARGE_FETCH) }.map_err(|e| e.message())?;
    let mut out = Vec::new();
    loop {
        let name = name_of(&data);
        if name != "." && name != ".." {
            out.push(Entry { name, attributes: data.dwFileAttributes, reparse_tag: data.dwReserved0, size: ((data.nFileSizeHigh as i64) << 32) | data.nFileSizeLow as i64 });
        }
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break;
        }
    }
    let last = windows::core::Error::from_thread();
    unsafe {
        let _ = FindClose(handle);
    }
    if last.code() != ERROR_NO_MORE_FILES.to_hresult() && last.code().0 != 0 {
        return Err(last.message());
    }
    Ok(out)
}

/// Get-HarvestFolderStats. `cluster_bytes` > 0 also sizes the folder as the
/// prologue's staging lays it on the stick; `stage_name` is its name under
/// `staging/`. `max_files` bounds the walk and marks the result truncated.
pub fn folder_stats(path: &Path, max_files: i64, cluster_bytes: i64, stage_name: &str) -> FolderStats {
    let mut s = FolderStats::default();
    let mut stick_files: i64 = 0;
    let mut manifest: i64 = 0;
    let mut dir_entries: Vec<(PathBuf, i64)> = Vec::new();
    let root = path.to_string_lossy().trim_end_matches('\\').to_string();
    let mut unreadable_all: Vec<String> = Vec::new();
    // depth first, a directory's files before its subdirectories, as
    // Get-ChildItem -Recurse walks
    let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];
    'walk: while let Some(dir) = stack.pop() {
        let entries = match list(&dir) {
            Ok(e) => e,
            Err(_) => {
                unreadable_all.push(dir.to_string_lossy().into_owned());
                continue;
            }
        };
        let mut subdirs = Vec::new();
        for e in &entries {
            let is_dir = e.attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
            let is_reparse = e.attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
            if is_dir {
                let is_link = is_reparse && (e.reparse_tag == IO_REPARSE_TAG_MOUNT_POINT || e.reparse_tag == IO_REPARSE_TAG_SYMLINK);
                if !is_link {
                    subdirs.push(dir.join(&e.name));
                }
                continue;
            }
            if s.files >= max_files {
                break 'walk;
            }
            s.files += 1;
            s.bytes += e.size;
            if e.size > s.max_file_bytes {
                s.max_file_bytes = e.size;
            }
            if e.size > 4294967295 {
                s.files_over_4gib += 1;
            }
            if is_placeholder(e.attributes as i64) {
                s.cloud_only_files += 1;
            }
            if cluster_bytes > 0 {
                stick_files += ceil_div(e.size, cluster_bytes) * cluster_bytes;
                let full = dir.join(&e.name).to_string_lossy().into_owned();
                let rel = full.get(root.len()..).unwrap_or("").trim_start_matches('\\').replace('\\', "/");
                // "<64 hex>  ./staging/<Name>/<rel>\n", as the prologue writes the manifest
                manifest += 64 + 2 + format!("./staging/{stage_name}/{rel}").len() as i64 + 1;
                // a directory entry set per file: FAT32 1 + ceil(name/13), exFAT
                // 2 + ceil(name/15) entries of 32 bytes; the larger of the two
                let name_units = e.name.encode_utf16().count() as i64;
                let entry_bytes = 32 * (2 + ceil_div(name_units, 13));
                match dir_entries.iter_mut().find(|(d, _)| *d == dir) {
                    Some((_, n)) => *n += entry_bytes,
                    None => dir_entries.push((dir.clone(), entry_bytes)),
                }
            }
        }
        // pushed in reverse so the first subdirectory is walked first
        for d in subdirs.into_iter().rev() {
            stack.push(d);
        }
    }
    if cluster_bytes > 0 {
        let dir_bytes: i64 = dir_entries.iter().map(|(_, n)| ceil_div(*n, cluster_bytes).max(1) * cluster_bytes).sum();
        s.stick_bytes = stick_files + dir_bytes + manifest;
    }
    s.truncated = s.files >= max_files;
    s.unreadable = unreadable_all.len() as i64;
    s.unreadable_first = unreadable_all.into_iter().take(3).collect();
    s
}

/// The bytes a file occupies on its volume. A dehydrated placeholder
/// allocates nothing; a sparse or compressed file less than its length,
/// which is why this is read together with the attributes, never alone.
pub fn allocated_bytes(path: &Path) -> Result<i64, String> {
    let prefixed = wide(&PathBuf::from(format!("\\\\?\\{}", path.to_string_lossy())));
    let mut high = 0u32;
    let low = unsafe { GetCompressedFileSizeW(PCWSTR(prefixed.as_ptr()), Some(&mut high)) };
    if low == u32::MAX {
        let e = windows::core::Error::from_thread();
        if e.code().0 != 0 {
            return Err(e.message());
        }
    }
    Ok(((high as i64) << 32) | low as i64)
}

pub fn attributes(path: &Path) -> Option<u32> {
    let w = wide(path);
    let a = unsafe { GetFileAttributesW(PCWSTR(w.as_ptr())) };
    (a != INVALID_FILE_ATTRIBUTES).then_some(a)
}

/// One file after the read-through (Invoke-HarvestMaterializeFile).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Materialized {
    pub path: String,
    pub length: i64,
    pub pinned: bool,
    pub bytes_read: i64,
    pub attributes_after: Option<i64>,
    pub allocated_bytes: Option<i64>,
    pub materialized: bool,
    pub error: Option<String>,
    pub seconds: f64,
}

/// Pin the file ("always keep on this device"), read it through with a
/// timeout, then re-read its facts and judge. Never panics on a file that
/// vanished or a fetch the cloud filter refused: that is a record with its
/// reason, which the caller must refuse on.
pub fn materialize_file(path: &Path, length: i64, timeout_secs: u64) -> Materialized {
    use std::io::Read;
    let start = std::time::Instant::now();
    let mut r = Materialized { path: path.to_string_lossy().into_owned(), length, ..Default::default() };
    r.pinned = std::process::Command::new("attrib.exe").args(["+P", "-U", &r.path]).output().is_ok_and(|o| o.status.success());
    let read = (|| -> Result<i64, String> {
        let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut total: i64 = 0;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if start.elapsed().as_secs() >= timeout_secs {
                return Err(format!("timed out after {timeout_secs} s with {total} of {length} bytes"));
            }
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Ok(total);
            }
            total += n as i64;
        }
    })();
    match read {
        Ok(n) => r.bytes_read = n,
        Err(e) => r.error = Some(e.split_whitespace().collect::<Vec<_>>().join(" ")),
    }
    r.attributes_after = attributes(path).map(|a| a as i64);
    match allocated_bytes(path) {
        Ok(a) => r.allocated_bytes = Some(a),
        Err(e) => {
            if r.error.is_none() {
                r.error = Some(e);
            }
        }
    }
    r.materialized = match (r.attributes_after, r.allocated_bytes) {
        (Some(a), Some(alloc)) => is_materialized(a, length, r.bytes_read, alloc),
        _ => false,
    };
    r.seconds = (start.elapsed().as_secs_f64() * 100.0).round() / 100.0;
    r
}

/// Every cloud-only placeholder under a directory (Get-HarvestPlaceholders).
/// Listing reads directory entries only; it hydrates nothing.
pub fn placeholders(path: &Path, max_files: i64) -> Vec<(PathBuf, i64, u32)> {
    let mut found = Vec::new();
    let mut seen: i64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = list(&dir) else { continue };
        let mut subdirs = Vec::new();
        for e in &entries {
            if e.attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
                if !(e.attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 && (e.reparse_tag == IO_REPARSE_TAG_MOUNT_POINT || e.reparse_tag == IO_REPARSE_TAG_SYMLINK)) {
                    subdirs.push(dir.join(&e.name));
                }
                continue;
            }
            seen += 1;
            if seen > max_files {
                return found;
            }
            if is_placeholder(e.attributes as i64) {
                found.push((dir.join(&e.name), e.size, e.attributes));
            }
        }
        for d in subdirs.into_iter().rev() {
            stack.push(d);
        }
    }
    found
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaterializeSummary {
    pub placeholders_found: i64,
    pub materialized: i64,
    pub failed: i64,
    pub bytes: i64,
    /// `none-found`, `materialized` or `refused`
    pub result: &'static str,
    pub files: Vec<Materialized>,
}

/// Invoke-HarvestMaterialize: every placeholder under the paths. `failed`
/// above zero means the caller writes no job at all.
pub fn materialize(paths: &[&Path], timeout_secs: u64, max_files: i64) -> MaterializeSummary {
    let mut all = Vec::new();
    for p in paths.iter().filter(|p| p.exists()) {
        all.extend(placeholders(p, max_files));
    }
    let files: Vec<Materialized> = all.iter().map(|(p, len, _)| materialize_file(p, *len, timeout_secs)).collect();
    let ok = files.iter().filter(|f| f.materialized).count() as i64;
    let bad = files.len() as i64 - ok;
    MaterializeSummary {
        placeholders_found: all.len() as i64,
        materialized: ok,
        failed: bad,
        bytes: files.iter().filter(|f| f.materialized).map(|f| f.length).sum(),
        result: if all.is_empty() { "none-found" } else if bad == 0 { "materialized" } else { "refused" },
        files,
    }
}
