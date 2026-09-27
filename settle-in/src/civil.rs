//! Calendar arithmetic: date and time fields <-> seconds since 1970, with no
//! zone. (Howard Hinnant's days-from-civil algorithm; public domain.)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Civil {
    /// These fields read as if they were UTC.
    pub fn as_unix(&self) -> i64 {
        let y = if self.month <= 2 { self.year - 1 } else { self.year };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146097 + doe - 719468;
        days * 86400 + self.hour as i64 * 3600 + self.minute as i64 * 60 + self.second as i64
    }

    pub fn from_unix(t: i64) -> Civil {
        let days = t.div_euclid(86400);
        let secs = t.rem_euclid(86400);
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
        Civil {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: (secs % 3600 / 60) as u32,
            second: (secs % 60) as u32,
        }
    }

    pub fn valid(&self) -> bool {
        (1..=12).contains(&self.month) && (1..=31).contains(&self.day) && self.hour < 24 && self.minute < 60 && self.second < 61
            && Civil::from_unix(self.as_unix()) == *self
    }

    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", self.year, self.month, self.day, self.hour, self.minute, self.second)
    }
}

pub fn iso_utc(t: i64) -> String {
    format!("{}Z", Civil::from_unix(t).iso())
}

/// "2026-09-27T12:00:00Z" -> seconds since 1970, or None.
pub fn parse_iso_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let n = |a: usize, z: usize| s[a..z].parse::<i64>().ok();
    let c = Civil {
        year: n(0, 4)?,
        month: n(5, 7)? as u32,
        day: n(8, 10)? as u32,
        hour: n(11, 13)? as u32,
        minute: n(14, 16)? as u32,
        second: n(17, 19)? as u32,
    };
    if !c.valid() {
        return None;
    }
    Some(c.as_unix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for t in [0i64, 951782400, 1790511411, 4102444800, -86400] {
            assert_eq!(Civil::from_unix(t).as_unix(), t);
        }
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951782400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn parses_iso() {
        assert_eq!(parse_iso_utc("2000-02-29T00:00:00Z"), Some(951782400));
        assert_eq!(parse_iso_utc("2001-02-29T00:00:00Z"), None);
        assert_eq!(parse_iso_utc("2026-09-27 12:00:00"), None);
    }
}
