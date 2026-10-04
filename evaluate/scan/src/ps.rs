//! Windows PowerShell's habits, kept on purpose. The scanner was written in
//! PowerShell 5.1 and its evidence was earned there, so where PowerShell
//! compares without regard to case, rounds half to even, or prints a date
//! its own way, the port does the same. Each helper says which habit it is.

use regress::Regex;
use serde::{Deserialize, Deserializer};
use std::fmt;

/// `-eq` on strings: case does not matter.
pub fn eq_ci(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// `-like 'prefix*'`.
pub fn starts_with_ci(text: &str, prefix: &str) -> bool {
    text.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// `-match`: a regular expression, searched anywhere, case not mattering.
/// `flags` adds to that: "m" makes `^` and `$` work per line.
pub fn re(pattern: &str, flags: &str) -> Regex {
    Regex::with_flags(pattern, format!("i{flags}").as_str()).unwrap_or_else(|e| panic!("regex {pattern}: {e}"))
}

pub fn matches(pattern: &str, text: &str) -> bool {
    re(pattern, "").find(text).is_some()
}

/// The first capture group of the first match.
pub fn capture<'t>(pattern: &str, flags: &str, text: &'t str) -> Option<&'t str> {
    let m = re(pattern, flags).find(text)?;
    m.group(1).map(|r| &text[r])
}

/// A string as an `if ($x)` sees it: there, and not empty.
pub fn truthy(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|t| !t.is_empty())
}

/// A value inside a "..." string: nothing when it is not there.
pub fn s(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("")
}

pub fn n(v: Option<i64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

/// `[math]::Round(x, 1)`: half goes to the even digit, not up.
pub fn round1(x: f64) -> f64 {
    (x * 10.0).round_ties_even() / 10.0
}

/// A number inside a "..." string: 120 for 120.0, 9.5 for 9.5.
pub fn num(x: f64) -> String {
    format!("{x}")
}

/// A local date and time, as the Windows event log gives it. Inside a
/// "..." string PowerShell prints it month first: 09/13/2026 15:16:48.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Stamp {
    /// From `2026-09-13T15:16:48`.
    pub fn parse(text: &str) -> Option<Stamp> {
        let b = text.as_bytes();
        if b.len() != 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
            return None;
        }
        let p = |from: usize, to: usize| text.get(from..to)?.parse::<u32>().ok();
        let st = Stamp { year: p(0, 4)? as i32, month: p(5, 7)?, day: p(8, 10)?, hour: p(11, 13)?, minute: p(14, 16)?, second: p(17, 19)? };
        let ok = (1..=12).contains(&st.month) && (1..=31).contains(&st.day) && st.hour < 24 && st.minute < 60 && st.second < 60;
        ok.then_some(st)
    }

    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", self.year, self.month, self.day, self.hour, self.minute, self.second)
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}/{:02}/{:04} {:02}:{:02}:{:02}", self.month, self.day, self.year, self.hour, self.minute, self.second)
    }
}

impl<'de> Deserialize<'de> for Stamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Stamp, D::Error> {
        let text = String::deserialize(d)?;
        Stamp::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("not a date and time: {text}")))
    }
}

/// .NET's `[version]`: two to four whole numbers with dots. `6.10` is newer
/// than `6.7`, which is the reason kernels are not compared as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version([i32; 4]);

impl Version {
    /// `ConvertTo-UpgVersion`: nothing for an empty or unreadable text,
    /// 99.0 for `rolling`.
    pub fn parse(text: &str) -> Option<Version> {
        if text.trim().is_empty() {
            return None;
        }
        if eq_ci(text, "rolling") {
            return Some(Version([99, 0, -1, -1]));
        }
        let parts: Vec<&str> = text.split('.').collect();
        if !(2..=4).contains(&parts.len()) {
            return None;
        }
        let mut v = [-1; 4];
        for (i, part) in parts.iter().enumerate() {
            let x: i32 = part.trim().parse().ok()?;
            if x < 0 {
                return None;
            }
            v[i] = x;
        }
        Some(Version(v))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().take_while(|x| **x >= 0).map(|x| x.to_string()).collect();
        write!(f, "{}", parts.join("."))
    }
}

/// A field that PowerShell hands over as one value or as a list.
pub fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Shape {
        One(String),
        Many(Vec<Option<String>>),
        Nothing(()),
    }
    Ok(match Shape::deserialize(d)? {
        Shape::One(x) => vec![x],
        Shape::Many(l) => l.into_iter().flatten().collect(),
        Shape::Nothing(()) => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(Version::parse("6.10").unwrap() > Version::parse("6.7").unwrap());
        assert!(Version::parse("6.12.1").unwrap() > Version::parse("6.12").unwrap());
        assert_eq!(Version::parse("rolling").unwrap().to_string(), "99.0");
        for bad in ["", " ", "6", "six.one", "6.-1", "1.2.3.4.5"] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn rounding_is_half_to_even() {
        assert_eq!(round1(17417.0 / 1024.0), 17.0);
        // asked of Windows PowerShell 5.1 on 2026-10-04:
        // 0.25 0.35 0.45 2.25 65.25 31.95 0.05 -> 0.2 0.4 0.4 2.2 65.2 32 0
        for (x, want) in [(0.25, 0.2), (0.35, 0.4), (0.45, 0.4), (2.25, 2.2), (65.25, 65.2), (31.95, 32.0), (0.05, 0.0), (1205.631836, 1205.6)] {
            assert_eq!(round1(x), want, "{x}");
        }
        assert_eq!(num(120.0), "120");
        assert_eq!(num(9.5), "9.5");
    }

    #[test]
    fn a_stamp_prints_the_way_powershell_does() {
        let st = Stamp::parse("2026-09-13T15:16:48").unwrap();
        assert_eq!(st.to_string(), "09/13/2026 15:16:48");
        assert!(st < Stamp::parse("2026-09-15T18:02:32").unwrap());
        assert!(Stamp::parse("2026-13-01T00:00:00").is_none());
    }
}
