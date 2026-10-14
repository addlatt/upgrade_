//! The live halves of `upgrade-scan diag`: what `Diag-SecureBoot.ps1` and
//! `Diag-Smart.ps1` read, read through the same bindings as the scanner's
//! own collectors. Read-only: nothing on the machine is written.

use super::events::{self, within};
use super::registry::{self, Hive};
use super::storage::{bus_type, count, health_status, int, media_type, operational_status, storage_wmi, text};
use super::win::{self, firmware_variable_ex, privilege_status, win32_message};
use super::wmi::{Wmi, CIMV2, WMI_ROOT};
use crate::diag::{secureboot, smart};
use serde_json::Value;
use windows::Win32::Security::SE_SYSTEM_ENVIRONMENT_NAME;

const DAY: i64 = 86400;

fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

/// `.GetValue()` as .NET gives a registry value: a DWORD and a QWORD as
/// signed numbers, a string as itself, a multi-string as its strings, a
/// binary value as its bytes.
fn reg_data(kind: u32, bytes: &[u8]) -> Value {
    let utf16 = |b: &[u8]| -> Vec<u16> { b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() };
    match kind {
        // REG_SZ, REG_EXPAND_SZ (expanded by the read)
        1 | 2 => Value::String(String::from_utf16_lossy(&utf16(bytes).into_iter().take_while(|u| *u != 0).collect::<Vec<_>>())),
        // REG_DWORD
        4 if bytes.len() >= 4 => Value::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        // REG_QWORD
        11 if bytes.len() >= 8 => Value::from(i64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8]))),
        // REG_MULTI_SZ
        7 => {
            let units = utf16(bytes);
            let mut strings: Vec<Value> = units.split(|u| *u == 0).map(|s| Value::String(String::from_utf16_lossy(s))).collect();
            while strings.last().is_some_and(|s| s.as_str() == Some("")) {
                strings.pop();
            }
            Value::Array(strings)
        }
        _ => Value::Array(bytes.iter().map(|b| Value::from(*b)).collect()),
    }
}

/// The key and every key under it, depth first, as `Get-Item` and
/// `Get-ChildItem -Recurse` list them.
fn registry_tree(key: &str) -> Vec<secureboot::Key> {
    fn walk(key: &str, out: &mut Vec<secureboot::Key>) {
        let values = registry::value_names(&Hive::LocalMachine, key)
            .into_iter()
            .map(|name| {
                let data = registry::value(&Hive::LocalMachine, key, &name).map(|(kind, bytes)| reg_data(kind, &bytes)).unwrap_or(Value::Null);
                secureboot::RegValue { name, data }
            })
            .collect();
        out.push(secureboot::Key { key: format!("HKLM\\{key}"), values });
        for sub in registry::subkeys(&Hive::LocalMachine, key) {
            walk(&format!("{key}\\{sub}"), out);
        }
    }
    let mut out = Vec::new();
    if registry::key_exists(&Hive::LocalMachine, key) {
        walk(key, &mut out);
    }
    out
}

/// Diag-SecureBoot.ps1's reads. `kit_root`: the stick's root, where the
/// boot files sit.
pub fn secureboot_facts(kit_root: Option<&std::path::Path>) -> secureboot::Facts {
    let (_, now) = super::now();
    let mut f = secureboot::Facts { now, computer: computer_name(), ..Default::default() };
    f.privilege = privilege_status(SE_SYSTEM_ENVIRONMENT_NAME);
    let elevated = f.privilege == "ok";
    for name in secureboot::VARIABLES {
        f.variables.push(match firmware_variable_ex(name, secureboot::SHIM_GUID) {
            Ok((bytes, attributes)) => secureboot::Variable { name: name.into(), status: secureboot::variable_ok(attributes), bytes: Some(bytes) },
            Err(code) => secureboot::Variable { name: name.into(), status: secureboot::variable_error(code), bytes: None },
        });
    }
    for name in secureboot::DATABASES {
        f.databases.push(if !elevated {
            secureboot::Database { name: name.into(), bytes: None, error: Some(secureboot::NOT_ELEVATED.into()) }
        } else {
            match firmware_variable_ex(name, secureboot::SECURITY_DB_GUID) {
                Ok((bytes, _)) => secureboot::Database { name: name.into(), bytes: Some(bytes), error: None },
                Err(code) => secureboot::Database { name: name.into(), bytes: None, error: Some(win32_message(code)) },
            }
        });
    }
    f.confirm_secure_boot = if !elevated {
        secureboot::NOT_ELEVATED.into()
    } else {
        match firmware_variable_ex("SecureBoot", secureboot::GLOBAL_GUID) {
            Ok((bytes, _)) => if bytes.first() == Some(&1) { "True".into() } else { "False".into() },
            Err(code) => win32_message(code),
        }
    };
    f.registry = registry_tree(secureboot::REGISTRY_KEY);
    for provider in secureboot::PROVIDERS {
        if let Ok(list) = events::query("System", &format!("*[System[Provider[@Name='{provider}'] and {}]]", within(secureboot::DAYS * DAY))) {
            for e in list.into_iter().filter(|e| secureboot::event_wanted(e.id)) {
                f.events.push(secureboot::Event { time_utc: e.time_utc, provider: provider.into(), id: e.id, message: e.message });
            }
        }
    }
    for name in secureboot::FILES {
        let bytes = kit_root.map(|r| r.join(name)).filter(|p| p.is_file()).and_then(|p| std::fs::read(p).ok());
        f.files.push(secureboot::File { name: name.into(), bytes });
    }
    f
}

fn opt_int(v: &Value) -> Option<i64> {
    int(v)
}

fn opt_u64(v: &Value) -> Option<u64> {
    count(v).as_u64()
}

/// A local event time with its ticks, for ordering and printing.
fn time_local_ticks(e: &events::Event) -> String {
    let frac = e.time_utc.split_once('.').map(|(_, f)| f.trim_end_matches('Z')).unwrap_or("0000000");
    format!("{}.{frac}", e.time_local.iso())
}

fn event_list(xpath: &str) -> Vec<smart::Event> {
    events::query("System", xpath).unwrap_or_default().into_iter().map(|e| smart::Event { time_local: time_local_ticks(&e), provider: e.provider, id: e.id, message: e.message }).collect()
}

fn bytes_of(v: &Value) -> Option<Vec<u8>> {
    v.as_array().map(|l| l.iter().filter_map(Value::as_u64).map(|x| x as u8).collect())
}

/// `root\wmi`'s failure-prediction classes: the error, or the instances.
fn wmi_root_read<T>(wmi: &Result<Wmi, String>, class: &str, properties: &[&str], make: impl Fn(&Value) -> T) -> smart::Read<T> {
    match wmi.as_ref().map_err(|e| e.clone()).and_then(|w| w.query(class, properties)) {
        Ok(list) => smart::Read { error: None, items: list.iter().map(make).collect() },
        // Get-CimInstance prints the provider's own text for this one, which
        // ends in a space (an NVMe drive on the G16, 2026-10-14)
        Err(e) if e == "Not supported" => smart::Read { error: Some("Not supported ".into()), items: Vec::new() },
        Err(e) => smart::Read { error: Some(e), items: Vec::new() },
    }
}

/// Diag-Smart.ps1's reads.
pub fn smart_facts() -> smart::Facts {
    let (_, now) = super::now();
    let mut f = smart::Facts { now, computer: computer_name(), ..Default::default() };
    let storage = storage_wmi();
    let counters = ["Wear", "Temperature", "TemperatureMax", "PowerOnHours", "ReadErrorsUncorrected", "WriteErrorsUncorrected", "ReadErrorsTotal", "WriteErrorsTotal", "ReadLatencyMax", "WriteLatencyMax", "StartStopCycleCount", "LoadUnloadCycleCount"];
    if let Ok(w) = &storage {
        let disks = w.query("MSFT_PhysicalDisk", &["ObjectId", "DeviceId", "FriendlyName", "BusType", "MediaType", "HealthStatus", "OperationalStatus", "Size", "FirmwareVersion", "SerialNumber"]).unwrap_or_default();
        for d in &disks {
            let mut disk = smart::PhysicalDisk {
                device_id: text(&d["DeviceId"]),
                friendly_name: text(&d["FriendlyName"]),
                bus_type: bus_type(&d["BusType"]),
                media_type: media_type(&d["MediaType"]),
                health_status: health_status(&d["HealthStatus"]),
                operational_status: operational_status(&d["OperationalStatus"], false).split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
                size: opt_u64(&d["Size"]),
                firmware_version: text(&d["FirmwareVersion"]),
                serial_number: text(&d["SerialNumber"]),
                reliability: None,
                reliability_error: None,
            };
            let assoc = format!("ASSOCIATORS OF {{{}}} WHERE AssocClass=MSFT_PhysicalDiskToStorageReliabilityCounter", text(&d["__RELPATH"]));
            match w.wql(&assoc, &counters) {
                Ok(l) if !l.is_empty() => {
                    let c = &l[0];
                    let g = |name: &str| opt_int(&c[name]);
                    disk.reliability = Some(smart::Reliability {
                        wear: g("Wear"),
                        temperature: g("Temperature"),
                        temperature_max: g("TemperatureMax"),
                        power_on_hours: g("PowerOnHours"),
                        read_errors_uncorrected: g("ReadErrorsUncorrected"),
                        write_errors_uncorrected: g("WriteErrorsUncorrected"),
                        read_errors_total: g("ReadErrorsTotal"),
                        write_errors_total: g("WriteErrorsTotal"),
                        read_latency_max: g("ReadLatencyMax"),
                        write_latency_max: g("WriteLatencyMax"),
                        start_stop_cycle_count: g("StartStopCycleCount"),
                        load_unload_cycle_count: g("LoadUnloadCycleCount"),
                    });
                }
                // without elevation the association answers nothing, where the
                // Storage cmdlet refuses in words (the G16, 2026-10-14)
                Ok(_) if !win::is_admin() => disk.reliability_error = Some(smart::COUNTERS_NOT_ELEVATED.into()),
                Ok(_) => {}
                Err(e) if e == "Access denied" => disk.reliability_error = Some(smart::COUNTERS_NOT_ELEVATED.into()),
                Err(e) => disk.reliability_error = Some(e),
            }
            f.physical_disks.push(disk);
        }
    }
    if let Ok(w) = Wmi::connect(CIMV2) {
        for d in w.query("Win32_DiskDrive", &["Index", "Model", "InterfaceType", "Status", "PNPDeviceID"]).unwrap_or_default() {
            f.disk_drives.push(smart::DiskDrive { index: opt_int(&d["Index"]), model: text(&d["Model"]), interface_type: text(&d["InterfaceType"]), status: text(&d["Status"]), pnp_device_id: text(&d["PNPDeviceID"]) });
        }
    }
    let wmi = Wmi::connect(WMI_ROOT);
    f.failure_predict_status = wmi_root_read(&wmi, "MSStorageDriver_FailurePredictStatus", &["InstanceName", "PredictFailure", "Reason"], |s| smart::StatusItem { instance_name: text(&s["InstanceName"]), predict_failure: s["PredictFailure"].as_bool(), reason: opt_int(&s["Reason"]) });
    f.failure_predict_data = wmi_root_read(&wmi, "MSStorageDriver_FailurePredictData", &["InstanceName", "VendorSpecific"], |s| smart::DataItem { instance_name: text(&s["InstanceName"]), vendor_specific: bytes_of(&s["VendorSpecific"]) });
    f.failure_predict_thresholds = wmi_root_read(&wmi, "MSStorageDriver_FailurePredictThresholds", &["InstanceName", "VendorSpecific"], |s| smart::DataItem { instance_name: text(&s["InstanceName"]), vendor_specific: bytes_of(&s["VendorSpecific"]) });
    f.disk_events = event_list(&format!("*[System[Provider[@Name='disk'] and {}]]", within(smart::DAYS * DAY)));
    let providers = smart::STORAGE_PROVIDERS.iter().map(|p| format!("@Name='{p}'")).collect::<Vec<_>>().join(" or ");
    f.storage_events = event_list(&format!("*[System[Provider[{providers}] and {}]]", within(smart::DAYS * DAY)));
    if let Ok(w) = &storage {
        for v in w.query("MSFT_Volume", &["DriveLetter", "FileSystemLabel", "FileSystem", "HealthStatus", "OperationalStatus", "Size", "SizeRemaining"]).unwrap_or_default() {
            // a char16: a number through COM, or a one-character string; 0 or empty = no letter
            let letter = match &v["DriveLetter"] {
                Value::Number(n) => n.as_u64().and_then(|u| char::from_u32(u as u32)).filter(|c| *c != '\0').map(|c| c.to_string()).unwrap_or_default(),
                other => text(other),
            };
            if letter.is_empty() {
                continue;
            }
            f.volumes.push(smart::Volume {
                drive_letter: letter,
                file_system_label: text(&v["FileSystemLabel"]),
                file_system: text(&v["FileSystem"]),
                health_status: health_status(&v["HealthStatus"]),
                operational_status: operational_status(&v["OperationalStatus"], true),
                size: opt_u64(&v["Size"]),
                size_remaining: opt_u64(&v["SizeRemaining"]),
            });
        }
    }
    f
}
