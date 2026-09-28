//! The few things only Windows can do for the window: elevation, a plain
//! message box when the window cannot draw, the stick's volume name, and the
//! one-shot sign-in task that reopens the window after the restart. Off
//! Windows these are stand-ins, so the window's logic builds and tests on
//! the Linux side of the repo.

#[allow(dead_code)]
pub const REOPEN_TASK: &str = "upgrade_ window reopen";

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
    use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK, SW_SHOWNORMAL};

    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn w(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    pub fn is_elevated() -> bool {
        unsafe { IsUserAnAdmin() != 0 }
    }

    /// Start this program again through the administrator prompt (the same
    /// thing the .cmd launchers do with Start-Process -Verb RunAs).
    pub fn relaunch_elevated(args: &str) -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        let (verb, file, params) = (w("runas"), w(&exe.to_string_lossy()), w(args));
        let h = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), std::ptr::null(), SW_SHOWNORMAL) };
        h as isize > 32
    }

    pub fn message_box(title: &str, text: &str) {
        let (t, x) = (w(title), w(text));
        unsafe { MessageBoxW(std::ptr::null_mut(), x.as_ptr(), t.as_ptr(), MB_OK | MB_ICONINFORMATION) };
    }

    /// "E:\" -> "\\?\Volume{...}\": the name the stick keeps if its letter changes.
    pub fn volume_path(root: &str) -> Option<String> {
        let r = w(root);
        let mut buf = [0u16; 64];
        let ok = unsafe { GetVolumeNameForVolumeMountPointW(r.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if ok == 0 {
            return None;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }

    fn schtasks(args: &[&str]) -> Result<(), String> {
        let o = Command::new("schtasks.exe").args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
        if o.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
        }
    }

    pub fn register_reopen(xml: &str, dir: &std::path::Path) -> Result<(), String> {
        // schtasks reads task XML as UTF-16 with a byte-order mark
        let mut b: Vec<u8> = vec![0xFF, 0xFE];
        for u in xml.encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        let p = dir.join("reopen-task.xml");
        std::fs::write(&p, b).map_err(|e| e.to_string())?;
        let r = schtasks(&["/Create", "/TN", super::REOPEN_TASK, "/XML", &p.to_string_lossy(), "/F"]);
        let _ = std::fs::remove_file(&p);
        r?;
        schtasks(&["/Query", "/TN", super::REOPEN_TASK]).map_err(|e| format!("the task is not there after it was made ({})", e))
    }

    pub fn unregister_reopen() -> bool {
        schtasks(&["/Delete", "/TN", super::REOPEN_TASK, "/F"]).is_ok()
    }

    /// The copy in ProgramData cannot delete itself while it runs: a hidden
    /// cmd waits a few seconds after the window closes, then removes it.
    pub fn remove_after_exit(dir: &std::path::Path) {
        let _ = Command::new("cmd.exe")
            .raw_arg(format!("/C ping -n 4 127.0.0.1 >nul & rmdir /S /Q \"{}\"", dir.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

#[cfg(not(windows))]
mod imp {
    #[allow(dead_code)]
    pub const CREATE_NO_WINDOW: u32 = 0;
    pub fn is_elevated() -> bool {
        true
    }
    pub fn relaunch_elevated(_args: &str) -> bool {
        false
    }
    pub fn message_box(title: &str, text: &str) {
        eprintln!("{}: {}", title, text);
    }
    pub fn volume_path(_root: &str) -> Option<String> {
        None
    }
    pub fn register_reopen(_xml: &str, _dir: &std::path::Path) -> Result<(), String> {
        Err("the sign-in task exists only on Windows".into())
    }
    pub fn unregister_reopen() -> bool {
        false
    }
    pub fn remove_after_exit(_dir: &std::path::Path) {}
}

pub use imp::*;
