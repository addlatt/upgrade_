//! Bytes from the OS random source, for the staging write-speed probe.

#[cfg(windows)]
pub fn fill(buf: &mut [u8]) -> Result<(), String> {
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok().map_err(|e| format!("the OS random source failed: {e}"))
}

#[cfg(not(windows))]
pub fn fill(buf: &mut [u8]) -> Result<(), String> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).map_err(|e| e.to_string())
}
