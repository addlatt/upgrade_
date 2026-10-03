//! What only Windows can do for the gate: list the drives WinPE sees (serial,
//! exact size, model), find which drive holds the stick, read a key and clear
//! the screen. Read-only toward every drive: the only writes the gate ever
//! makes are its record files on the stick. Off Windows these are stand-ins,
//! so the gate's logic builds and tests on Linux.

#[cfg(windows)]
mod imp {
    use crate::logic::Seen;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{FlushConsoleInputBuffer, GetStdHandle, ReadConsoleInputW, INPUT_RECORD, KEY_EVENT, STD_INPUT_HANDLE};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405C;
    const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002D_1080;

    fn w(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    /// Opened for reading at most (the length query needs read access; WinPE
    /// runs as SYSTEM), else with no access rights. Never for writing.
    fn open_query(path: &str) -> Option<HANDLE> {
        const GENERIC_READ: u32 = 0x8000_0000;
        let p = w(path);
        for access in [GENERIC_READ, 0] {
            let h = unsafe { CreateFileW(p.as_ptr(), access, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
            if h != INVALID_HANDLE_VALUE {
                return Some(h);
            }
        }
        None
    }

    fn ioctl(h: HANDLE, code: u32, input: &[u8], out: &mut [u8]) -> Option<usize> {
        let mut n: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(h, code, input.as_ptr() as _, input.len() as u32, out.as_mut_ptr() as _, out.len() as u32, &mut n, std::ptr::null_mut())
        };
        if ok == 0 { None } else { Some(n as usize) }
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).unwrap_or(0)
    }

    fn cstr_at(b: &[u8], off: u32) -> String {
        let o = off as usize;
        if o == 0 || o >= b.len() {
            return String::new();
        }
        let end = b[o..].iter().position(|&c| c == 0).map(|e| o + e).unwrap_or(b.len());
        String::from_utf8_lossy(&b[o..end]).trim().to_string()
    }

    /// Every \\.\PhysicalDriveN WinPE shows, with serial, exact size and model.
    pub fn list_disks() -> Vec<Seen> {
        let mut out = Vec::new();
        for n in 0..64u32 {
            let Some(h) = open_query(&format!("\\\\.\\PhysicalDrive{}", n)) else { continue };
            // STORAGE_PROPERTY_QUERY { StorageDeviceProperty, PropertyStandardQuery, [0] }
            let q = [0u8; 12];
            let mut d = vec![0u8; 4096];
            let (mut serial, mut model) = (String::new(), String::new());
            if ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q, &mut d).is_some() {
                // STORAGE_DEVICE_DESCRIPTOR: VendorIdOffset @12, ProductIdOffset @16, SerialNumberOffset @24
                let vendor = cstr_at(&d, u32_at(&d, 12));
                let product = cstr_at(&d, u32_at(&d, 16));
                model = if vendor.is_empty() { product } else { format!("{} {}", vendor, product) };
                serial = cstr_at(&d, u32_at(&d, 24));
            }
            // StorageDeviceIdProperty (2): the VPD page 0x83 identifiers (NAA, EUI-64)
            let mut ids = Vec::new();
            let mut q2 = [0u8; 12];
            q2[0] = 2;
            let mut b = vec![0u8; 4096];
            if let Some(got) = ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q2, &mut b) {
                let count = u32_at(&b, 8) as usize;
                let mut at = 12usize;
                for _ in 0..count {
                    if at + 16 > got {
                        break;
                    }
                    let (code_set, kind) = (u32_at(&b, at), u32_at(&b, at + 4));
                    let size = u16::from_le_bytes([b[at + 8], b[at + 9]]) as usize;
                    let next = u16::from_le_bytes([b[at + 10], b[at + 11]]) as usize;
                    // binary (1) EUI-64 (2) or FCPH/NAA (3)
                    if code_set == 1 && (kind == 2 || kind == 3) && at + 16 + size <= got {
                        ids.push(b[at + 16..at + 16 + size].iter().map(|x| format!("{:02x}", x)).collect::<String>());
                    }
                    if next == 0 {
                        break;
                    }
                    at += next;
                }
            }
            let mut len = [0u8; 8];
            let size = if ioctl(h, IOCTL_DISK_GET_LENGTH_INFO, &[], &mut len).is_some() { u64::from_le_bytes(len) } else { 0 };
            unsafe { CloseHandle(h) };
            out.push(Seen { number: n, serial, size, model, ids });
        }
        out
    }

    /// The physical drive number holding a volume ("D:"), if Windows says.
    pub fn disk_of_volume(letter: &str) -> Option<u32> {
        let h = open_query(&format!("\\\\.\\{}", letter.trim_end_matches('\\')))?;
        let mut b = [0u8; 12];
        let r = ioctl(h, IOCTL_STORAGE_GET_DEVICE_NUMBER, &[], &mut b);
        unsafe { CloseHandle(h) };
        r.map(|_| u32_at(&b, 4))
    }

    pub fn flush_keys() {
        unsafe { FlushConsoleInputBuffer(GetStdHandle(STD_INPUT_HANDLE)) };
    }

    /// Waits up to `ms` for a key to go down. Any key counts.
    pub fn key_within(ms: u32) -> bool {
        let h = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let start = std::time::Instant::now();
        loop {
            let spent = start.elapsed().as_millis() as u32;
            if spent >= ms {
                return false;
            }
            if unsafe { WaitForSingleObject(h, ms - spent) } != WAIT_OBJECT_0 {
                return false;
            }
            let mut rec: [INPUT_RECORD; 16] = unsafe { std::mem::zeroed() };
            let mut n: u32 = 0;
            if unsafe { ReadConsoleInputW(h, rec.as_mut_ptr(), rec.len() as u32, &mut n) } == 0 {
                return false;
            }
            for r in &rec[..n as usize] {
                if r.EventType as u32 == KEY_EVENT && unsafe { r.Event.KeyEvent.bKeyDown } != 0 {
                    return true;
                }
            }
        }
    }

    pub fn clear() {
        let _ = std::process::Command::new("cmd.exe").args(["/c", "cls"]).status();
    }

    pub fn reboot() {
        let _ = std::process::Command::new("wpeutil.exe").arg("reboot").status();
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::logic::Seen;
    pub fn list_disks() -> Vec<Seen> { Vec::new() }
    pub fn disk_of_volume(_: &str) -> Option<u32> { None }
    pub fn flush_keys() {}
    pub fn key_within(ms: u32) -> bool { std::thread::sleep(std::time::Duration::from_millis(ms as u64)); false }
    pub fn clear() {}
    pub fn reboot() {}
}

pub use imp::*;
