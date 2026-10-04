//! Remote access (SSH), decided 2026-10-04 (the owner): carried only if
//! Windows already had it on, and only as the public keys Windows allowed
//! (job harvest.ssh, written by evaluate). For a person who never set up SSH
//! nothing happens here: no server is turned on, nothing changes.
//!
//! When it was carried: the keys go into the new account's
//! ~/.ssh/authorized_keys (folder 0700, file 0600, owned by the person),
//! password sign-in over SSH is turned off by a drop-in that wins over the
//! distribution's own (sshd reads sshd_config.d in name order and keeps the
//! first value), and the SSH server is enabled and started. Every step either
//! happens or is reported with its reason; nothing is guessed.

use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const DROP_IN: &str = "etc/ssh/sshd_config.d/30-upgrade_.conf";
const DROP_IN_TEXT: &str = "# upgrade_ settle-in: remote access was carried from Windows with its keys only\n# (job harvest.ssh). Password sign-in over SSH stays off.\nPasswordAuthentication no\nKbdInteractiveAuthentication no\n";

/// The account's uid, gid and home folder from the system's passwd file.
pub fn account(passwd: &str, name: &str) -> Option<(u32, u32, String)> {
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        if f.len() >= 7 && f[0] == name { Some((f[2].parse().ok()?, f[3].parse().ok()?, f[5].to_string())) } else { None }
    })
}

/// Keys from the job that are not already in the file, in the job's order.
pub fn keys_to_add(existing: &str, keys: &[String]) -> Vec<String> {
    let have: Vec<&str> = existing.lines().map(str::trim).collect();
    let mut out: Vec<String> = Vec::new();
    for k in keys {
        let k = k.trim();
        if !k.is_empty() && !have.contains(&k) && !out.iter().any(|o| o == k) {
            out.push(k.to_string());
        }
    }
    out
}

fn sshd_installed(r: &str) -> Option<String> {
    ["usr/lib/systemd/system/sshd.service", "lib/systemd/system/sshd.service", "usr/lib/systemd/system/ssh.service", "lib/systemd/system/ssh.service"]
        .iter().find(|p| std::path::Path::new(&format!("{}/{}", r, p)).exists()).map(|p| p.to_string())
}

pub fn run(root: &str, job: &Value) -> Value {
    let r = root.trim_end_matches('/');
    let h = job.pointer("/harvest/ssh");
    let carried = h.and_then(|h| h.get("result")).and_then(Value::as_str) == Some("carried");
    let keys: Vec<String> = h.and_then(|h| h.get("keys")).and_then(Value::as_array).map(|a| a.iter().filter_map(|k| k.as_str().map(String::from)).collect()).unwrap_or_default();
    if !carried || keys.is_empty() {
        let why = h.and_then(|h| h.get("why")).and_then(Value::as_str).unwrap_or("Windows did not have remote access (SSH) on");
        return json!({ "result": "not-carried", "why": why });
    }
    let Some(unit) = sshd_installed(r) else {
        return json!({ "result": "no-ssh-server", "why": "this system has no SSH server installed; the keys were not set up", "keys": keys.len() });
    };
    let name = job.pointer("/intent/account/linux_name").and_then(Value::as_str).unwrap_or("");
    let passwd = std::fs::read_to_string(format!("{}/etc/passwd", r)).unwrap_or_default();
    let Some((uid, gid, home)) = account(&passwd, name) else {
        return json!({ "result": "failed", "why": format!("the account '{}' is not on this system", name) });
    };
    let step = |what: &str, e: std::io::Error| json!({ "result": "failed", "why": format!("{}: {}", what, e) });
    // 1. the keys, owned by the person
    let dir = format!("{}{}/.ssh", r, home);
    if let Err(e) = std::fs::create_dir_all(&dir) { return step("could not make ~/.ssh", e); }
    if let Err(e) = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)) { return step("could not set ~/.ssh to 0700", e); }
    let file = format!("{}/authorized_keys", dir);
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let add = keys_to_add(&existing, &keys);
    if !add.is_empty() {
        let mut text = String::new();
        if !existing.is_empty() && !existing.ends_with('\n') { text.push('\n'); }
        for k in &add { text.push_str(k); text.push('\n'); }
        let w = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&file).and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(e) = w { return step("could not write authorized_keys", e); }
    }
    if let Err(e) = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)) { return step("could not set authorized_keys to 0600", e); }
    for p in [&dir, &file] {
        if let Err(e) = std::os::unix::fs::chown(p, Some(uid), Some(gid)) { return step("could not give ~/.ssh to the account", e); }
    }
    // 2. keys only: the drop-in wins over the distribution's own settings
    let dropin = format!("{}/{}", r, DROP_IN);
    if let Err(e) = std::fs::create_dir_all(format!("{}/etc/ssh/sshd_config.d", r)) { return step("could not make sshd_config.d", e); }
    let w = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&dropin).and_then(|mut f| f.write_all(DROP_IN_TEXT.as_bytes()));
    if let Err(e) = w { return step("could not write the keys-only setting", e); }
    // 3. the server: enabled for every later start; on the running system also started now and the labels fixed
    let svc = std::path::Path::new(&unit).file_name().and_then(|n| n.to_str()).unwrap_or("sshd.service").to_string();
    let wants = format!("{}/etc/systemd/system/multi-user.target.wants", r);
    let _ = std::fs::create_dir_all(&wants);
    let link = format!("{}/{}", wants, svc);
    let linked = std::fs::symlink_metadata(&link).is_ok() || std::os::unix::fs::symlink(format!("/{}", unit), &link).is_ok();
    if !linked { return step("could not enable the SSH server", std::io::Error::other(link.clone())); }
    let mut started = Value::Null;
    if r.is_empty() {
        // SELinux: ~/.ssh needs its own label for sshd to read it
        let _ = std::process::Command::new("restorecon").args(["-R", &format!("{}/.ssh", home), &format!("/{}", DROP_IN)]).status();
        // settle-in runs before sysinit.target: queue the start, never wait for it
        started = json!(std::process::Command::new("systemctl").args(["--no-block", "start", &svc]).status().map(|s| s.success()).unwrap_or(false));
    }
    json!({ "result": "carried", "keys": keys.len(), "added": add.len(), "service": svc, "start_queued": started, "password_sign_in": "off" })
}

#[cfg(test)]
mod tests {
    use super::*;
    const K1: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMQQyvVmEy6AQRORsQwKHQO9f1hMJtfxm1tm/jWBqmvP laptop";
    const K2: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGq1Xn9gW7tYb0Y3cJ6Yb2b8m7pWJq4qkqv0WkQnH2xR other";

    fn root(sshd: bool) -> String {
        let d = std::env::temp_dir().join(format!("upg-ssh-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::create_dir_all(d.join("home/rig")).unwrap();
        let me = own_ids();
        std::fs::write(d.join("etc/passwd"), format!("root:x:0:0::/root:/bin/bash\nrig:x:{}:{}::/home/rig:/bin/bash\n", me.0, me.1)).unwrap();
        if sshd {
            std::fs::create_dir_all(d.join("usr/lib/systemd/system")).unwrap();
            std::fs::write(d.join("usr/lib/systemd/system/sshd.service"), "[Unit]\n").unwrap();
        }
        d.to_string_lossy().into_owned()
    }
    // the test's own uid/gid, so chown succeeds without root
    fn own_ids() -> (u32, u32) {
        std::fs::metadata("/proc/self").map(|m| (std::os::unix::fs::MetadataExt::uid(&m), std::os::unix::fs::MetadataExt::gid(&m))).unwrap_or((0, 0))
    }
    fn job(result: &str, keys: &[&str]) -> Value {
        json!({ "intent": { "account": { "linux_name": "rig" } }, "harvest": { "ssh": { "result": result, "keys": keys, "sources": [], "why": null } } })
    }

    #[test]
    fn nothing_happens_unless_windows_had_it() {
        let r = root(true);
        assert_eq!(run(&r, &job("off", &[]))["result"], "not-carried");
        assert_eq!(run(&r, &json!({ "harvest": {} }))["result"], "not-carried");
        assert!(!std::path::Path::new(&format!("{}/home/rig/.ssh", r)).exists());
        assert!(!std::path::Path::new(&format!("{}/{}", r, DROP_IN)).exists());
        assert!(!std::path::Path::new(&format!("{}/etc/systemd/system/multi-user.target.wants/sshd.service", r)).exists());
    }

    #[test]
    fn carried_keys_only_and_enabled() {
        let r = root(true);
        let out = run(&r, &job("carried", &[K1]));
        assert_eq!(out["result"], "carried", "{}", out);
        let ak = format!("{}/home/rig/.ssh/authorized_keys", r);
        assert_eq!(std::fs::read_to_string(&ak).unwrap(), format!("{}\n", K1));
        assert_eq!(std::fs::metadata(&ak).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(format!("{}/home/rig/.ssh", r)).unwrap().permissions().mode() & 0o777, 0o700);
        let d = std::fs::read_to_string(format!("{}/{}", r, DROP_IN)).unwrap();
        assert!(d.contains("PasswordAuthentication no") && d.contains("KbdInteractiveAuthentication no"));
        assert_eq!(std::fs::read_link(format!("{}/etc/systemd/system/multi-user.target.wants/sshd.service", r)).unwrap().to_string_lossy(), "/usr/lib/systemd/system/sshd.service");
        // a second run adds nothing twice
        let again = run(&r, &job("carried", &[K1, K2]));
        assert_eq!(again["added"], 1);
        assert_eq!(std::fs::read_to_string(&ak).unwrap(), format!("{}\n{}\n", K1, K2));
    }

    #[test]
    fn no_server_installed_sets_nothing_up() {
        let r = root(false);
        assert_eq!(run(&r, &job("carried", &[K1]))["result"], "no-ssh-server");
        assert!(!std::path::Path::new(&format!("{}/home/rig/.ssh", r)).exists());
    }

    #[test]
    fn an_unknown_account_is_a_failure_with_its_reason() {
        let r = root(true);
        let mut j = job("carried", &[K1]);
        j["intent"]["account"]["linux_name"] = json!("nobody-here");
        let out = run(&r, &j);
        assert_eq!(out["result"], "failed");
        assert!(out["why"].as_str().unwrap().contains("nobody-here"));
    }

    #[test]
    fn passwd_lookup() {
        assert_eq!(account("rig:x:1000:1000::/home/rig:/bin/bash\n", "rig"), Some((1000, 1000, "/home/rig".into())));
        assert_eq!(account("rigx:x:1000:1000::/home/rigx:/bin/bash\n", "rig"), None);
    }
}
