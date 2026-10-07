//! Registry values, read-only. `HKLM` and `HKCU` only, by full path.

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_VALUE_TYPE, RRF_RT_ANY};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub enum Hive {
    LocalMachine,
    CurrentUser,
}

impl Hive {
    fn handle(&self) -> HKEY {
        match self {
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::CurrentUser => HKEY_CURRENT_USER,
        }
    }
}

/// The raw bytes and type of one value, or nothing when the key or value
/// is not there (or cannot be read).
pub fn value(hive: &Hive, key: &str, name: &str) -> Option<(u32, Vec<u8>)> {
    let (k, n) = (wide(key), wide(name));
    unsafe {
        let mut kind = REG_VALUE_TYPE(0);
        let mut size = 0u32;
        let probe = RegGetValueW(hive.handle(), PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_ANY, Some(&mut kind as *mut REG_VALUE_TYPE), None, Some(&mut size));
        if probe != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let got = RegGetValueW(hive.handle(), PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_ANY, Some(&mut kind as *mut REG_VALUE_TYPE), Some(buf.as_mut_ptr() as *mut _), Some(&mut size));
        if got != ERROR_SUCCESS {
            return None;
        }
        buf.truncate(size as usize);
        Some((kind.0, buf))
    }
}

/// A REG_DWORD, as `Get-ItemProperty` would give it.
pub fn dword(hive: &Hive, key: &str, name: &str) -> Option<i64> {
    let (kind, bytes) = value(hive, key, name)?;
    // REG_DWORD = 4
    (kind == 4 && bytes.len() >= 4).then(|| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64)
}

/// A REG_SZ or REG_EXPAND_SZ, without its terminator.
pub fn string(hive: &Hive, key: &str, name: &str) -> Option<String> {
    let (kind, bytes) = value(hive, key, name)?;
    // REG_SZ = 1, REG_EXPAND_SZ = 2 (RegGetValue expands these)
    if kind != 1 && kind != 2 {
        return None;
    }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    Some(String::from_utf16_lossy(&units))
}

/// The names of a key's values, in the registry's order.
pub fn value_names(hive: &Hive, key: &str) -> Vec<String> {
    use windows::Win32::System::Registry::RegEnumValueW;
    let k = wide(key);
    let mut out = Vec::new();
    unsafe {
        let mut h = HKEY::default();
        if RegOpenKeyExW(hive.handle(), PCWSTR(k.as_ptr()), Some(0), KEY_READ, &mut h) != ERROR_SUCCESS {
            return out;
        }
        let mut i = 0u32;
        loop {
            let mut name = [0u16; 16384];
            let mut len = name.len() as u32;
            let r = RegEnumValueW(h, i, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None);
            if r != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
            i += 1;
        }
        let _ = RegCloseKey(h);
    }
    out
}

/// The names of a key's subkeys, in the registry's order.
pub fn subkeys(hive: &Hive, key: &str) -> Vec<String> {
    let k = wide(key);
    let mut out = Vec::new();
    unsafe {
        let mut h = HKEY::default();
        if RegOpenKeyExW(hive.handle(), PCWSTR(k.as_ptr()), Some(0), KEY_READ, &mut h) != ERROR_SUCCESS {
            return out;
        }
        let mut i = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            let r = RegEnumKeyExW(h, i, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None);
            if r != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
            i += 1;
        }
        let _ = RegCloseKey(h);
    }
    out
}
