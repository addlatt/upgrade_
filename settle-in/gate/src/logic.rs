//! The gate's decisions, with no Windows in them, so they are tested on
//! Linux (rule #5, the logic level). RISKS R33; architecture.md, "The way
//! back to Windows", stage 2.
//!
//! Everything that can refuse refuses here, before the countdown (rule #3):
//! a job that is not ours, a job that already ran, a drive that is not
//! there, a drive that is there twice, the stick named as a drive to erase.
//! The words on the screens are drafts until the owner approves them.

use serde_json::{json, Value};

pub const SCHEMA: &str = "go-back-job/1";
/// DRAFT, awaiting the owner's approval (the Linux program asks for the same words).
pub const SENTENCE: &str = "I confirm that Linux and everything on this computer will be deleted and nothing will be kept";
pub const COUNT_SECS: u64 = 120;

/// A drive as the gate sees it in WinPE.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub number: u32,
    pub serial: String,
    pub size: u64,
    pub model: String,
    /// World-wide names the drive reports (NAA, EUI-64), lower-case hex. Hyper-V's
    /// disks report no serial in WinPE, and NVMe drives report a namespace id where
    /// Linux shows the controller's serial (rig and G16, 2026-09-29).
    pub ids: Vec<String>,
}

/// A drive the job names, with the number WinPE gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub role: String,
    pub number: u32,
    pub model: String,
    pub size: u64,
    pub how: &'static str,
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase()
}

/// ATA serials come back from some Windows drivers with each pair of
/// characters swapped ("IE8A..." for "EI8A..."). Pure.
fn pair_swapped(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for c in b.chunks(2) {
        if c.len() == 2 {
            out.push(c[1]);
            out.push(c[0]);
        } else {
            out.push(c[0]);
        }
    }
    out
}

/// How a seen serial matches the job's, if it does.
pub fn serial_match(job: &str, seen: &str) -> Option<&'static str> {
    let (j, s) = (norm(job), norm(seen));
    if j.is_empty() || s.is_empty() {
        return None;
    }
    if j == s {
        Some("serial")
    } else if pair_swapped(&j) == s || pair_swapped(&s) == j {
        Some("serial (pairs swapped by the driver)")
    } else {
        None
    }
}

/// "0x6002248...", "naa.6002248...", "eui.0025..." -> lower-case hex. Pure.
/// Anything else (Linux's "t10.ATA <model> <serial>" fallback) is not a name: "".
pub fn wwn_hex(s: &str) -> String {
    let t = s.trim().to_ascii_lowercase();
    let body = t.strip_prefix("0x").or_else(|| t.strip_prefix("naa.")).or_else(|| t.strip_prefix("eui.")).unwrap_or(&t);
    if body.chars().all(|c| c.is_ascii_hexdigit()) { body.to_string() } else { String::new() }
}

/// Windows' grouped NVMe serial ("0000_0000_..._575B.") as hex, if it is one.
fn grouped_hex(s: &str) -> String {
    let t: String = s.trim().trim_end_matches('.').chars().filter(|c| *c != '_').collect();
    if t.len() >= 16 && t.chars().all(|c| c.is_ascii_hexdigit()) { t.to_ascii_lowercase() } else { String::new() }
}

/// How a seen drive matches a job's drive by who it is (never by size alone).
pub fn identity_match(serial: &str, wwn: &str, seen: &Seen) -> Option<&'static str> {
    if let Some(h) = serial_match(serial, &seen.serial) {
        return Some(h);
    }
    let w = wwn_hex(wwn);
    if w.len() >= 16 && seen.ids.iter().any(|i| *i == w) {
        return Some("world-wide name");
    }
    // NVMe: Windows gives the namespace's EUI as its "serial", in groups
    // ("0000_0000_0000_0001_00A0_7524_480C_575B."; the G16, 2026-09-29)
    if w.len() >= 16 && seen.serial.contains('_') && grouped_hex(&seen.serial) == w {
        return Some("world-wide name (as Windows' NVMe serial)");
    }
    None
}

/// A Windows account name the answer file can carry safely.
pub fn account_ok(name: &str) -> bool {
    let reserved = ["administrator", "guest", "defaultaccount", "wdagutilityaccount", "system", "none"];
    !name.is_empty()
        && name.len() <= 20
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        && !name.ends_with('.')
        && !reserved.contains(&name.to_ascii_lowercase().as_str())
}

/// Check the job itself. Pure. Err = the reason, in words for the screen.
pub fn check_job(job: &Value, previous: Option<&Value>) -> Result<(), String> {
    if job["schema"] != json!(SCHEMA) {
        return Err("the instructions on this stick are not ones this program understands".into());
    }
    if job["consent"]["sentence"] != json!(SENTENCE) {
        return Err("the instructions on this stick do not carry the sentence you typed".into());
    }
    if let Some(p) = previous {
        if p["job_id"] == job["job_id"] && p["result"] == json!("crossed") {
            return Err("this stick has already started erasing this computer once. It never starts twice".into());
        }
    }
    let drives = job["drives"].as_array().ok_or("the instructions name no drives")?;
    let roles: Vec<&str> = drives.iter().filter_map(|d| d["role"].as_str()).collect();
    if roles.first() != Some(&"system") || roles.len() != drives.len() || roles.iter().skip(1).any(|r| *r != "second") || drives.len() > 2 {
        return Err("the instructions must name the system drive first, and at most one more drive".into());
    }
    for d in drives {
        let no_serial = d["serial"].as_str().map(norm).unwrap_or_default().is_empty();
        let no_wwn = wwn_hex(d["wwn"].as_str().unwrap_or("")).len() < 16;
        if (no_serial && no_wwn) || d["size_bytes"].as_u64().unwrap_or(0) == 0 {
            return Err("a drive in the instructions has no serial number, world-wide name or size".into());
        }
    }
    if !account_ok(job["account"]["name"].as_str().unwrap_or("")) {
        return Err("the account name in the instructions cannot be used for Windows".into());
    }
    if job["windows"]["image_index"].as_u64().unwrap_or(0) == 0 {
        return Err("the instructions do not say which Windows to install".into());
    }
    Ok(())
}

/// Find every drive the job names among the drives WinPE sees. Pure.
/// Each must match exactly one drive by serial and exact size, no two on the
/// same drive, and never the stick itself.
pub fn find_drives(job: &Value, seen: &[Seen], stick: Option<u32>) -> Result<Vec<Found>, String> {
    let mut out: Vec<Found> = Vec::new();
    for d in job["drives"].as_array().into_iter().flatten() {
        let role = d["role"].as_str().unwrap_or("?").to_string();
        let serial = d["serial"].as_str().unwrap_or("");
        let wwn = d["wwn"].as_str().unwrap_or("");
        let size = d["size_bytes"].as_u64().unwrap_or(0);
        let model = d["model"].as_str().unwrap_or("").to_string();
        let what = if model.is_empty() { format!("the {} drive", role) } else { format!("the {} drive ({})", role, model) };
        let hits: Vec<(&Seen, &'static str)> = seen.iter().filter_map(|s| identity_match(serial, wwn, s).map(|h| (s, h))).collect();
        match hits.as_slice() {
            [] => return Err(format!("{} is not in this computer", what)),
            [(s, how)] => {
                if s.size != size {
                    return Err(format!("{} is here, but its size is not the one recorded ({} bytes, not {})", what, s.size, size));
                }
                if Some(s.number) == stick {
                    return Err(format!("{} is this USB stick", what));
                }
                if out.iter().any(|f| f.number == s.number) {
                    return Err(format!("{} is the same drive as another one in the instructions", what));
                }
                out.push(Found { role, number: s.number, model: s.model.clone(), size, how });
            }
            _ => return Err(format!("{} matches more than one drive here", what)),
        }
    }
    Ok(out)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The answer file for Windows Setup, naming the drives the gate found. Pure.
/// No product key (Setup reads a firmware key itself; `<Key></Key>` +
/// `Never` skips its key page, rig spike 2026-09-29), no password (the
/// account is empty and must be changed at first sign-in, SetupComplete.cmd).
pub fn unattend(job: &Value, found: &[Found]) -> String {
    let lang = esc(job["windows"]["language"].as_str().unwrap_or("en-US"));
    let index = job["windows"]["image_index"].as_u64().unwrap_or(1);
    let name = esc(job["account"]["name"].as_str().unwrap_or("user"));
    let sys = found.iter().find(|f| f.role == "system").map(|f| f.number).unwrap_or(0);
    let mut disks = format!(
        r#"        <Disk wcm:action="add">
          <DiskID>{sys}</DiskID><WillWipeDisk>true</WillWipeDisk>
          <CreatePartitions>
            <CreatePartition wcm:action="add"><Order>1</Order><Type>EFI</Type><Size>300</Size></CreatePartition>
            <CreatePartition wcm:action="add"><Order>2</Order><Type>MSR</Type><Size>16</Size></CreatePartition>
            <CreatePartition wcm:action="add"><Order>3</Order><Type>Primary</Type><Extend>true</Extend></CreatePartition>
          </CreatePartitions>
          <ModifyPartitions>
            <ModifyPartition wcm:action="add"><Order>1</Order><PartitionID>1</PartitionID><Format>FAT32</Format><Label>System</Label></ModifyPartition>
            <ModifyPartition wcm:action="add"><Order>2</Order><PartitionID>2</PartitionID></ModifyPartition>
            <ModifyPartition wcm:action="add"><Order>3</Order><PartitionID>3</PartitionID><Format>NTFS</Format><Label>Windows</Label><Letter>C</Letter></ModifyPartition>
          </ModifyPartitions>
        </Disk>
"#
    );
    for f in found.iter().filter(|f| f.role == "second") {
        disks.push_str(&format!(
            r#"        <Disk wcm:action="add">
          <DiskID>{}</DiskID><WillWipeDisk>true</WillWipeDisk>
          <CreatePartitions><CreatePartition wcm:action="add"><Order>1</Order><Type>Primary</Type><Extend>true</Extend></CreatePartition></CreatePartitions>
          <ModifyPartitions><ModifyPartition wcm:action="add"><Order>1</Order><PartitionID>1</PartitionID><Format>NTFS</Format><Label>Data</Label><Letter>D</Letter></ModifyPartition></ModifyPartitions>
        </Disk>
"#,
            f.number
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!-- written by upgrade-gate {ver} for job {job_id}: the drives it found by serial and size -->
<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
  <settings pass="windowsPE">
    <component name="Microsoft-Windows-International-Core-WinPE" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <SetupUILanguage><UILanguage>{lang}</UILanguage></SetupUILanguage>
      <InputLocale>{lang}</InputLocale><SystemLocale>{lang}</SystemLocale><UILanguage>{lang}</UILanguage><UserLocale>{lang}</UserLocale>
    </component>
    <component name="Microsoft-Windows-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <DiskConfiguration>
{disks}      </DiskConfiguration>
      <ImageInstall><OSImage>
        <InstallFrom><MetaData wcm:action="add"><Key>/IMAGE/INDEX</Key><Value>{index}</Value></MetaData></InstallFrom>
        <InstallTo><DiskID>{sys}</DiskID><PartitionID>3</PartitionID></InstallTo>
      </OSImage></ImageInstall>
      <UserData><AcceptEula>true</AcceptEula><ProductKey><Key></Key><WillShowUI>Never</WillShowUI></ProductKey></UserData>
    </component>
  </settings>
  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <InputLocale>{lang}</InputLocale><SystemLocale>{lang}</SystemLocale><UILanguage>{lang}</UILanguage><UserLocale>{lang}</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <OOBE>
        <HideEULAPage>true</HideEULAPage><HideOEMRegistrationScreen>true</HideOEMRegistrationScreen>
        <HideOnlineAccountScreens>true</HideOnlineAccountScreens><HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
        <ProtectYourPC>3</ProtectYourPC>
      </OOBE>
      <UserAccounts><LocalAccounts><LocalAccount wcm:action="add">
        <Name>{name}</Name><Group>Administrators</Group>
        <Password><Value></Value><PlainText>true</PlainText></Password>
      </LocalAccount></LocalAccounts></UserAccounts>
    </component>
  </settings>
</unattend>
"#,
        ver = crate::VERSION,
        job_id = esc(job["job_id"].as_str().unwrap_or("?")),
    )
}

/// The countdown screen. DRAFT words.
pub fn countdown_screen(left: u64, found: &[Found]) -> String {
    let names: Vec<String> = found.iter().map(|f| if f.model.is_empty() { f.role.clone() } else { f.model.clone() }).collect();
    let what = if found.len() > 1 { "BOTH DRIVES" } else { "THIS COMPUTER'S DRIVE" };
    format!(
        "\n\n   ERASING {} IN {}:{:02}\n\n   Linux and everything on this computer will be deleted, and Windows installed.\n   ({})\n\n   Press any key to CANCEL and restart into Linux.\n",
        what,
        left / 60,
        left % 60,
        names.join(" and ")
    )
}

/// The refusal screen. DRAFT words.
pub fn refusal_screen(why: &str) -> String {
    format!(
        "\n\n   NOTHING WAS ERASED.\n\n   Going back to Windows stopped before changing anything, because\n   {}.\n\n   This computer restarts into Linux in a minute (or press any key).\n   The reason is also written on this USB stick, in upgrade_\\go-back-gate.json.\n",
        why
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Value {
        json!({
            "schema": SCHEMA, "job_id": "j1",
            "consent": { "sentence": SENTENCE },
            "windows": { "version": "11", "image_index": 1, "language": "en-US", "edition_name": "Windows 11 Home" },
            "account": { "name": "rig" },
            "drives": [
                { "role": "system", "serial": "EI8AN00951150A71I", "size_bytes": 256060514304u64, "model": "HFS256G39TND-N210A" },
                { "role": "second", "serial": "WD-WXV1A88H0TFV", "size_bytes": 1000204886016u64, "model": "WDC WD10SPZX-21Z10T0" }
            ]
        })
    }
    fn seen() -> Vec<Seen> {
        vec![
            Seen { number: 0, serial: "WD-WXV1A88H0TFV".into(), size: 1000204886016, model: "WDC WD10SPZX-21Z10T0".into(), ids: vec![] },
            Seen { number: 1, serial: "EI8AN00951150A71I    ".into(), size: 256060514304, model: "HFS256G39TND-N210A".into(), ids: vec![] },
            Seen { number: 2, serial: "0123456789".into(), size: 15_664_676_864, model: "General UDisk".into(), ids: vec![] },
        ]
    }

    #[test]
    fn finds_both_drives_whatever_numbers_winpe_gave_them() {
        let f = find_drives(&job(), &seen(), Some(2)).unwrap();
        assert_eq!((f[0].role.as_str(), f[0].number), ("system", 1));
        assert_eq!((f[1].role.as_str(), f[1].number), ("second", 0));
    }

    #[test]
    fn a_pair_swapped_serial_still_matches_and_says_so() {
        let mut s = seen();
        s[1].serial = "IEA800N59115A0171I".into();
        assert_eq!(serial_match("EI8AN00951150A71I", "IE8A0N95101517AI"), None);
        assert_eq!(serial_match("EI8AN00951150A71I", &pair_swapped("EI8AN00951150A71I")), Some("serial (pairs swapped by the driver)"));
        s[1].serial = pair_swapped("EI8AN00951150A71I");
        assert_eq!(find_drives(&job(), &s, None).unwrap()[0].number, 1);
    }

    #[test]
    fn a_drive_with_no_serial_is_found_by_its_world_wide_name() {
        // Hyper-V in WinPE: no serial, the NAA id only (rig, 2026-09-29)
        let mut j = job();
        j["drives"] = json!([{ "role": "system", "serial": "", "wwn": "0x600224802F9DA875E3B23683792B2346", "size_bytes": 85899345920u64, "model": "Virtual Disk" }]);
        assert!(check_job(&j, None).is_ok());
        let s = vec![Seen { number: 0, serial: "".into(), size: 85899345920, model: "Msft Virtual Disk".into(), ids: vec!["600224802f9da875e3b23683792b2346".into()] }];
        let f = find_drives(&j, &s, Some(1)).unwrap();
        assert_eq!((f[0].number, f[0].how), (0, "world-wide name"));
        // an empty serial never matches an empty serial
        let s2 = vec![Seen { ids: vec![], ..s[0].clone() }];
        assert!(find_drives(&j, &s2, Some(1)).unwrap_err().contains("is not in this computer"));
        // and a drive with neither is refused before anything
        j["drives"][0]["wwn"] = json!("");
        assert!(check_job(&j, None).unwrap_err().contains("no serial number, world-wide name or size"));
        assert_eq!(wwn_hex("eui.0025388B91B0A1C2"), "0025388b91b0a1c2");
        assert_eq!(wwn_hex("t10.ATA     HFS256G39TND-N210A   EI8AN00951150A71I"), "");
        // the G16's NVMe drive: Linux's eui against Windows' grouped serial
        let nv = Seen { number: 0, serial: "0000_0000_0000_0001_00A0_7524_480C_575B.".into(), size: 1, model: "m".into(), ids: vec![] };
        assert_eq!(identity_match("", "eui.000000000000000100a07524480c575b", &nv), Some("world-wide name (as Windows' NVMe serial)"));
        assert_eq!(identity_match("", "eui.000000000000000100a07524480c575c", &nv), None);
    }

    #[test]
    fn a_missing_drive_refuses() {
        let s: Vec<Seen> = seen().into_iter().filter(|d| d.number != 0).collect();
        let e = find_drives(&job(), &s, None).unwrap_err();
        assert!(e.contains("second drive (WDC WD10SPZX-21Z10T0) is not in this computer"), "{}", e);
    }

    #[test]
    fn a_wrong_size_refuses() {
        let mut s = seen();
        s[1].size -= 512;
        assert!(find_drives(&job(), &s, None).unwrap_err().contains("its size is not the one recorded"));
    }

    #[test]
    fn the_stick_is_never_erased() {
        let mut j = job();
        j["drives"][1] = json!({ "role": "second", "serial": "0123456789", "size_bytes": 15_664_676_864u64, "model": "General UDisk" });
        assert!(find_drives(&j, &seen(), Some(2)).unwrap_err().contains("is this USB stick"));
    }

    #[test]
    fn one_serial_on_two_drives_refuses() {
        let mut s = seen();
        s[2].serial = "EI8AN00951150A71I".into();
        assert!(find_drives(&job(), &s, None).unwrap_err().contains("more than one drive"));
    }

    #[test]
    fn the_job_must_be_ours_carry_the_sentence_and_not_have_run() {
        assert!(check_job(&job(), None).is_ok());
        let mut j = job();
        j["consent"]["sentence"] = json!("yes");
        assert!(check_job(&j, None).is_err());
        let mut j = job();
        j["schema"] = json!("job/1");
        assert!(check_job(&j, None).is_err());
        let prev = json!({ "job_id": "j1", "result": "crossed" });
        assert!(check_job(&job(), Some(&prev)).unwrap_err().contains("never starts twice"));
        let prev = json!({ "job_id": "j1", "result": "cancelled" });
        assert!(check_job(&job(), Some(&prev)).is_ok());
    }

    #[test]
    fn the_job_shape_is_checked() {
        let mut j = job();
        j["drives"] = json!([j["drives"][1], j["drives"][0]]);
        assert!(check_job(&j, None).is_err());
        let mut j = job();
        j["account"]["name"] = json!("Administrator");
        assert!(check_job(&j, None).is_err());
        let mut j = job();
        j["account"]["name"] = json!("a<b");
        assert!(check_job(&j, None).is_err());
        let mut j = job();
        j["drives"][0]["serial"] = json!("  ");
        assert!(check_job(&j, None).is_err());
    }

    #[test]
    fn the_answer_file_names_the_found_drives_and_no_key_or_password() {
        let f = find_drives(&job(), &seen(), Some(2)).unwrap();
        let x = unattend(&job(), &f);
        assert!(x.contains("<DiskID>1</DiskID><WillWipeDisk>true</WillWipeDisk>"));
        assert!(x.contains("<DiskID>0</DiskID><WillWipeDisk>true</WillWipeDisk>"));
        assert!(x.contains("<InstallTo><DiskID>1</DiskID><PartitionID>3</PartitionID></InstallTo>"));
        assert!(!x.contains("<DiskID>2</DiskID>"));
        assert!(x.contains("<Key></Key><WillShowUI>Never</WillShowUI>"));
        assert!(x.contains("<Name>rig</Name>"));
        assert!(x.contains("<Password><Value></Value>"));
        assert!(x.contains("<Label>Data</Label>"));
    }

    #[test]
    fn one_drive_jobs_wipe_only_the_system_drive() {
        let mut j = job();
        j["drives"] = json!([j["drives"][0]]);
        let f = find_drives(&j, &seen(), None).unwrap();
        let x = unattend(&j, &f);
        assert_eq!(x.matches("<WillWipeDisk>true</WillWipeDisk>").count(), 1);
        assert!(countdown_screen(120, &f).contains("THIS COMPUTER'S DRIVE IN 2:00"));
    }

    #[test]
    fn the_screens_say_cancel_and_nothing_erased() {
        let f = find_drives(&job(), &seen(), None).unwrap();
        let c = countdown_screen(75, &f);
        assert!(c.contains("ERASING BOTH DRIVES IN 1:15") && c.contains("Press any key to CANCEL"));
        assert!(c.contains("HFS256G39TND-N210A and WDC WD10SPZX-21Z10T0"));
        assert!(refusal_screen("x").contains("NOTHING WAS ERASED"));
    }
}
