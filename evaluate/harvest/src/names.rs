//! Two mappings from Windows' names to Linux's: the time zone and the
//! sign-in name.

/// A Windows time zone ID as an IANA one. Only the handful that matter in
/// practice; anything else is nothing, and the installer asks. Never a guess.
pub fn iana_time_zone(windows_id: &str) -> Option<&'static str> {
    const MAP: [(&str, &str); 14] = [
        ("Eastern Standard Time", "America/New_York"),
        ("Central Standard Time", "America/Chicago"),
        ("Mountain Standard Time", "America/Denver"),
        ("Pacific Standard Time", "America/Los_Angeles"),
        ("Alaskan Standard Time", "America/Anchorage"),
        ("Hawaiian Standard Time", "Pacific/Honolulu"),
        ("GMT Standard Time", "Europe/London"),
        ("W. Europe Standard Time", "Europe/Berlin"),
        ("Romance Standard Time", "Europe/Paris"),
        ("Central Europe Standard Time", "Europe/Budapest"),
        ("AUS Eastern Standard Time", "Australia/Sydney"),
        ("Tokyo Standard Time", "Asia/Tokyo"),
        ("India Standard Time", "Asia/Kolkata"),
        ("UTC", "UTC"),
    ];
    // a PowerShell table does not care about case
    MAP.iter().find(|(win, _)| win.to_lowercase() == windows_id.to_lowercase()).map(|(_, iana)| *iana)
}

/// A Windows account name as a Linux sign-in name: lower case, only
/// `a-z 0-9 _ -`, and `user` when nothing usable is left or it would start
/// with a digit.
pub fn linux_name(windows_name: &str) -> String {
    let name: String = windows_name.to_lowercase().chars().filter(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-')).collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) { "user".to_string() } else { name }
}
