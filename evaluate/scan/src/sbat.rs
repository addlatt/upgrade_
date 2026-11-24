//! Secure Boot revocations (SBAT) and which Linux releases can start
//! (RISKS R34). The rule is shim's own: a revocation level is a list of
//! `component,generation` lines, and a boot file that names a component with
//! a lower generation than the level requires is refused.

use crate::check::{ReleaseVerdict, Scan, Status};
use crate::data::Release;
use crate::facts::{KitBootFile, SbatSource};
use crate::ps::{eq_ci, matches, s, truthy, Version};

/// `component -> generation`, in the order first seen, plus the level's date.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SbatMap {
    pub date: Option<String>,
    pub parts: Vec<(String, i32)>,
}

impl SbatMap {
    fn get(&self, name: &str) -> Option<i32> {
        self.parts.iter().find(|(k, _)| eq_ci(k, name)).map(|(_, g)| *g)
    }
    fn raise(&mut self, name: &str, generation: i32) {
        match self.parts.iter_mut().find(|(k, _)| eq_ci(k, name)) {
            Some((_, g)) => *g = (*g).max(generation),
            None => self.parts.push((name.to_string(), generation)),
        }
    }
}

/// A revocation level (`sbat,1,2025051000 / shim,4 / grub,5`) or a boot
/// file's `.sbat` section, read into a map.
pub fn parse(text: &str) -> SbatMap {
    let mut out = SbatMap::default();
    for line in text.split(['\r', '\n']) {
        let f: Vec<&str> = line.trim_matches(['\0', ' ']).split(',').collect();
        if f.len() < 2 || f[0].is_empty() {
            continue;
        }
        let Ok(generation) = f[1].trim().parse::<i32>() else { continue };
        if eq_ci(f[0], "sbat") {
            if f.len() >= 3 && matches(r"^\d{10}$", f[2]) {
                out.date = Some(f[2].to_string());
            }
            continue;
        }
        out.raise(f[0], generation);
    }
    out
}

/// The strictest of several levels: per component the highest generation,
/// and the newest date. Every source counts.
pub fn merge(levels: &[SbatMap]) -> SbatMap {
    let mut m = SbatMap { date: Some(String::new()), parts: Vec::new() };
    for l in levels {
        if let Some(d) = &l.date {
            if d.as_str() > m.date.as_deref().unwrap_or("") {
                m.date = Some(d.clone());
            }
        }
        for (k, g) in &l.parts {
            m.raise(k, *g);
        }
    }
    m
}

/// The components of one boot file that the level refuses, as
/// `grub,3 < grub,5`. Empty means the level allows the file.
pub fn revoked(level: &SbatMap, file: &SbatMap) -> Vec<String> {
    file.parts
        .iter()
        .filter_map(|(k, g)| level.get(k).filter(|need| g < need).map(|need| format!("{k},{g} < {k},{need}")))
        .collect()
}

const TITLE: &str = "Secure Boot revocations";

/// Test-UpgSbat. `secure_boot`: 1 on, 0 off, nothing = could not read (and
/// then it is judged as on).
pub fn judge_sbat(scan: &mut Scan, secure_boot: Option<i64>, levels: &[SbatSource], files: &[KitBootFile]) {
    let readable: Vec<&SbatSource> = levels.iter().filter(|l| truthy(&l.text)).collect();
    let parsed: Vec<SbatMap> = readable.iter().map(|l| parse(s(&l.text))).collect();
    let level = merge(&parsed);
    let lv = level.parts.iter().map(|(k, g)| format!("{k},{g}")).collect::<Vec<_>>().join(" ");
    let lv_text = if lv.is_empty() {
        "no revocation level found".to_string()
    } else {
        let date = level.date.as_deref().filter(|d| !d.is_empty()).unwrap_or("(no date)");
        format!("level {date}: {lv}")
    };
    let sources = readable.iter().map(|l| s(&l.source)).collect::<Vec<_>>().join(", ");

    if files.is_empty() {
        let from = if sources.is_empty() { "nothing readable" } else { &sources };
        scan.add("Fundamentals", TITLE, Status::Info, lv_text).note(format!(
            "Secure Boot refuses Linux boot programs older than this level (from: {from}). Run the scanner from the upgrade_ stick to check the stick's own boot files against it."
        ));
        return;
    }
    let (mut bad, mut unread) = (Vec::new(), Vec::new());
    for f in files {
        if truthy(&f.error) || !truthy(&f.sbat) {
            let why = if truthy(&f.error) { s(&f.error) } else { "no SBAT data" };
            unread.push(format!("{} ({why})", s(&f.name)));
            continue;
        }
        let r = revoked(&level, &parse(s(&f.sbat)));
        if !r.is_empty() {
            bad.push(format!("{}: {}", s(&f.name), r.join(", ")));
        }
    }
    if secure_boot == Some(0) && (!bad.is_empty() || !unread.is_empty()) {
        let all: Vec<String> = bad.iter().chain(&unread).cloned().collect();
        scan.add("Fundamentals", TITLE, Status::Warn, format!("Secure Boot is off; with it on, these would be refused: {}", all.join("; ")))
            .note(format!("{lv_text}. Turning Secure Boot on later would stop Linux from starting."))
            .remedy("Leave Secure Boot as it is until a kit with newer boot files is used.");
        return;
    }
    if !bad.is_empty() {
        scan.add("Fundamentals", TITLE, Status::Fail, format!("this computer's Secure Boot refuses the stick's boot files: {}", bad.join("; ")))
            .note(format!("{lv_text} (from: {sources}). The computer would restart, refuse to start Linux, and come back to Windows. Nothing has been changed."))
            .remedy("This kit cannot convert this computer. It needs a kit built with newer boot files. Do not turn Secure Boot off to get around it.");
        return;
    }
    if !unread.is_empty() {
        scan.add("Fundamentals", TITLE, Status::Fail, format!("the stick's boot files could not be checked: {}", unread.join("; ")))
            .note(format!("{lv_text}. A boot file that cannot be checked is treated as refused."))
            .remedy("Rebuild the stick with the kit builder, then scan again.");
        return;
    }
    let names = files.iter().map(|f| s(&f.name)).collect::<Vec<_>>().join(", ");
    scan.add("Fundamentals", TITLE, Status::Ok, format!("the stick's boot files meet this computer's {lv_text}"))
        .note(format!("Checked: {names} (from: {sources})."));
}

/// Test-UpgReleases: which releases in the table this computer can start,
/// from the stick and once installed. `db_authorities`: the certificate
/// names the firmware trusts, or nothing when they could not be read.
pub fn judge_releases(scan: &mut Scan, secure_boot: Option<i64>, level: &SbatMap, db_authorities: Option<&[String]>, table: &[Release]) {
    scan.releases.clear();
    for r in table {
        let (mut why, mut unknown) = (Vec::new(), Vec::new());
        if !truthy(&r.installer) {
            why.push("the kit cannot install it unattended yet".to_string());
        }
        if secure_boot != Some(0) {
            let mut all = vec![level.clone()];
            all.extend(r.boot.iter().flat_map(|b| &b.sbat_level).filter(|t| !t.is_empty()).map(|t| parse(t)));
            let lv = merge(&all);
            for f in &r.boot {
                let bad = revoked(&lv, &parse(s(&f.sbat)));
                if !bad.is_empty() {
                    why.push(format!("{} {}: {}", f.role, f.file, bad.join(", ")));
                }
            }
            for f in r.boot.iter().filter(|f| eq_ci(&f.file, "BOOTX64.EFI") || eq_ci(&f.file, "shimx64.efi")) {
                let Some(db) = db_authorities else {
                    unknown.push(format!("{} {}", f.role, f.file));
                    continue;
                };
                if !f.signed_by.iter().any(|a| db.iter().any(|d| eq_ci(a, d))) {
                    let first = f.signed_by.first().map(String::as_str).unwrap_or("");
                    why.push(format!("{} {}: signed by {first}, which this computer's Secure Boot does not trust", f.role, f.file));
                }
            }
        }
        let starts = if !why.is_empty() { "no" } else if !unknown.is_empty() { "unknown" } else { "yes" };
        let kernel = r.installed_kernel().map_or(String::new(), |(_, text)| text.to_string());
        scan.releases.push(ReleaseVerdict { id: r.id.clone(), name: r.name.clone(), starts: starts.into(), why, kernel });
    }
    releases_check(scan, false);
}

/// The kernel half of the release check: a release whose installed kernel is
/// older than the newest one this computer's hardware asks for is a "no",
/// said plainly, so the picker does not offer it and the job writer refuses
/// it. It runs after the hardware checks (they say what kernel is needed) and
/// rewrites the "Linux releases" line where it stands. A release with no
/// measured kernel is not guessed at: it is a "no" too.
pub fn judge_release_kernels(scan: &mut Scan, required: Option<Version>, table: &[Release]) {
    let Some(need) = required else { return };
    let mut changed = false;
    for v in scan.releases.iter_mut() {
        let Some(r) = table.iter().find(|r| r.id == v.id) else { continue };
        let why = match r.installed_kernel() {
            Some((has, _)) if has >= need => continue,
            Some((has, _)) => format!("its kernel {has} is older than the {need} this computer's hardware needs"),
            None => format!("its kernel was not measured, and this computer's hardware needs {need} or newer"),
        };
        v.why.push(why);
        v.starts = "no".into();
        changed = true;
    }
    if !changed {
        return;
    }
    let at = scan.checks.iter().position(|c| c.title == RELEASES_TITLE);
    if let Some(at) = at {
        scan.checks.remove(at);
    }
    releases_check(scan, true);
    if let (Some(at), Some(made)) = (at, scan.checks.pop()) {
        scan.checks.insert(at, made);
    }
}

const RELEASES_TITLE: &str = "Linux releases";

/// The one "Linux releases" line, from the verdicts so far. `by_kernel`: a
/// kernel that is too old took part, so "start" alone would be the wrong word.
fn releases_check(scan: &mut Scan, by_kernel: bool) {
    let named = |state: &str| scan.releases.iter().filter(|r| r.starts == state).cloned().collect::<Vec<_>>();
    let (yes, unk, no) = (named("yes"), named("unknown"), named("no"));
    let no_text = no.iter().map(|r| format!("{}: {}", r.name, r.why.join("; "))).collect::<Vec<_>>().join(" | ");
    const T: &str = RELEASES_TITLE;
    if !yes.is_empty() {
        let names = yes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ");
        scan.add("Fundamentals", T, Status::Ok, format!("starts on this computer: {names}"))
            .note(if no.is_empty() { "Every release in the kit's table starts here.".to_string() } else { format!("Not these: {no_text}") });
    } else if !unk.is_empty() {
        scan.add("Fundamentals", T, Status::Unknown, "could not read which signing keys this computer trusts")
            .note(if no.is_empty() { "The release check needs the firmware's key list.".to_string() } else { format!("Not these: {no_text}") })
            .remedy("Run the scanner as administrator.");
    } else {
        scan.add("Fundamentals", T, Status::Fail, if by_kernel { "no release the kit carries can be used on this computer" } else { "no release the kit carries can start on this computer" })
            .note(if no_text.is_empty() { "The release table is empty.".to_string() } else { no_text })
            .remedy("Nothing has been changed. A kit with a newer release is needed.");
    }
}

#[cfg(test)]
mod kernel_tests {
    use super::*;
    use crate::data::Kernel;

    fn release(id: &str, kernel: &str) -> Release {
        Release { id: id.into(), name: id.into(), installer: Some("kickstart".into()), kernels: vec![Kernel { role: "installed".into(), file: "vmlinuz".into(), release: kernel.into(), sha256: String::new() }], ..Default::default() }
    }

    /// The rewritten line stays where the first one stood: the report and
    /// the JSON keep their order.
    #[test]
    fn the_releases_line_is_rewritten_in_place() {
        let table = [release("old", "6.8.0-31-generic"), release("new", "7.0.0-30-generic")];
        let mut sc = Scan::new();
        sc.add("Fundamentals", "Before", Status::Ok, "");
        judge_releases(&mut sc, Some(0), &SbatMap::default(), None, &table);
        sc.add("Hardware", "After", Status::Ok, "");
        judge_release_kernels(&mut sc, Version::parse("6.10"), &table);
        let titles: Vec<&str> = sc.checks.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Before", "Linux releases", "After"]);
        assert_eq!(sc.checks[1].status, Status::Ok);
        assert!(sc.checks[1].note.contains("its kernel 6.8.0 is older than the 6.10 this computer's hardware needs"), "{}", sc.checks[1].note);
        assert_eq!((sc.releases[0].starts.as_str(), sc.releases[1].starts.as_str()), ("no", "yes"));
    }

    /// Every release too old is a hardware "no": the verdict is RED.
    #[test]
    fn nothing_new_enough_is_a_fail() {
        let table = [release("old", "6.8.0-31-generic")];
        let mut sc = Scan::new();
        judge_releases(&mut sc, Some(0), &SbatMap::default(), None, &table);
        judge_release_kernels(&mut sc, Version::parse("6.10"), &table);
        assert_eq!(sc.checks.len(), 1);
        assert_eq!(sc.checks[0].status, Status::Fail);
        assert_eq!(crate::verdict::verdict(&sc).level, crate::verdict::Level::Red);
    }
}
