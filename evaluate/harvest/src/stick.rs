//! Do the person's folders fit on the stick (clean slate; RISKS R26)?

const GB: f64 = 1073741824.0;
pub const RESERVE_BYTES: i64 = 64 * 1048576;

/// One folder of the folder map, as far as the fit needs it.
#[derive(Debug, Clone, Default)]
pub struct Folder {
    pub exists: bool,
    pub bytes: i64,
    /// its cluster-exact size on the stick: files, directories, manifest lines
    pub stick_bytes: i64,
    pub files_over_4gib: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StickFit {
    pub file_system: String,
    pub cluster_bytes: i64,
    pub free_bytes: i64,
    pub files_bytes: i64,
    pub needed_bytes: i64,
    pub files_over_4gib: i64,
    pub fits: bool,
    pub gap_bytes: i64,
    /// why not, in plain words; nothing when it fits
    pub reason: Option<String>,
}

/// A number the way PowerShell's `{0:N2}` prints it: two decimals, halves
/// rounded away from zero, thousands separated by commas.
fn n2(x: f64) -> String {
    // .NET Framework rounds from the 15-digit decimal form of the number
    let whole_digits = if x.abs() < 1.0 { 1 } else { x.abs().log10().floor() as usize + 1 };
    let shown = format!("{:.*}", 15usize.saturating_sub(whole_digits).max(2), x.abs());
    let (int_part, frac) = shown.split_once('.').unwrap_or((&shown, ""));
    let mut cents: u128 = format!("{int_part}{}", &frac[..2]).parse().unwrap_or(0);
    if frac.as_bytes().get(2).is_some_and(|d| *d >= b'5') {
        cents += 1;
    }
    let (units, hundredths) = (cents / 100, cents % 100);
    let digits = units.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{}{grouped}.{hundredths:02}", if x < 0.0 && cents > 0 { "-" } else { "" })
}

/// Get-HarvestStickFit. Needed is the cluster-exact size or the prologue's
/// own "bytes x 1.02" rule, whichever is larger, plus the prologue's
/// reserve, so a "fits" here also passes the prologue's check at staging.
/// A FAT32 volume cannot hold a file larger than 4 GB: one such file is
/// "does not fit". A filesystem or cluster size this cannot size is not a
/// fit either: refuse, not guess.
pub fn stick_fit(folders: &[Folder], free_bytes: i64, file_system: &str, cluster_bytes: i64, reserve_bytes: i64) -> StickFit {
    let present = || folders.iter().filter(|f| f.exists);
    let files_bytes: i64 = present().map(|f| f.bytes).sum();
    let stick: i64 = present().map(|f| f.stick_bytes).sum();
    let over4: i64 = present().map(|f| f.files_over_4gib).sum();
    let needed = (stick as f64).max((files_bytes as f64 * 1.02).ceil()) as i64 + reserve_bytes;
    let gap = (needed - free_bytes).max(0);
    let is = |name: &str| file_system.to_lowercase() == name.to_lowercase();
    let reason = if !is("FAT32") && !is("exFAT") {
        Some(format!("the stick's volume is '{file_system}'; this version sizes only FAT32 and exFAT"))
    } else if cluster_bytes <= 0 {
        Some("the stick's cluster size could not be read".to_string())
    } else if is("FAT32") && over4 > 0 {
        Some(format!("{over4} file(s) are larger than 4 GB, and the stick's FAT32 volume cannot hold a file that large"))
    } else if gap > 0 {
        Some(format!("the folders need {} GB on the stick and it has {} GB free", n2(needed as f64 / GB), n2(free_bytes as f64 / GB)))
    } else {
        None
    };
    StickFit { file_system: file_system.to_string(), cluster_bytes, free_bytes, files_bytes, needed_bytes: needed, files_over_4gib: over4, fits: reason.is_none(), gap_bytes: gap, reason }
}

#[cfg(test)]
mod tests {
    use super::n2;

    #[test]
    fn n2_prints_as_powershell_does() {
        // answers recorded from Windows PowerShell 5.1 (tests/golden.json)
        assert_eq!(n2(1.125), "1.13");
        assert_eq!(n2(2040.0625), "2,040.06");
        assert_eq!(n2(0.0), "0.00");
        assert_eq!(n2(1500.004999), "1,500.00");
        assert_eq!(n2(1234567.891), "1,234,567.89");
        assert_eq!(n2(0.999), "1.00");
    }
}
