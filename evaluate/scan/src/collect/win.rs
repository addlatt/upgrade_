//! Small Windows helpers the collectors share: elevation, a privilege,
//! firmware variables, local time, running a Windows tool for its text.

use crate::ps::Stamp;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID, SYSTEMTIME};
use windows::Win32::Security::{AdjustTokenPrivileges, AllocateAndInitializeSid, CheckTokenMembership, FreeSid, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, PSID, SECURITY_NT_AUTHORITY, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY};
use windows::Win32::System::SystemServices::{DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
use windows::Win32::System::WindowsProgramming::GetFirmwareEnvironmentVariableExW;

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Test-UpgAdmin: does this process run as an administrator?
pub fn is_admin() -> bool {
    unsafe {
        let mut sid = PSID::default();
        if AllocateAndInitializeSid(&SECURITY_NT_AUTHORITY, 2, SECURITY_BUILTIN_DOMAIN_RID as u32, DOMAIN_ALIAS_RID_ADMINS as u32, 0, 0, 0, 0, 0, 0, &mut sid).is_err() {
            return false;
        }
        let mut member = windows::core::BOOL(0);
        let ok = CheckTokenMembership(None, sid, &mut member).is_ok();
        FreeSid(sid);
        ok && member.as_bool()
    }
}

/// `[Security.Principal.WindowsIdentity]::GetCurrent().Name`: the account
/// this process runs as, named from its token's user SID (`DOMAIN\Name`).
/// For the SYSTEM account that is `NT AUTHORITY\SYSTEM`; `GetUserNameExW`
/// would give the machine account (`WORKGROUP\PC$`) instead, which is what
/// the Rust prologue's first rig run recorded (2026-10-07). Empty when the
/// token cannot be read.
pub fn account_name() -> String {
    use windows::core::PWSTR;
    use windows::Win32::Security::{GetTokenInformation, LookupAccountSidW, TokenUser, SID_NAME_USE, TOKEN_USER};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return String::new();
        }
        let mut size: u32 = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        let mut buf = vec![0u8; size.max(1) as usize];
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut _), size, &mut size).is_ok();
        let _ = CloseHandle(token);
        if !got {
            return String::new();
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let (mut name_len, mut domain_len): (u32, u32) = (0, 0);
        let mut kind = SID_NAME_USE::default();
        let _ = LookupAccountSidW(None, user.User.Sid, None, &mut name_len, None, &mut domain_len, &mut kind);
        let mut name: Vec<u16> = vec![0; name_len.max(1) as usize];
        let mut domain: Vec<u16> = vec![0; domain_len.max(1) as usize];
        if LookupAccountSidW(None, user.User.Sid, Some(PWSTR(name.as_mut_ptr())), &mut name_len, Some(PWSTR(domain.as_mut_ptr())), &mut domain_len, &mut kind).is_err() {
            return String::new();
        }
        let (n, d) = (String::from_utf16_lossy(&name[..name_len as usize]), String::from_utf16_lossy(&domain[..domain_len as usize]));
        if d.is_empty() { n } else { format!("{d}\\{n}") }
    }
}

/// Turn one privilege on for this process (`SeSystemEnvironmentPrivilege`
/// for the firmware's variables). False when it could not be.
pub fn enable_privilege(name: PCWSTR) -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut luid = LUID::default();
        let ok = LookupPrivilegeValueW(None, name, &mut luid).is_ok() && {
            let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
            AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None).is_ok()
        };
        let _ = CloseHandle(token);
        ok
    }
}

/// One UEFI variable's bytes, or nothing when it is not there or cannot be
/// read (the privilege must be on first).
pub fn firmware_variable(name: &str, guid: &str) -> Option<Vec<u8>> {
    let (n, g) = (wide(name), wide(guid));
    let mut buf = vec![0u8; 256 * 1024];
    unsafe {
        let got = GetFirmwareEnvironmentVariableExW(PCWSTR(n.as_ptr()), PCWSTR(g.as_ptr()), Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, None);
        if got == 0 {
            return None;
        }
        buf.truncate(got as usize);
    }
    Some(buf)
}

fn civil_from_seconds(secs: i64) -> Stamp {
    super::utc_from_seconds(secs)
}

/// A UTC moment as this machine's local time, as PowerShell shows every
/// date it reads.
pub fn utc_to_local(utc: Stamp) -> Stamp {
    let st = SYSTEMTIME { wYear: utc.year as u16, wMonth: utc.month as u16, wDay: utc.day as u16, wHour: utc.hour as u16, wMinute: utc.minute as u16, wSecond: utc.second as u16, ..Default::default() };
    let mut local = SYSTEMTIME::default();
    if unsafe { SystemTimeToTzSpecificLocalTime(None, &st, &mut local) }.is_err() {
        return utc;
    }
    Stamp { year: local.wYear as i32, month: local.wMonth as u32, day: local.wDay as u32, hour: local.wHour as u32, minute: local.wMinute as u32, second: local.wSecond as u32 }
}

/// Seconds since 1970 as a UTC stamp.
pub fn utc_from_seconds(secs: i64) -> Stamp {
    civil_from_seconds(secs)
}

pub fn now_utc_seconds() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Local time now.
pub fn now_local() -> Stamp {
    utc_to_local(civil_from_seconds(now_utc_seconds()))
}

/// This account's Desktop folder (`[Environment]::GetFolderPath('Desktop')`).
pub fn desktop_folder() -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};
    let p = unsafe { SHGetKnownFolderPath(&FOLDERID_Desktop, KNOWN_FOLDER_FLAG(0), None) }.ok()?;
    let path = unsafe { p.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(p.0 as *const std::ffi::c_void)) };
    path.map(std::path::PathBuf::from)
}

/// Run a Windows tool and return what it printed (both streams), as lines.
/// A tool that is not there or does not finish in time is an error.
pub fn run_tool(program: &str, args: &[&str], timeout_secs: u64) -> Result<Vec<String>, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{program}: {e}"))?;
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let _ = out.read_to_end(&mut a);
        let _ = err.read_to_end(&mut b);
        (a, b)
    });
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed().as_secs() >= timeout_secs => {
                let _ = child.kill();
                return Err(format!("{program}: timed out after {timeout_secs} s"));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return Err(format!("{program}: {e}")),
        }
    }
    let (a, b) = reader.join().map_err(|_| format!("{program}: output lost"))?;
    let text = decode_console(&a) + &decode_console(&b);
    Ok(text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect())
}

/// Console output as text. The tools print in the OEM code page; what the
/// scanner reads from them is ASCII, so bytes past ASCII become `?`.
fn decode_console(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    bytes.iter().map(|b| if b.is_ascii() { *b as char } else { '?' }).collect()
}
