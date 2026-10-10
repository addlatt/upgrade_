//! The backup arithmetic: how much room a copy of Windows plus the person's
//! data would need on an external drive, and whether one present has it.

const GB: f64 = 1073741824.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    pub drive_letter: String,
    pub free_bytes: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Capacity {
    pub windows_used_bytes: i64,
    pub user_data_bytes: i64,
    pub browser_data_bytes: i64,
    pub backup_needed_bytes: i64,
    pub backup_needed_gb: f64,
    pub external_present: bool,
    pub external_sufficient: bool,
    pub best_external: Option<Drive>,
}

/// Get-HarvestCapacity: needed = 1.1 x what Windows uses + the user folders
/// + the browser data. The best drive is the one with the most free space
/// (of equals, the later one, as PowerShell's descending sort leaves them).
pub fn capacity(user_folder_bytes: &[i64], browser_bytes: &[i64], external: &[Drive], windows_used_bytes: i64) -> Capacity {
    let user: i64 = user_folder_bytes.iter().sum();
    let browser: i64 = browser_bytes.iter().sum();
    let needed = (windows_used_bytes as f64 * 1.1 + user as f64 + browser as f64).round_ties_even() as i64;
    let mut best: Option<&Drive> = None;
    for d in external {
        if best.is_none_or(|b| d.free_bytes >= b.free_bytes) {
            best = Some(d);
        }
    }
    Capacity {
        windows_used_bytes,
        user_data_bytes: user,
        browser_data_bytes: browser,
        backup_needed_bytes: needed,
        backup_needed_gb: (needed as f64 / GB * 10.0).round_ties_even() / 10.0,
        external_present: !external.is_empty(),
        external_sufficient: best.is_some_and(|b| b.free_bytes >= needed),
        best_external: best.cloned(),
    }
}
