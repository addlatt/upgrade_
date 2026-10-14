//! What both steps share: the log that is also the screen, the tools they
//! call (every call is the script's own, by name, so a recording of the
//! calls stays comparable), times, hashes, the job's fields as the
//! scripts' `jq_` read them, and `textwrap.wrap` as Python wraps.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `exec > >(tee -a "$LOG") 2>&1`: every line goes to the screen and the log.
pub struct Log {
    pub path: String,
}

impl Log {
    pub fn say(&self, line: &str) {
        println!("{line}");
        let _ = std::io::stdout().flush();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Several lines at once (a tool's output, as `tee` showed it).
    pub fn say_text(&self, text: &str) {
        for l in text.lines() {
            self.say(l);
        }
    }
}

/// A tool's exit code and what it printed, both streams.
pub struct Ran {
    pub status: i32,
    pub out: String,
    pub err: String,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
    /// `2>&1`
    pub fn merged(&self) -> String {
        format!("{}{}", self.out, self.err)
    }
}

/// Run a tool by name, as the shell did (PATH decides which one).
pub fn run(program: &str, args: &[&str]) -> Ran {
    match Command::new(program).args(args).stdin(Stdio::null()).output() {
        Ok(o) => Ran { status: o.status.code().unwrap_or(-1), out: String::from_utf8_lossy(&o.stdout).into_owned(), err: String::from_utf8_lossy(&o.stderr).into_owned() },
        Err(_) => Ran { status: 127, out: String::new(), err: format!("{program}: command not found\n") },
    }
}

/// `command -v name`.
pub fn have(program: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file())).unwrap_or(false)
}

pub fn sync() {
    run("sync", &[]);
}

pub fn sleep(seconds: &str) {
    run("sleep", &[seconds]);
}

/// `systemctl reboot 2>/dev/null || reboot -f 2>/dev/null || { echo b > /proc/sysrq-trigger; }`
pub fn reboot() {
    if run("systemctl", &["reboot"]).ok() {
        return;
    }
    if run("reboot", &["-f"]).ok() {
        return;
    }
    let _ = std::fs::write("/proc/sysrq-trigger", "b\n");
}

/// `date -u +%FT%TZ`.
pub fn utc_now() -> String {
    utc_of(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
}

pub fn epoch_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn utc_of(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

pub fn sha256_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

pub fn read_to_string(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// `jq_`: a dotted path into the job, printed as the scripts printed it:
/// a string as itself, a number as itself, a boolean as Python prints it,
/// an object or a list as JSON, nothing as the empty text.
pub fn jq(job: &Value, path: &str) -> String {
    let mut v = job;
    for k in path.split('.') {
        v = match v {
            Value::Array(l) => match k.parse::<usize>().ok().and_then(|i| l.get(i)) {
                Some(x) => x,
                None => return String::new(),
            },
            Value::Object(o) => match o.get(k) {
                Some(x) => x,
                None => return String::new(),
            },
            _ => return String::new(),
        };
        if v.is_null() {
            return String::new();
        }
    }
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn jq_len(job: &Value, path: &str) -> usize {
    let mut v = job;
    for k in path.split('.') {
        v = match v {
            Value::Array(l) => match k.parse::<usize>().ok().and_then(|i| l.get(i)) {
                Some(x) => x,
                None => return 0,
            },
            Value::Object(o) => match o.get(k) {
                Some(x) => x,
                None => return 0,
            },
            _ => return 0,
        };
    }
    v.as_array().map_or(0, Vec::len)
}

/// `textwrap.wrap(text, width)` as Python 3 wraps: words split at
/// whitespace, a hyphenated word may break after its hyphen, a line never
/// starts or ends with whitespace, a word longer than the width is cut.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_space = false;
    for c in text.chars() {
        let space = c.is_whitespace();
        if space != prev_space && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.push(if space { ' ' } else { c });
        prev_space = space;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    // a hyphen between letters splits the word after the hyphen
    let mut split: Vec<String> = Vec::new();
    for ch in chunks {
        if ch.starts_with(' ') {
            split.push(ch);
            continue;
        }
        let chars: Vec<char> = ch.chars().collect();
        let mut start = 0;
        for i in 1..chars.len().saturating_sub(1) {
            if chars[i] == '-' && chars[i - 1].is_alphanumeric() && chars[i + 1].is_alphanumeric() {
                split.push(chars[start..=i].iter().collect());
                start = i + 1;
            }
        }
        split.push(chars[start..].iter().collect());
    }
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;
    while i < split.len() {
        let mut line: Vec<String> = Vec::new();
        let mut len = 0;
        if split[i].starts_with(' ') {
            i += 1;
            continue;
        }
        while i < split.len() {
            let l = split[i].chars().count();
            if len + l <= width {
                line.push(split[i].clone());
                len += l;
                i += 1;
            } else {
                break;
            }
        }
        if i < split.len() && line.is_empty() {
            // a word longer than the line: cut it
            let word: Vec<char> = split[i].chars().collect();
            line.push(word[..width].iter().collect());
            split[i] = word[width..].iter().collect();
        }
        if line.last().is_some_and(|c| c.starts_with(' ')) {
            line.pop();
        }
        if !line.is_empty() {
            lines.push(line.concat());
        }
    }
    lines
}

/// `ls <dir>/*` as the shell expands it: the entries in C order.
pub fn sorted_entries(dir: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// `sort -V`: version order, one key against another.
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> Vec<(bool, String)> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut digit = false;
        for c in s.chars() {
            if !cur.is_empty() && c.is_ascii_digit() != digit {
                out.push((digit, std::mem::take(&mut cur)));
            }
            digit = c.is_ascii_digit();
            cur.push(c);
        }
        if !cur.is_empty() {
            out.push((digit, cur));
        }
        out
    };
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(&pb) {
        let o = match (x.0, y.0) {
            (true, true) => x.1.parse::<u64>().unwrap_or(0).cmp(&y.1.parse::<u64>().unwrap_or(0)),
            _ => x.1.cmp(&y.1),
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    pa.len().cmp(&pb.len())
}

/// The seconds a test asks for, else the product's.
pub fn test_secs(var: &str, default: u64) -> f64 {
    std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_like_python() {
        assert_eq!(wrap("The installer stopped before touching anything, because this USB stick has already converted this computer, and it never installs twice. Take the stick out and restart.", 60),
                   vec!["The installer stopped before touching anything, because this", "USB stick has already converted this computer, and it never", "installs twice. Take the stick out and restart."]);
        assert_eq!(wrap("", 60), Vec::<String>::new());
        assert_eq!(wrap("a last-chance word", 8), vec!["a last-", "chance", "word"]);
    }

    #[test]
    fn utc_prints_as_date_does() {
        assert_eq!(utc_of(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_of(1792008000), "2026-10-14T20:00:00Z");
    }

    #[test]
    fn version_sort() {
        let mut v = vec!["6.14.0-63.fc44.x86_64", "6.13.9-200.fc42.x86_64", "6.9.1"];
        v.sort_by(|a, b| version_cmp(a, b));
        assert_eq!(v.last().unwrap(), &"6.14.0-63.fc44.x86_64");
    }
}
