//! The harvester's filesystem reads, Windows only: a folder's size as it
//! stands (Get-HarvestFolderStats) and a file's allocated bytes. The
//! on-stick size and the read-through that downloaded cloud placeholders
//! left on 2026-11-26 (the owner): nothing stages files to the stick, and
//! online-only files stay in the cloud (decided 2026-09-26).
//!
//! Three rules from the PowerShell, kept: a junction or a directory symlink
//! is not followed (a folder's size never includes what a link points at);
//! a cloud directory is (it is a reparse point too); a directory Windows
//! will not list is counted as unreadable, never skipped in silence (R6).

use crate::cloud::is_placeholder;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::Storage::FileSystem::{FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH, WIN32_FIND_DATAW};

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

/// Get-HarvestFolderStats. `max_files` bounds the walk and marks the
/// result truncated.
pub fn folder_stats(path: &Path, max_files: i64) -> FolderStats {
    let mut s = FolderStats::default();
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
        }
        // pushed in reverse so the first subdirectory is walked first
        for d in subdirs.into_iter().rev() {
            stack.push(d);
        }
    }
    s.truncated = s.files >= max_files;
    s.unreadable = unreadable_all.len() as i64;
    s.unreadable_first = unreadable_all.into_iter().take(3).collect();
    s
}
