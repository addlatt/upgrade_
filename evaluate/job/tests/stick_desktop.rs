//! A stick the window filled carries one desktop and says which in its
//! release.json. A job that asks for another is refused (Fedora and Ubuntu in
//! parallel, 2026-11-13: an Ubuntu stick carries GNOME only). The Rust's own
//! cases, beside `parity.rs`, not in its goldens.

use serde_json::{json, Value};
use upgrade_job::document::{job_document, Choices, Stamp};
use upgrade_job::FOLLOWS_JOB_WRITER;

const HASH: &str = "$6$rounds=5000$saltsaltsaltsalt$0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ./0123456789abcdefghijklmnopqrstuv";

fn job(desktop_on_stick: Option<&str>, asked: &str) -> Result<Value, Vec<String>> {
    let mut f: Value = serde_json::from_str(include_str!("base-facts.json")).unwrap();
    f["LinuxPartitions"] = json!([]);
    f["LinuxBootFolders"] = json!([]);
    f["OtherLinuxError"] = Value::Null;
    match desktop_on_stick {
        Some(d) => f["KitRelease"]["desktop"] = json!(d),
        None => {
            f["KitRelease"].as_object_mut().unwrap().remove("desktop");
        }
    }
    let choice = Choices { desktop: asked, start_at: "desktop", password_hash: HASH, report_rel: "upgrade-report.json", acknowledge_data_loss: "", erase_everything: "" };
    job_document(&f, &choice, &Stamp { job_id: "00000000-0000-4000-8000-000000000000", now_utc: "2026-11-13T12:00:00Z", writer_version: FOLLOWS_JOB_WRITER })
}

#[test]
fn the_desktop_the_stick_carries_gets_its_job() {
    assert_eq!(job(Some("gnome"), "gnome").expect("a job")["intent"]["desktop"], "gnome");
    assert_eq!(job(Some("GNOME"), "gnome").expect("a job")["intent"]["desktop"], "gnome");
}

#[test]
fn another_desktop_than_the_sticks_is_refused() {
    let why = job(Some("gnome"), "kde").expect_err("a refusal");
    assert_eq!(why.len(), 1, "{why:?}");
    assert!(why[0].contains("with the gnome desktop, not kde"), "{}", why[0]);
}

#[test]
fn a_stick_that_names_no_desktop_carries_both() {
    // the kit builder's stick (make-kit.sh --release) carries every desktop of its release
    assert!(job(None, "kde").is_ok() && job(None, "gnome").is_ok());
}
