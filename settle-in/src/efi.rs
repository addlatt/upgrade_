//! The firmware's boot entries, through efivarfs (/sys/firmware/efi/efivars,
//! the kernel's standard view of UEFI variables, the same on every
//! distribution) - no efibootmgr. Each variable file is 4 bytes of
//! attributes, then the data. The kernel marks most of them immutable, so a
//! write or a delete clears that flag first (as efibootmgr does).

use std::io::Write;
use std::os::fd::AsRawFd;

pub const GLOBAL: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";
const ATTRS_NV_BS_RT: u32 = 0x7;

#[derive(Debug, Clone, PartialEq)]
pub struct LoadOption {
    pub description: String,
    /// The hard-drive node's partition id (GPT unique GUID, as stored), if any.
    pub partition: Option<[u8; 16]>,
    /// The file path node, e.g. \EFI\Microsoft\Boot\bootmgfw.efi
    pub path: Option<String>,
}

fn utf16z(b: &[u8]) -> (String, usize) {
    let mut u = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        let c = u16::from_le_bytes([b[i], b[i + 1]]);
        i += 2;
        if c == 0 {
            break;
        }
        u.push(c);
    }
    (String::from_utf16_lossy(&u), i)
}

/// EFI_LOAD_OPTION (UEFI spec 3.1.3): attributes u32, path list length u16,
/// description (UCS-2, NUL-ended), then the device path nodes.
pub fn parse_load_option(d: &[u8]) -> Option<LoadOption> {
    if d.len() < 8 {
        return None;
    }
    let fpl = u16::from_le_bytes([d[4], d[5]]) as usize;
    let (description, used) = utf16z(&d[6..]);
    let start = 6 + used;
    let paths = d.get(start..start + fpl)?;
    let mut partition = None;
    let mut path = None;
    let mut i = 0;
    while i + 4 <= paths.len() {
        let (t, st) = (paths[i], paths[i + 1]);
        let len = u16::from_le_bytes([paths[i + 2], paths[i + 3]]) as usize;
        if len < 4 || i + len > paths.len() {
            return None;
        }
        let node = &paths[i..i + len];
        match (t, st) {
            (0x7f, 0xff) => break,
            // hard drive: partition number, start, size, signature[16], format, signature type (2 = GUID)
            (0x04, 0x01) if len >= 42 && node[41] == 0x02 => {
                let mut g = [0u8; 16];
                g.copy_from_slice(&node[24..40]);
                partition = Some(g);
            }
            (0x04, 0x04) => path = Some(utf16z(&node[4..]).0),
            _ => {}
        }
        i += len;
    }
    Some(LoadOption { description, partition, path })
}

pub fn var_path(root: &str, name: &str) -> String {
    format!("{}/sys/firmware/efi/efivars/{}-{}", root.trim_end_matches('/'), name, GLOBAL)
}

/// A variable's data (without the 4 attribute bytes), or None if absent.
pub fn read_var(root: &str, name: &str) -> Option<Vec<u8>> {
    let b = std::fs::read(var_path(root, name)).ok()?;
    if b.len() < 4 { None } else { Some(b[4..].to_vec()) }
}

pub fn u16_list(d: &[u8]) -> Vec<u16> {
    d.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

pub fn boot_entries(root: &str) -> Vec<(u16, LoadOption)> {
    let dir = format!("{}/sys/firmware/efi/efivars", root.trim_end_matches('/'));
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let Some(rest) = n.strip_prefix("Boot") else { continue };
            let Some(hex) = rest.strip_suffix(&format!("-{}", GLOBAL)) else { continue };
            if hex.len() != 4 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) || hex.bytes().any(|c| c.is_ascii_lowercase()) {
                continue;
            }
            let Ok(num) = u16::from_str_radix(hex, 16) else { continue };
            if let Some(d) = read_var(root, &format!("Boot{}", hex))
                && let Some(lo) = parse_load_option(&d) {
                    v.push((num, lo));
                }
        }
    }
    v.sort_by_key(|x| x.0);
    v
}

// FS_IOC_GETFLAGS / FS_IOC_SETFLAGS and FS_IMMUTABLE_FL, from <linux/fs.h>
const FS_IOC_GETFLAGS: libc::c_ulong = 0x8008_6601;
const FS_IOC_SETFLAGS: libc::c_ulong = 0x4008_6602;
const FS_IMMUTABLE_FL: libc::c_int = 0x10;

fn clear_immutable(path: &str) {
    if let Ok(f) = std::fs::File::open(path) {
        let mut flags: libc::c_int = 0;
        // SAFETY: both ioctls take a pointer to an int (the kernel's actual type, as efibootmgr uses).
        unsafe {
            if libc::ioctl(f.as_raw_fd(), FS_IOC_GETFLAGS as _, &mut flags) == 0 && flags & FS_IMMUTABLE_FL != 0 {
                flags &= !FS_IMMUTABLE_FL;
                libc::ioctl(f.as_raw_fd(), FS_IOC_SETFLAGS as _, &flags);
            }
        }
    }
}

/// Replace a variable's data, keeping the usual attributes (non-volatile,
/// boot and runtime access). One write, as efivarfs requires; opened the
/// way libefivar opens it (no truncation - efivarfs replaces the whole
/// variable on each write).
pub fn write_var(root: &str, name: &str, data: &[u8]) -> Result<(), String> {
    let p = var_path(root, name);
    clear_immutable(&p);
    let mut buf = ATTRS_NV_BS_RT.to_le_bytes().to_vec();
    buf.extend_from_slice(data);
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&p).map_err(|e| format!("{}: {}", p, e))?;
    f.write_all(&buf).map_err(|e| format!("{}: {}", p, e))?;
    // efivarfs replaces the whole variable on a write; anything else (a test's
    // copy of a machine) is an ordinary file whose old tail must be cut off
    if !on_efivarfs(&f) {
        f.set_len(buf.len() as u64).map_err(|e| format!("{}: {}", p, e))?;
    }
    Ok(())
}

const EFIVARFS_MAGIC: i64 = 0xde5e_81e4;

fn on_efivarfs(f: &std::fs::File) -> bool {
    // SAFETY: fstatfs fills the struct it is given.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // f_type is a different integer type on glibc and musl; the cast is needed on one of them
    #[allow(clippy::unnecessary_cast)]
    unsafe { libc::fstatfs(f.as_raw_fd(), &mut st) == 0 && st.f_type as i64 == EFIVARFS_MAGIC }
}

pub fn delete_var(root: &str, name: &str) -> Result<(), String> {
    let p = var_path(root, name);
    clear_immutable(&p);
    std::fs::remove_file(&p).map_err(|e| format!("{}: {}", p, e))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A load option like the one Windows' installer writes.
    pub fn load_option(desc: &str, part: [u8; 16], path: &str) -> Vec<u8> {
        let mut dp = Vec::new();
        let mut hd = vec![0x04, 0x01, 42, 0];
        hd.extend_from_slice(&1u32.to_le_bytes());
        hd.extend_from_slice(&2048u64.to_le_bytes());
        hd.extend_from_slice(&204800u64.to_le_bytes());
        hd.extend_from_slice(&part);
        hd.extend_from_slice(&[0x02, 0x02]);
        dp.extend(hd);
        let p: Vec<u8> = path.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()).collect();
        dp.extend_from_slice(&[0x04, 0x04]);
        dp.extend_from_slice(&((4 + p.len()) as u16).to_le_bytes());
        dp.extend(p);
        dp.extend_from_slice(&[0x7f, 0xff, 4, 0]);
        let mut d = 1u32.to_le_bytes().to_vec();
        d.extend_from_slice(&(dp.len() as u16).to_le_bytes());
        d.extend(desc.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()));
        d.extend(dp);
        d
    }

    #[test]
    fn parses_a_windows_entry() {
        let g = crate::gpt::guid_bytes("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        let lo = parse_load_option(&load_option("Windows Boot Manager", g, "\\EFI\\Microsoft\\Boot\\bootmgfw.efi")).unwrap();
        assert_eq!(lo.description, "Windows Boot Manager");
        assert_eq!(lo.partition, Some(g));
        assert_eq!(lo.path.as_deref(), Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi"));
    }

    #[test]
    fn garbage_is_not_an_entry() {
        assert_eq!(parse_load_option(&[1, 0, 0, 0, 200, 0, 65, 0, 0, 0]), None);
        assert_eq!(parse_load_option(&[1, 2]), None);
    }
}
