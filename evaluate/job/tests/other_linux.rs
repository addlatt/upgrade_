//! RISKS R39: a keep-Windows job is refused beside a Linux already on the
//! Windows disk. These cases are the Rust's own (the PowerShell never had
//! the check), so they sit beside `parity.rs`, not in its goldens. The base
//! facts are the same made-up machine `parity.rs` uses.

use serde_json::{json, Value};
use upgrade_job::document::{job_document, Choices, Stamp};
use upgrade_job::{ERASE_STATEMENT, FOLLOWS_JOB_WRITER};

const HASH: &str = "$6$rounds=5000$saltsaltsaltsalt$0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ./0123456789abcdefghijklmnopqrstuv";

fn facts(overrides: Value) -> Value {
    let mut f: Value = serde_json::from_str(include_str!("base-facts.json")).unwrap();
    for (k, v) in overrides.as_object().into_iter().flatten() {
        f[k.as_str()] = v.clone();
    }
    f
}

fn job(f: &Value, erase: &str) -> Result<Value, Vec<String>> {
    let choice = Choices { desktop: "kde", start_at: "desktop", password_hash: HASH, report_rel: "upgrade-report.json", acknowledge_data_loss: "", erase_everything: erase };
    job_document(f, &choice, &Stamp { job_id: "00000000-0000-4000-8000-000000000000", now_utc: "2026-11-11T12:00:00Z", writer_version: FOLLOWS_JOB_WRITER })
}

fn aspire() -> Value {
    // the Aspire on 2026-10-28: its 2026-10-11 Fedora still there after the rollback
    json!({"LinuxPartitions": [{"Number": 5, "Kind": "Linux boot", "Size": 1.07e9}, {"Number": 6, "Kind": "Linux filesystem", "Size": 25.77e9}], "LinuxBootFolders": ["fedora"], "OtherLinuxError": null})
}

#[test]
fn the_base_machine_with_no_linux_gets_its_job() {
    let j = job(&facts(json!({"LinuxPartitions": [], "LinuxBootFolders": [], "OtherLinuxError": null})), "").expect("a job");
    assert_eq!(j["intent"]["path"], "keep-windows");
}

#[test]
fn keep_windows_is_refused_beside_a_linux_already_there() {
    let why = job(&facts(aspire()), "").expect_err("a refusal");
    assert_eq!(why.len(), 1, "{why:?}");
    assert!(why[0].starts_with("a Linux is already on this computer's Windows disk (partition 5 (Linux boot, 1.1 GB); partition 6 (Linux filesystem, 25.8 GB); the boot folder EFI\\fedora)"), "{}", why[0]);
    assert!(why[0].contains("no job; nothing was changed"));
}

#[test]
fn one_sign_is_enough() {
    let only_folder = job(&facts(json!({"LinuxPartitions": [], "LinuxBootFolders": ["ubuntu"], "OtherLinuxError": null})), "").expect_err("a refusal");
    assert!(only_folder[0].contains("the boot folder EFI\\ubuntu"));
    let only_partition = job(&facts(json!({"LinuxPartitions": [{"Number": 4, "Kind": "Linux LVM", "Size": 5e10}], "LinuxBootFolders": [], "OtherLinuxError": null})), "").expect_err("a refusal");
    assert!(only_partition[0].contains("partition 4 (Linux LVM, 50.0 GB)"));
}

#[test]
fn a_disk_that_could_not_be_read_is_a_refusal_not_a_pass() {
    let why = job(&facts(json!({"LinuxPartitions": [], "LinuxBootFolders": [], "OtherLinuxError": "the boot partition could not be mounted"})), "").expect_err("a refusal");
    assert!(why[0].contains("could not be read for a Linux already on it (the boot partition could not be mounted)"), "{}", why[0]);
}

#[test]
fn the_clean_slate_is_not_refused_for_it() {
    // the erase removes the old Linux with everything else
    let j = job(&facts(aspire()), ERASE_STATEMENT).expect("a job");
    assert_eq!(j["intent"]["path"], "clean-slate");
}
