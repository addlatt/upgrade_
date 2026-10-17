//! `post`: the installed system's last settings, the work the kickstart's
//! chroot `%post` did in shell up to kickstart generator 0.10.0. Since
//! upgrade-installer 0.1.3 (2026-10-17) `outcome` runs it first, so it
//! still comes before anything `outcome` reads from the installed system
//! (grub.cfg, the default target), in the shell's order:
//!
//! 1. keep-windows only (RISKS R21, R36; architecture step 12): os-prober
//!    off, GRUB's own environment block on the ESP, our Windows entry with
//!    the ESP's UUID, grub.cfg made again, its Windows lines counted;
//! 2. every path: no desktop first-run wizard, the start target the
//!    person chose, and what the target and the firmware's entries read.
//!
//! Files are written by the Rust straight into the installed system; the
//! installed system's own tools (grub2-editenv, grub2-mkconfig, systemctl,
//! efibootmgr) run through chroot, as they did inside the chroot %post.
//! The GRUB menu entry stays GRUB script, a fixed text. Every command and
//! its output goes to /root/upgrade_-post.log, the file the %post's --log
//! wrote, so `outcome` still copies it to the stick as report/post.log.
//! Nothing here aborts: a step that fails is written down and the next runs.

use crate::util::*;
use std::os::unix::fs::PermissionsExt;

/// The Windows entry (architecture step 12, decided 2026-10-10): it
/// chainloads Windows' loader under its kept name when shim sits in
/// Windows' slot, and first tells the prologue's return that Windows was
/// chosen from this menu (amended 2026-10-11). The flag lives in its own
/// environment block ON THE ESP: GRUB's default block is
/// /boot/grub2/grubenv on ext4, which Windows cannot read (kickstart
/// 0.10.0, 2026-10-12). `exec tail -n +3 $0` makes grub2-mkconfig copy the
/// lines after it into grub.cfg as they are. ESPUUID is replaced.
const KEPT_ENTRY: &str = "#!/bin/sh
exec tail -n +3 $0
menuentry 'Windows (kept)' --class windows --class os {
    insmod part_gpt
    insmod fat
    insmod chain
    search --no-floppy --fs-uuid --set=root ESPUUID
    set upg_windows_chosen=1
    save_env -f /EFI/fedora/grubenv upg_windows_chosen
    if [ -f /EFI/Microsoft/Boot/bootmgfw-kept.efi ]; then chainloader /EFI/Microsoft/Boot/bootmgfw-kept.efi; else chainloader /EFI/Microsoft/Boot/bootmgfw.efi; fi
}
";

/// The kept entry for an ESP.
pub fn kept_entry(esp_uuid: &str) -> String {
    KEPT_ENTRY.replace("ESPUUID", esp_uuid)
}

/// /etc/default/grub with os-prober off (R21: Windows is in the menu by our
/// doing, not by luck): every GRUB_DISABLE_OS_PROBER= line says true, or
/// the line is added when there is none.
pub fn os_prober_off(default_grub: &str) -> String {
    const KEY: &str = "GRUB_DISABLE_OS_PROBER=";
    if default_grub.lines().any(|l| l.starts_with(KEY)) {
        let mut out: String = default_grub.lines().map(|l| if l.starts_with(KEY) { format!("{KEY}true\n") } else { format!("{l}\n") }).collect();
        if !default_grub.ends_with('\n') {
            out.pop();
        }
        out
    } else {
        let sep = if default_grub.is_empty() || default_grub.ends_with('\n') { "" } else { "\n" };
        format!("{default_grub}{sep}{KEY}true\n")
    }
}

/// `grep -c -i windows grub.cfg`: lines naming Windows, in any case.
pub fn windows_lines(grub_cfg: &str) -> usize {
    grub_cfg.lines().filter(|l| l.to_lowercase().contains("windows")).count()
}

/// What the computer starts at is the person's choice (2026-09-26, the
/// Aspire's run 9): desktop = graphical sign-in, console = text.
pub fn start_target(start_at: &str) -> &'static str {
    if start_at.eq_ignore_ascii_case("console") { "multi-user.target" } else { "graphical.target" }
}

struct PostLog {
    path: String,
}

impl PostLog {
    fn line(&self, text: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(text.as_bytes());
            if !text.is_empty() && !text.ends_with('\n') {
                let _ = f.write_all(b"\n");
            }
        }
    }

    /// `set -x`: the command, then what it printed.
    fn tool(&self, sysroot: &str, args: &[&str]) -> Ran {
        self.line(&format!("+ {}", args.join(" ")));
        let mut all = vec![sysroot];
        all.extend_from_slice(args);
        let r = run("chroot", &all);
        self.line(&r.merged());
        if !r.ok() {
            self.line(&format!("+ (exit {})", r.status));
        }
        r
    }

    fn wrote(&self, path: &str, ok: bool) {
        self.line(&format!("+ write {path}{}", if ok { "" } else { " FAILED" }));
    }
}

fn write_mode(path: &str, text: &str, mode: u32) -> bool {
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, text).is_ok() && std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_ok()
}

/// What `outcome` logs of it.
pub struct Done {
    pub windows_entries: Option<usize>,
    pub target: &'static str,
    pub target_set: bool,
}

pub fn main(sysroot: &str, path_chosen: &str, start_at: &str) -> Done {
    let log = PostLog { path: format!("{sysroot}/root/upgrade_-post.log") };
    let _ = std::fs::create_dir_all(format!("{sysroot}/root"));
    log.line(&format!("== upgrade-installer {} post {}", env!("CARGO_PKG_VERSION"), utc_now()));
    let mut written: Vec<&str> = Vec::new();
    let mut windows_entries = None;
    if path_chosen.eq_ignore_ascii_case("keep-windows") {
        let dg = format!("{sysroot}/etc/default/grub");
        let mode = std::fs::metadata(&dg).map(|m| m.permissions().mode() & 0o7777).unwrap_or(0o644);
        let ok = write_mode(&dg, &os_prober_off(&read_to_string(&dg)), mode);
        log.wrote("/etc/default/grub (GRUB_DISABLE_OS_PROBER=true)", ok);
        written.push("/etc/default/grub");
        let r = run("findmnt", &["-no", "UUID", &format!("{sysroot}/boot/efi")]);
        let esp_uuid = r.out.trim().to_string();
        log.line(&format!("+ findmnt -no UUID /boot/efi: {}", if esp_uuid.is_empty() { "(none: the entry cannot find the ESP)" } else { &esp_uuid }));
        log.tool(sysroot, &["grub2-editenv", "/boot/efi/EFI/fedora/grubenv", "create"]);
        let ok = write_mode(&format!("{sysroot}/etc/grub.d/25_windows_kept"), &kept_entry(&esp_uuid), 0o755);
        log.wrote("/etc/grub.d/25_windows_kept", ok);
        written.push("/etc/grub.d/25_windows_kept");
        log.tool(sysroot, &["grub2-mkconfig", "-o", "/boot/grub2/grub.cfg"]);
        let n = std::fs::read_to_string(format!("{sysroot}/boot/grub2/grub.cfg")).ok().map(|t| windows_lines(&t));
        let _ = std::fs::write(format!("{sysroot}/root/upgrade_-grub-windows-entries.txt"), n.map(|n| format!("{n}\n")).unwrap_or_default());
        log.line(&format!("+ grub.cfg lines naming Windows: {}", n.map(|n| n.to_string()).unwrap_or_else(|| "(grub.cfg unreadable)".into())));
        written.push("/root/upgrade_-grub-windows-entries.txt");
        windows_entries = n;
    }
    // The account is made by the kickstart, so no desktop's first-run wizard
    // may ask for one (Fedora 44 KDE's Plasma Setup, the rig, 2026-10-03).
    let ok = write_mode(&format!("{sysroot}/etc/plasma-setup-done"), "", 0o644);
    log.wrote("/etc/plasma-setup-done", ok);
    written.push("/etc/plasma-setup-done");
    log.tool(sysroot, &["systemctl", "disable", "plasma-setup.service"]);
    let target = start_target(start_at);
    let target_set = log.tool(sysroot, &["systemctl", "set-default", target]).ok();
    let got = log.tool(sysroot, &["systemctl", "get-default"]);
    let _ = std::fs::write(format!("{sysroot}/root/upgrade_-default-target.txt"), got.merged());
    let fw = log.tool(sysroot, &["efibootmgr", "-v"]);
    let _ = std::fs::write(format!("{sysroot}/root/upgrade_-efibootmgr.txt"), fw.merged());
    written.extend(["/root/upgrade_-default-target.txt", "/root/upgrade_-efibootmgr.txt", "/root/upgrade_-post.log"]);
    // written from outside the chroot, so labelled from inside it
    let mut rc = vec!["restorecon"];
    rc.extend(written);
    log.tool(sysroot, &rc);
    Done { windows_entries, target, target_set }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kept_entry_names_the_esp_and_is_grub_script() {
        let e = kept_entry("ABCD-1234");
        assert!(e.starts_with("#!/bin/sh\nexec tail -n +3 $0\nmenuentry 'Windows (kept)'"));
        assert!(e.contains("search --no-floppy --fs-uuid --set=root ABCD-1234\n"));
        assert!(!e.contains("ESPUUID"));
        assert!(e.contains("save_env -f /EFI/fedora/grubenv upg_windows_chosen"));
        assert!(e.contains("chainloader /EFI/Microsoft/Boot/bootmgfw-kept.efi; else chainloader /EFI/Microsoft/Boot/bootmgfw.efi; fi"));
        assert!(e.ends_with("}\n"));
    }

    #[test]
    fn os_prober_is_turned_off_whatever_the_file_said() {
        assert_eq!(os_prober_off("GRUB_TIMEOUT=5\nGRUB_DISABLE_OS_PROBER=false\nGRUB_X=1\n"), "GRUB_TIMEOUT=5\nGRUB_DISABLE_OS_PROBER=true\nGRUB_X=1\n");
        assert_eq!(os_prober_off("GRUB_TIMEOUT=5\n"), "GRUB_TIMEOUT=5\nGRUB_DISABLE_OS_PROBER=true\n");
        assert_eq!(os_prober_off("GRUB_TIMEOUT=5"), "GRUB_TIMEOUT=5\nGRUB_DISABLE_OS_PROBER=true\n");
        assert_eq!(os_prober_off(""), "GRUB_DISABLE_OS_PROBER=true\n");
        // a commented line is not the setting, as with sed's ^
        assert_eq!(os_prober_off("#GRUB_DISABLE_OS_PROBER=false\n"), "#GRUB_DISABLE_OS_PROBER=false\nGRUB_DISABLE_OS_PROBER=true\n");
        assert_eq!(os_prober_off("GRUB_DISABLE_OS_PROBER=false"), "GRUB_DISABLE_OS_PROBER=true");
    }

    #[test]
    fn windows_lines_are_counted_as_grep_c_i_counts_them() {
        assert_eq!(windows_lines("menuentry 'Fedora' {\n}\nmenuentry 'Windows (kept)' --class windows --class os {\n"), 1);
        assert_eq!(windows_lines("WINDOWS\nwindows windows\nx\n"), 2);
        assert_eq!(windows_lines(""), 0);
    }

    #[test]
    fn the_start_target_is_the_persons_choice() {
        assert_eq!(start_target("console"), "multi-user.target");
        assert_eq!(start_target("Console"), "multi-user.target");
        assert_eq!(start_target("desktop"), "graphical.target");
    }
}
