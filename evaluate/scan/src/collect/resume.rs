//! Get-UpgResumeFacts: can the prologue's startup task be registered here
//! (RISKS R24)? The Schedule service, the task-creation policy, and whether
//! the machine is managed.

use super::registry::{self, Hive};
use super::win::run_tool;
use super::wmi::Wmi;
use serde_json::{json, Value};

pub fn resume_facts(cimv2: Option<&Wmi>) -> Value {
    let service = cimv2.and_then(|w| w.query_where("Win32_Service", &["State"], "Name='Schedule'").ok()).and_then(|l| l.into_iter().next()).map(|s| s["State"].clone()).unwrap_or(Value::Null);
    let policy = registry::dword(&Hive::LocalMachine, r"SOFTWARE\Policies\Microsoft\Windows\Task Scheduler5.0", "Task Creation");
    let domain = cimv2.and_then(|w| w.query("Win32_ComputerSystem", &["PartOfDomain"]).ok()).and_then(|l| l.into_iter().next()).map(|s| s["PartOfDomain"].clone()).unwrap_or(Value::Null);
    let (mut aad, mut mdm) = (Value::Null, Value::Null);
    if let Ok(lines) = run_tool("dsregcmd", &["/status"], 60) {
        for line in &lines {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("AzureAdJoined").map(|r| r.trim_start()) {
                if let Some(v) = rest.strip_prefix(':') {
                    let v = v.trim();
                    if v == "YES" || v == "NO" {
                        aad = json!(v == "YES");
                    }
                }
            }
            if let Some(rest) = t.strip_prefix("MdmUrl").map(|r| r.trim_start()) {
                if let Some(v) = rest.strip_prefix(':') {
                    let v = v.trim();
                    if !v.is_empty() {
                        mdm = json!(v.split_whitespace().next().unwrap_or(""));
                    }
                }
            }
        }
    }
    json!({"ScheduleService": service, "TaskCreationPolicy": policy, "DomainJoined": domain, "AzureAdJoined": aad, "Mdm": mdm})
}
