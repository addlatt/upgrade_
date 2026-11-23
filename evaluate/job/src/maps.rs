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

/// Names Linux keeps for itself on the Fedora and Ubuntu the kit installs:
/// its own accounts and groups. A person's account with one of these names
/// either cannot be made or takes over something it should not. The list is
/// from the distributions' documentation of their system accounts, not read
/// off the installed images; it errs on the side of refusing.
const KEPT_NAMES: &[&str] = &[
    "root", "daemon", "bin", "sys", "sync", "games", "man", "lp", "mail", "news", "uucp", "proxy", "www-data", "backup", "list", "irc",
    "gnats", "nobody", "nogroup", "adm", "admin", "shutdown", "halt", "operator", "ftp", "dbus", "messagebus", "polkitd", "sshd", "ssh",
    "gdm", "sddm", "lightdm", "tss", "chrony", "rtkit", "pipewire", "pulse", "avahi", "colord", "geoclue", "usbmux", "syslog", "uuidd",
    "tcpdump", "kernoops", "whoopsie", "cups", "saned", "hplip", "dnsmasq", "sssd", "abrt", "qemu", "unbound", "setroubleshoot",
    "wheel", "sudo", "users", "staff", "tty", "disk", "kmem", "mem", "dialout", "cdrom", "floppy", "tape", "audio", "video", "input",
    "kvm", "render", "plugdev", "netdev", "lpadmin", "sambashare", "utmp", "shadow", "lock", "utempter", "src", "fax", "voice", "sasl",
    "crontab", "systemd", "ubuntu", "fedora", "liveuser", "installer", "user-data",
];

/// Why a name the person chose cannot be their Linux sign-in name, in words
/// for them, or None when it can (2026-11-23). Stricter than the contract's
/// pattern: a first letter, and none of the names Linux keeps for itself.
pub fn linux_name_refusal(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("type a sign-in name");
    }
    if !name.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-')) {
        return Some("a sign-in name can only have small letters (a to z), digits, - and _, with no spaces");
    }
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Some("a sign-in name starts with a letter");
    }
    if name.len() > 32 {
        return Some("a sign-in name is 32 characters at most");
    }
    if KEPT_NAMES.contains(&name) || name.starts_with("systemd-") {
        return Some("Linux keeps that name for itself: choose another");
    }
    None
}

#[cfg(test)]
mod name_tests {
    #[test]
    fn a_chosen_sign_in_name_is_refused_in_plain_words_or_taken() {
        use super::linux_name_refusal as r;
        for ok in ["ann", "ann-smith", "a", "sam_2", "abcdefghijklmnopqrstuvwxyzabcdef"] {
            assert_eq!(r(ok), None, "{ok}");
        }
        assert!(r("").unwrap().contains("type a"));
        for bad in ["Ann", "ann smith", "ann.smith", "j\u{f6}rg", "ann\n"] {
            assert!(r(bad).unwrap().contains("small letters"), "{bad}");
        }
        for bad in ["2ann", "-ann", "_apt"] {
            assert!(r(bad).unwrap().contains("starts with a letter"), "{bad}");
        }
        assert!(r("abcdefghijklmnopqrstuvwxyzabcdefg").unwrap().contains("32"));
        for kept in ["root", "admin", "wheel", "sudo", "gdm", "systemd-network"] {
            assert!(r(kept).unwrap().contains("keeps that name"), "{kept}");
        }
    }
}
