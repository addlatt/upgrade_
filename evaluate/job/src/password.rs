//! The new account's password, ported from `evaluate/windows/Read-Password.ps1`
//! 0.2.0: asked twice at a console, never shown and never written; only its
//! SHA-512 crypt hash (`$6$<salt>$<hash>`, what Fedora's `/etc/shadow` and
//! kickstart's `user --iscrypted` take) is written to the file the launchers
//! name, and the job writer puts that in `job.json`.
//!
//! The hash is written out from its specification (Ulrich Drepper, "Unix
//! crypt using SHA-256 and SHA-512", steps 1 to 22), and held to that
//! document's test vectors and to what the PowerShell answers
//! (`tests/password.rs`).

use sha2::{Digest, Sha512};

/// The PowerShell hasher this port follows.
pub const FOLLOWS_HASHER: &str = "0.2.0";

pub const CRYPT_ALPHABET: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// The first `len` bytes of `block` repeated.
fn repeated(block: &[u8], len: usize) -> Vec<u8> {
    (0..len).map(|i| block[i % block.len()]).collect()
}

/// ConvertTo-Sha512Crypt: SHA-512 crypt of `password` with `salt` (at most
/// 16 characters are used) and `rounds` (5000 when not given; below 1000 is
/// raised to 1000; written into the result only when given).
pub fn sha512_crypt(password: &str, salt: &str, rounds: Option<u32>) -> String {
    sha512_crypt_with(password, salt, rounds.unwrap_or(5000), rounds.is_some())
}

/// The script's two parameters as they are: `-Rounds` and `-RoundsGiven`
/// separately (the rounds are used either way; the prefix names them only
/// when given). The product always calls with 5000, not given.
pub fn sha512_crypt_with(password: &str, salt: &str, rounds: u32, rounds_given: bool) -> String {
    let p = password.as_bytes();
    let salt: String = salt.chars().take(16).collect();
    let s = salt.as_bytes();
    let given = rounds_given;
    let rounds = rounds.max(1000);
    // steps 4-8: B = H(P S P)
    let b = sha512(&[p, s, p]);
    // steps 1-3, 9-12: A = H(P S B-repeated-to-|P| then, per bit of |P|, B or P)
    let mut ma: Vec<u8> = Vec::new();
    ma.extend_from_slice(p);
    ma.extend_from_slice(s);
    let mut n = p.len();
    while n > 64 {
        ma.extend_from_slice(&b);
        n -= 64;
    }
    ma.extend_from_slice(&b[..n]);
    let mut n = p.len();
    while n > 0 {
        if n & 1 == 1 {
            ma.extend_from_slice(&b);
        } else {
            ma.extend_from_slice(p);
        }
        n >>= 1;
    }
    let a = sha512(&[&ma]);
    // steps 13-16: P-sequence from H(P repeated |P| times)
    let dp: Vec<u8> = p.repeat(p.len());
    let p_seq = repeated(&sha512(&[&dp]), p.len());
    // steps 17-20: S-sequence from H(S repeated 16 + A[0] times)
    let ds: Vec<u8> = s.repeat(16 + a[0] as usize);
    let s_seq = repeated(&sha512(&[&ds]), s.len());
    // step 21: the rounds
    let mut c = a;
    for i in 0..rounds {
        let mut mc: Vec<u8> = Vec::new();
        if i & 1 == 1 {
            mc.extend_from_slice(&p_seq);
        } else {
            mc.extend_from_slice(&c);
        }
        if i % 3 != 0 {
            mc.extend_from_slice(&s_seq);
        }
        if i % 7 != 0 {
            mc.extend_from_slice(&p_seq);
        }
        if i & 1 == 1 {
            mc.extend_from_slice(&c);
        } else {
            mc.extend_from_slice(&p_seq);
        }
        c = sha512(&[&mc]);
    }
    // step 22: the SHA-512 byte order into crypt's base-64
    const ORDER: [[usize; 3]; 21] = [
        [0, 21, 42], [22, 43, 1], [44, 2, 23], [3, 24, 45], [25, 46, 4], [47, 5, 26], [6, 27, 48], [28, 49, 7], [50, 8, 29], [9, 30, 51], [31, 52, 10],
        [53, 11, 32], [12, 33, 54], [34, 55, 13], [56, 14, 35], [15, 36, 57], [37, 58, 16], [59, 17, 38], [18, 39, 60], [40, 61, 19], [62, 20, 41],
    ];
    let mut out = String::with_capacity(86);
    for t in ORDER {
        let mut w = ((c[t[0]] as u32) << 16) | ((c[t[1]] as u32) << 8) | c[t[2]] as u32;
        for _ in 0..4 {
            out.push(CRYPT_ALPHABET[(w & 63) as usize] as char);
            w >>= 6;
        }
    }
    let mut w = c[63] as u32;
    for _ in 0..2 {
        out.push(CRYPT_ALPHABET[(w & 63) as usize] as char);
        w >>= 6;
    }
    let prefix = if given { format!("$6$rounds={rounds}$") } else { "$6$".to_string() };
    format!("{prefix}{salt}${out}")
}

/// New-CryptSalt: 16 characters from crypt's alphabet, from the OS random
/// source. An error when the OS gives none: a salt is never made up.
pub fn new_salt() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    os_random(&mut bytes)?;
    Ok(bytes.iter().map(|b| CRYPT_ALPHABET[(*b % 64) as usize] as char).collect())
}

#[cfg(windows)]
fn os_random(buf: &mut [u8]) -> Result<(), String> {
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok().map_err(|e| format!("the OS random source failed: {e}"))
}

#[cfg(not(windows))]
fn os_random(buf: &mut [u8]) -> Result<(), String> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).map_err(|e| format!("the OS random source failed: {e}"))
}

/// Test-PasswordPair: the refusal for two typed entries, or nothing.
pub fn pair_refusal(first: &str, second: &str) -> Option<&'static str> {
    if first.is_empty() {
        return Some("the password is empty");
    }
    if first != second {
        return Some("the two entries are not the same");
    }
    if first.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f) {
        return Some("the password contains a control character");
    }
    None
}

/// Best effort: the bytes of a password are overwritten before they are
/// freed, as the script nulls its copies.
pub fn wipe(s: &mut String) {
    // all zero bytes are valid UTF-8, so the string stays well formed
    unsafe { s.as_bytes_mut() }.fill(0);
    s.clear();
}

/// One line typed at the console with echo off, without its line end.
/// Windows only: the console mode is the Windows one.
#[cfg(windows)]
pub fn read_hidden_line(prompt: &str) -> Result<String, String> {
    use std::io::{BufRead, Write};
    use windows::Win32::System::Console::{GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE, ENABLE_ECHO_INPUT, STD_INPUT_HANDLE};
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) }.map_err(|e| format!("no console input: {e}"))?;
    let mut mode = CONSOLE_MODE(0);
    let had_mode = unsafe { GetConsoleMode(handle, &mut mode) }.is_ok();
    if had_mode {
        unsafe { SetConsoleMode(handle, mode & !ENABLE_ECHO_INPUT) }.map_err(|e| format!("cannot hide the typing: {e}"))?;
    }
    let mut line = String::new();
    let read = std::io::stdin().lock().read_line(&mut line);
    if had_mode {
        unsafe { SetConsoleMode(handle, mode) }.ok();
    }
    println!();
    read.map_err(|e| format!("reading the password: {e}"))?;
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(line)
}

#[cfg(not(windows))]
pub fn read_hidden_line(_prompt: &str) -> Result<String, String> {
    Err("the password prompt runs on Windows".to_string())
}

/// The whole prompt as the script runs it: the words, three tries, the hash
/// written to `out_file` (one line, UTF-8, no byte order mark). Exit 0 when
/// set, 1 when not.
pub fn ask_and_write(out_file: &std::path::Path, linux_name: &str) -> i32 {
    println!();
    println!("  Your Fedora account:  {linux_name}");
    println!("  You sign in to Fedora as {linux_name} with the password you choose now.");
    println!("  Nothing shows while you type. Write it down if you need to - nothing else stores it.");
    for _ in 1..=3 {
        let (mut a, mut b) = match (read_hidden_line("  Password: "), read_hidden_line("  Type it again: ")) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("  {e}");
                break;
            }
        };
        let why = pair_refusal(&a, &b);
        if why.is_none() {
            let salt = match new_salt() {
                Ok(s) => s,
                Err(e) => {
                    wipe(&mut a);
                    wipe(&mut b);
                    eprintln!("  {e}");
                    break;
                }
            };
            let hash = sha512_crypt(&a, &salt, None);
            wipe(&mut a);
            wipe(&mut b);
            if let Err(e) = std::fs::write(out_file, format!("{hash}\n")) {
                eprintln!("  cannot write {}: {e}", out_file.display());
                break;
            }
            println!("  Password set for {linux_name}.");
            return 0;
        }
        wipe(&mut a);
        wipe(&mut b);
        println!("  Not set: {}. Try again.", why.unwrap_or(""));
    }
    println!("  No password was set. Nothing was changed.");
    1
}
