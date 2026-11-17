//! Placing this program's copy in the state directory, from which a task
//! runs it. The deciding half, apart from Windows, so its cases run anywhere.
//!
//! A copy that already holds the same bytes is left alone: the previous
//! check or resume may still be running from it (its last popup), and
//! Windows refuses to overwrite a running program. The rig's erase arm A
//! found this on 2026-10-09.
//!
//! A copy that holds OTHER bytes and is still running is moved aside, and
//! the new one put in its place: Windows lets a running program be renamed.
//! The Aspire found this on 2026-11-15 (`oneclick-runs.csv` line 41): a
//! notice window of prologue 0.4.4 was still open when 0.4.5 started, the
//! copy failed with "being used by another process", and the prologue
//! stopped and undid its own work.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// What `place` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Placed {
    /// `me` is the copy itself.
    Itself,
    /// the copy already held these bytes.
    Same,
    Copied,
    /// the old copy was in use: it now has this name, and the new one is in place.
    CopiedOldAside(PathBuf),
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase(),
        _ => false,
    }
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => Sha256::digest(&a) == Sha256::digest(&b),
        _ => false,
    }
}

/// The name an in-use copy is moved to: `upgrade-prologue.old-<tag>.exe`.
pub fn aside_name(copy: &Path, tag: &str) -> PathBuf {
    let stem = copy.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = copy.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    copy.with_file_name(format!("{stem}.old-{tag}{ext}"))
}

/// Copies moved aside by earlier runs, removed when nothing runs from them
/// any more (one still running refuses the delete and stays).
pub fn sweep_aside(copy: &Path) {
    let (Some(dir), Some(stem)) = (copy.parent(), copy.file_stem().map(|s| s.to_string_lossy().to_string())) else { return };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().starts_with(&format!("{stem}.old-")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// `copy_fn` is `std::fs::copy` (a test's stands in for a program in use);
/// `tag` makes the aside name (the time, in the live program).
pub fn place(me: &Path, copy: &Path, tag: &str, copy_fn: &dyn Fn(&Path, &Path) -> std::io::Result<u64>) -> Result<Placed, String> {
    if same_file(me, copy) {
        return Ok(Placed::Itself);
    }
    sweep_aside(copy);
    if copy.exists() && same_bytes(me, copy) {
        return Ok(Placed::Same);
    }
    let first = match copy_fn(me, copy) {
        Ok(_) => return Ok(Placed::Copied),
        Err(_) if copy.exists() && same_bytes(me, copy) => return Ok(Placed::Same),
        Err(e) => e,
    };
    if !copy.exists() {
        return Err(format!("copying the program to the state directory: {first}"));
    }
    // other bytes, and in use: aside it goes (a running program can be renamed, not overwritten)
    let aside = aside_name(copy, tag);
    std::fs::rename(copy, &aside).map_err(|e| format!("copying the program to the state directory: {first}; and the copy in use could not be moved aside: {e}"))?;
    match copy_fn(me, copy) {
        Ok(_) if same_bytes(me, copy) => Ok(Placed::CopiedOldAside(aside)),
        other => {
            // put the old one back: the tasks that name this path must still find a program
            let _ = std::fs::remove_file(copy);
            let _ = std::fs::rename(&aside, copy);
            Err(format!("copying the program to the state directory after moving the copy in use aside: {}", match other {
                Err(e) => e.to_string(),
                Ok(_) => "the new copy does not hold the program's bytes".to_string(),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("upg-place-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A copy that fails while the destination exists, as Windows does over a running program.
    fn locked(me: &Path, to: &Path) -> std::io::Result<u64> {
        if to.exists() {
            return Err(std::io::Error::other("The process cannot access the file because it is being used by another process."));
        }
        std::fs::copy(me, to)
    }

    #[test]
    fn a_fresh_copy_and_the_same_bytes_again() {
        let d = dir("fresh");
        let (me, copy) = (d.join("me.exe"), d.join("state").join("upgrade-prologue.exe"));
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::write(&me, b"0.4.6").unwrap();
        assert_eq!(place(&me, &copy, "t", &|a, b| std::fs::copy(a, b)), Ok(Placed::Copied));
        // the same bytes are never copied again, in use or not
        assert_eq!(place(&me, &copy, "t", &|_, _| panic!("no copy is tried")), Ok(Placed::Same));
        assert_eq!(place(&copy, &copy, "t", &|_, _| panic!("no copy is tried")), Ok(Placed::Itself));
    }

    #[test]
    fn another_version_still_running_is_moved_aside_and_the_new_one_placed() {
        // the Aspire, 2026-11-15: 0.4.4's notice window open, 0.4.5 arriving
        let d = dir("aside");
        let (me, copy) = (d.join("me.exe"), d.join("upgrade-prologue.exe"));
        std::fs::write(&me, b"0.4.5").unwrap();
        std::fs::write(&copy, b"0.4.4").unwrap();
        let got = place(&me, &copy, "20261115T013500Z", &locked).unwrap();
        let aside = d.join("upgrade-prologue.old-20261115T013500Z.exe");
        assert_eq!(got, Placed::CopiedOldAside(aside.clone()));
        assert_eq!(std::fs::read(&copy).unwrap(), b"0.4.5");
        assert_eq!(std::fs::read(&aside).unwrap(), b"0.4.4", "the running program keeps its bytes under the new name");
        // the next placing sweeps the old one away once nothing runs from it
        std::fs::write(&me, b"0.4.6").unwrap();
        assert_eq!(place(&me, &copy, "t2", &|a, b| std::fs::copy(a, b)), Ok(Placed::Copied));
        assert!(!aside.exists());
    }

    #[test]
    fn a_copy_that_fails_twice_puts_the_old_program_back() {
        let d = dir("back");
        let (me, copy) = (d.join("me.exe"), d.join("upgrade-prologue.exe"));
        std::fs::write(&me, b"new").unwrap();
        std::fs::write(&copy, b"old").unwrap();
        let calls = Cell::new(0);
        let r = place(&me, &copy, "t", &|_, _| {
            calls.set(calls.get() + 1);
            Err(std::io::Error::other("disk full"))
        });
        assert!(r.unwrap_err().contains("disk full"));
        assert_eq!(calls.get(), 2);
        assert_eq!(std::fs::read(&copy).unwrap(), b"old", "the tasks that name this path still find a program");
        assert!(!aside_name(&copy, "t").exists());
    }

    #[test]
    fn no_copy_there_and_a_failing_copy_is_the_plain_error() {
        let d = dir("plain");
        let (me, copy) = (d.join("me.exe"), d.join("upgrade-prologue.exe"));
        std::fs::write(&me, b"new").unwrap();
        let r = place(&me, &copy, "t", &|_, _| Err(std::io::Error::other("access denied")));
        assert_eq!(r.unwrap_err(), "copying the program to the state directory: access denied");
    }
}
