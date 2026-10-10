//! The small mappings: time zone, keyboard layout, sign-in name.

use upgrade_scan::ps::capture;

/// A Windows time zone ID as an IANA one, or nothing. An unmapped zone is a
/// refusal in the job, never a guess.
pub fn iana_time_zone(windows_id: &str) -> Option<&'static str> {
    const MAP: [(&str, &str); 23] = [
        ("Eastern Standard Time", "America/New_York"),
        ("Central Standard Time", "America/Chicago"),
        ("Mountain Standard Time", "America/Denver"),
        ("Pacific Standard Time", "America/Los_Angeles"),
        ("Alaskan Standard Time", "America/Anchorage"),
        ("Hawaiian Standard Time", "Pacific/Honolulu"),
        ("US Mountain Standard Time", "America/Phoenix"),
        ("Atlantic Standard Time", "America/Halifax"),
        ("GMT Standard Time", "Europe/London"),
        ("W. Europe Standard Time", "Europe/Berlin"),
        ("Romance Standard Time", "Europe/Paris"),
        ("Central Europe Standard Time", "Europe/Budapest"),
        ("Central European Standard Time", "Europe/Warsaw"),
        ("E. Europe Standard Time", "Europe/Chisinau"),
        ("FLE Standard Time", "Europe/Kiev"),
        ("GTB Standard Time", "Europe/Athens"),
        ("AUS Eastern Standard Time", "Australia/Sydney"),
        ("Tokyo Standard Time", "Asia/Tokyo"),
        ("India Standard Time", "Asia/Kolkata"),
        ("China Standard Time", "Asia/Shanghai"),
        ("Singapore Standard Time", "Asia/Singapore"),
        ("New Zealand Standard Time", "Pacific/Auckland"),
        ("UTC", "UTC"),
    ];
    MAP.iter().find(|(win, _)| win.to_lowercase() == windows_id.to_lowercase()).map(|(_, iana)| *iana)
}

/// A Windows input method tip (`0409:00000409`) as a keyboard layout name
/// for the installer, or nothing.
pub fn keymap(input_method_tip: &str) -> Option<&'static str> {
    const MAP: [(&str, &str); 20] = [
        ("00000409", "us"), ("00000809", "gb"), ("00000407", "de"), ("0000040c", "fr"), ("0000080c", "be"),
        ("00000410", "it"), ("0000040a", "es"), ("00000c0a", "es"), ("00000416", "br"), ("00000816", "pt"),
        ("00000413", "nl"), ("0000041d", "se"), ("00000414", "no"), ("00000406", "dk"), ("0000040b", "fi"),
        ("00000807", "ch"), ("00000405", "cz"), ("00000415", "pl"), ("00001009", "ca"), ("00000c0c", "ca"),
    ];
    let klid = capture(":([0-9A-Fa-f]{8})$", "", input_method_tip)?.to_lowercase();
    MAP.iter().find(|(k, _)| *k == klid).map(|(_, layout)| *layout)
}

/// A Windows account name as a Linux sign-in name: lower case, only
/// `a-z 0-9 _ -`, `user` when nothing usable is left or it would start with
/// a digit, 32 characters at most.
pub fn linux_name(windows_name: &str) -> String {
    let name: String = windows_name.to_lowercase().chars().filter(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-')).collect();
    let name = if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) { "user".to_string() } else { name };
    name.chars().take(32).collect()
}
