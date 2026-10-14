//! The harness's live half, Windows only: the seam on a real machine,
//! following Test-StorageMode.ps1's live helpers. The scanner runs in this
//! process (the script started `upgrade-scan.ps1` twice per leg); the
//! resume task, the state directory's protection, the popups and the
//! registry are the prologue's own; every tool call is kept by the
//! recorder (`tools.jsonl` in the state directory).

use super::flow::{LegFiles, Live};
use super::{STICK_WAIT_SECONDS, TASK_NAME};
use crate::live as pl;
use crate::tools::Recorder;
use crate::val::{int, s};
use serde_json::{json, Value};
use std::path::Path;
use upgrade_scan::collect::registry::{self, Hive};
use upgrade_scan::collect::storage::{bus_type, storage_wmi};
use upgrade_scan::collect::wmi::{Wmi, CIMV2};
use upgrade_scan::{collect, product};

const SAFEBOOT_OPTION: &str = r"SYSTEM\CurrentControlSet\Control\SafeBoot\Option";
const RUNONCE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce";

pub struct WinLive {
    pub rec: Recorder,
}

impl WinLive {
    pub fn new(state_dir: &str) -> WinLive {
        WinLive { rec: Recorder::new(Some(Path::new(state_dir).join("tools.jsonl"))) }
    }
}

/// The default state directory, `%ProgramData%\upgrade_\storage-mode`.
pub fn default_state_dir() -> String {
    format!("{}\\storage-mode", program_data())
}

fn program_data() -> String {
    format!("{}\\upgrade_", std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into()))
}

/// The SYSTEM startup task's XML for the resume.
pub fn task_xml(exe: &str, state: &str) -> String {
    pl::task_xml(exe, "upgrade_: continue the storage-mode test after a restart (removed by the test itself)", &format!("storage-mode resume --state-dir \"{state}\""), "PT1H")
}

impl Live for WinLive {
    fn elevated(&self) -> bool {
        pl::is_elevated()
    }
    fn now_o(&self) -> String {
        pl::now_o()
    }
    fn safe_mode(&self) -> Option<i64> {
        registry::dword(&Hive::LocalMachine, SAFEBOOT_OPTION, "OptionValue")
    }
    fn context(&self) -> Value {
        let c = pl::live_resume_context();
        json!({"Utc": pl::now_o(), "RunAs": c["RunAs"], "Interactive": c["Interactive"], "SessionId": c["SessionId"], "ExplorerRunning": c["ExplorerRunning"], "Unattended": c["Unattended"], "UptimeSeconds": pl::uptime_seconds()})
    }
    fn facts(&mut self, root: &str) -> Result<Value, String> {
        let cimv2 = Wmi::connect(CIMV2)?;
        let one = |class: &str, props: &[&str]| cimv2.query(class, props).ok().and_then(|l| l.into_iter().next()).unwrap_or(Value::Null);
        let cs = one("Win32_ComputerSystem", &["Manufacturer", "Model"]);
        let os = one("Win32_OperatingSystem", &["Caption", "BuildNumber"]);
        let bios = one("Win32_BIOS", &["SMBIOSBIOSVersion"]);
        let bl = pl::bitlocker_state(&mut self.rec);
        let bitlocker = match s(&bl["State"]).as_str() {
            "on" => "on",
            "off" => "off",
            _ => "unknown",
        };
        let letter = &root[..1];
        let w = storage_wmi()?;
        let sv = w.query_where("MSFT_Volume", &["UniqueId"], &format!("DriveLetter='{letter}'"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Volume objects found with property 'DriveLetter' equal to '{letter}'. Verify the value of the property and retry."))?;
        let bus = (|| -> Option<String> {
            let sp = w.query_where("MSFT_Partition", &["DiskNumber"], &format!("DriveLetter='{letter}'")).ok()?.into_iter().next()?;
            let sd = w.query_where("MSFT_Disk", &["BusType"], &format!("Number={}", int(&sp["DiskNumber"]))).ok()?.into_iter().next()?;
            Some(bus_type(&sd["BusType"]))
        })();
        Ok(json!({
            "Vendor": s(&cs["Manufacturer"]), "Model": s(&cs["Model"]), "BiosVersion": s(&bios["SMBIOSBIOSVersion"]),
            "OsCaption": s(&os["Caption"]), "OsBuild": int(&os["BuildNumber"]),
            "SecureBoot": pl::secure_boot(), "BitLocker": bitlocker, "StickVolumeId": s(&sv["UniqueId"]), "StickBus": bus,
        }))
    }
    fn find_stick(&mut self, st: &Value) -> Option<String> {
        crate::judge::find_stick_root(&pl::volumes(), &s(&st["StickVolumeId"]))
    }
    fn wait_stick(&mut self, st: &Value) -> Option<String> {
        let t0 = pl::now_local().seconds();
        loop {
            if let Some(r) = self.find_stick(st) {
                return Some(r);
            }
            if pl::now_local().seconds() - t0 >= STICK_WAIT_SECONDS {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }
    fn scan_leg(&mut self, dir: &str) -> LegFiles {
        let d = Path::new(dir);
        let _ = std::fs::create_dir_all(d);
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                if e.path().is_file() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        // the kit's boot files sit at the stick's root (the scanner reads their .sbat)
        let kit_root = dir.get(..3).map(Path::new);
        let mut report = None;
        let mut scan_log = String::new();
        match product::scan_this_machine(kit_root) {
            Ok(r) => {
                scan_log = r.lines.join("\r\n") + "\r\n";
                match product::write_report(&r, d, true) {
                    Ok((_, Some(js))) => report = Some((js.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), product::report_json(&r))),
                    Ok((_, None)) => {}
                    Err(e) => scan_log.push_str(&format!("{e}\r\n")),
                }
            }
            Err(e) => scan_log.push_str(&format!("{e}\r\n")),
        }
        let _ = std::fs::write(d.join("scan.log"), scan_log);
        let c = collect::collect_hardware();
        let now = collect::now().0;
        let capture = product::machine_capture(&c, &format!("{:04}-{:02}-{:02}", now.year, now.month, now.day));
        let cp = d.join("machine-capture.json");
        let capture = match std::fs::write(&cp, serde_json::to_string_pretty(&capture).unwrap_or_default() + "\n") {
            Ok(()) => Some(capture),
            Err(_) => None,
        };
        LegFiles { report, capture }
    }
    fn native(&mut self, program: &str, args: &[String]) -> (i64, String) {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let r = self.rec.run(program, &a);
        (r.exit.map_or(-1, i64::from), r.lines().join(" ").trim().to_string())
    }
    fn popup(&mut self, title: &str, text: &str, seconds: i64, buttons: i64) -> i64 {
        pl::show_popup(text, title, seconds, buttons)
    }
    fn reg_set(&mut self, name: &str, value: &str) -> Result<(), String> {
        registry::set_string(&Hive::LocalMachine, RUNONCE, name, value)
    }
    fn reg_remove(&mut self, name: &str) {
        let _ = registry::delete_value(&Hive::LocalMachine, RUNONCE, name);
    }
    fn reg_present(&self, name: &str) -> bool {
        registry::string(&Hive::LocalMachine, RUNONCE, name).is_some()
    }
    fn protect_state_dir(&mut self, state: &str) -> Result<(), String> {
        pl::protect_state_dir(&mut self.rec, state).map(|_| ())
    }
    fn copy_program(&mut self, state: &str) -> Result<(), String> {
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        pl::place_program(&me, Path::new(&super::program_in(state)))
    }
    fn register_task(&mut self, state: &str) -> Result<(), String> {
        let xml = task_xml(&super::program_in(state), state);
        pl::register_task(&mut self.rec, state, TASK_NAME, "resume-task.xml", &xml)
    }
    fn unregister_task(&mut self) -> bool {
        pl::unregister_task_named(&mut self.rec, TASK_NAME)
    }
    fn log(&mut self, line: &str) {
        println!("{line}");
    }
    fn host(&mut self, line: &str) {
        println!("{line}");
    }
    fn marker_path(&self) -> String {
        format!("{}\\storage-mode-safeboot.txt", program_data())
    }
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
    fn mkdir(&mut self, path: &str) {
        let _ = std::fs::create_dir_all(path);
    }
    fn remove(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
    }
    fn move_file(&mut self, from: &str, to: &str) {
        let _ = std::fs::remove_file(to);
        let _ = std::fs::rename(from, to);
    }
    fn append_line(&mut self, path: &str, line: &str) {
        use std::io::Write;
        if let Some(d) = Path::new(path).parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = write!(f, "{line}\r\n");
        }
    }
    fn save_json(&mut self, path: &str, doc: &Value) {
        if let Some(d) = Path::new(path).parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(path, serde_json::to_string_pretty(doc).unwrap_or_default());
    }
    fn read_json(&self, path: &str) -> Option<Value> {
        serde_json::from_str(std::fs::read_to_string(path).ok()?.trim_start_matches('\u{feff}')).ok()
    }
    fn read_lines(&self, path: &str) -> Vec<String> {
        std::fs::read_to_string(path).unwrap_or_default().trim_start_matches('\u{feff}').lines().map(|l| l.trim_end_matches('\r').to_string()).collect()
    }
}
