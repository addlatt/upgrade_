//! "You had these programs; here is the Linux version" (decided 2026-11-07,
//! the owner: a list that names a Linux or web version for each program;
//! settle-in installs nothing, architecture.md Module 3).
//!
//! The job's `harvest.software` (names only) is matched against
//! data/programs.json, compiled in. The table's Flathub entries carry the
//! date Flathub's own catalog confirmed them (data/tools/check-programs.py).
//! A program the table does not know is listed as not known, never guessed.

use serde_json::{json, Value};
use std::sync::OnceLock;

const TABLE: &str = include_str!("../../data/programs.json");
const KINDS: [&str; 5] = ["same", "alternative", "web", "built-in", "not-needed"];

pub struct Rule {
    pattern: regress::Regex,
    kind: String,
    linux: String,
    flathub: String,
    web: String,
    note: String,
}

fn parse(text: &str) -> Result<Vec<Rule>, String> {
    let t: Value = serde_json::from_str(text).map_err(|e| format!("data/programs.json is not JSON ({e})"))?;
    let list = t.get("Programs").and_then(Value::as_array).ok_or("data/programs.json has no Programs list")?;
    let s = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).map(str::to_string).ok_or(format!("data/programs.json: an entry has no {k}"));
    list.iter()
        .map(|e| {
            let m = s(e, "Match")?;
            let kind = s(e, "Kind")?;
            if !KINDS.contains(&kind.as_str()) {
                return Err(format!("data/programs.json: {m}: Kind {kind} is not one of {KINDS:?}"));
            }
            let pattern = regress::Regex::with_flags(&m, "i").map_err(|x| format!("data/programs.json: the pattern {m} does not read ({x})"))?;
            Ok(Rule { pattern, kind, linux: s(e, "Linux")?, flathub: s(e, "Flathub")?, web: s(e, "Web")?, note: s(e, "Note")? })
        })
        .collect()
}

fn table() -> &'static [Rule] {
    static T: OnceLock<Vec<Rule>> = OnceLock::new();
    // the table is checked by the tests and by data/tools/check-programs.py; a
    // table that does not read lists every program as not known, never panics
    T.get_or_init(|| parse(TABLE).unwrap_or_default())
}

/// The list for the public summary. Names only, as the job has them.
pub fn list(job: &Value) -> Value {
    list_with(job, table())
}

fn list_with(job: &Value, rules: &[Rule]) -> Value {
    let Some(sw) = job.pointer("/harvest/software").filter(|v| v.is_object()) else {
        return json!({ "result": "not-in-job" });
    };
    let mut names: Vec<String> = Vec::new();
    for k in ["desktop", "store"] {
        for p in sw.get(k).and_then(Value::as_array).cloned().unwrap_or_default() {
            if let Some(n) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
                if !names.iter().any(|x| x.eq_ignore_ascii_case(n)) {
                    names.push(n.to_string());
                }
            }
        }
    }
    let mut hits: Vec<(usize, Vec<String>)> = Vec::new();
    let mut unknown = Vec::new();
    let mut not_needed = 0;
    for n in &names {
        match rules.iter().position(|r| r.pattern.find(n).is_some()) {
            Some(i) if rules[i].kind == "not-needed" => not_needed += 1,
            Some(i) => match hits.iter_mut().find(|(j, _)| *j == i) {
                Some((_, had)) => had.push(n.clone()),
                None => hits.push((i, vec![n.clone()])),
            },
            None => unknown.push(n.clone()),
        }
    }
    // the same program first, then a different one, the web, what the desktop does already
    hits.sort_by_key(|(i, had)| (KINDS.iter().position(|k| *k == rules[*i].kind), had[0].to_lowercase()));
    unknown.sort_by_key(|n| n.to_lowercase());
    let items: Vec<Value> = hits
        .iter()
        .map(|(i, had)| {
            let r = &rules[*i];
            json!({ "had": had, "kind": r.kind, "linux": r.linux, "flathub": r.flathub, "web": r.web, "note": r.note })
        })
        .collect();
    json!({
        "result": if names.is_empty() { "none" } else { "listed" },
        "truncated": sw.get("truncated") == Some(&json!(true)),
        "items": items,
        "unknown": unknown,
        "not_needed": not_needed,
    })
}

/// The section's lines. Draft words (2026-11-07), for the owner's approval.
pub fn lines(p: &Value) -> Vec<String> {
    let mut l = Vec::new();
    match p["result"].as_str() {
        Some("listed") => {}
        Some("none") => return vec!["Windows listed no programs of yours.".into()],
        _ => return vec!["The list of your Windows programs did not come across, so there is nothing to show here.".into()],
    }
    let items = p["items"].as_array().cloned().unwrap_or_default();
    if !items.is_empty() {
        l.push("Windows had these programs. Here is what to use on Linux:".into());
    }
    for it in &items {
        let had = it["had"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let linux = it["linux"].as_str().unwrap_or("");
        let get = match (it["flathub"].as_str().unwrap_or(""), it["web"].as_str().unwrap_or("")) {
            ("", "") => String::new(),
            ("", w) => format!(" Get it at {}.", w.trim_start_matches("https://")),
            (f, _) => format!(" Get it from the Software app, or at flathub.org/apps/{}.", f),
        };
        let mut line = match it["kind"].as_str() {
            Some("same") => format!("{had}: {linux} runs on Linux too.{get}"),
            Some("alternative") => format!("{had}: use {linux} instead.{get}"),
            Some("web") => format!("{had}: use {linux}, at {}.", it["web"].as_str().unwrap_or("").trim_start_matches("https://")),
            _ => format!("{had}: {linux} does this already."),
        };
        if let Some(n) = it["note"].as_str().filter(|n| !n.is_empty()) {
            line.push(' ');
            line.push_str(n);
        }
        l.push(format!("  {line}"));
    }
    let unknown = p["unknown"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default();
    if !unknown.is_empty() {
        l.push("We do not know a Linux version of these yet:".into());
        l.push(format!("  {}", unknown.join(", ")));
    }
    match p["not_needed"].as_u64().unwrap_or(0) {
        0 => {}
        1 => l.push("One more was a driver, a maker's tool or a part of Windows; Linux does not need it.".into()),
        n => l.push(format!("{n} more were drivers, the maker's tools or parts of Windows; Linux needs none of them.")),
    }
    if p["truncated"] == json!(true) {
        l.push("Windows' list of programs was cut short, so some may be missing here.".into());
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(desktop: &[&str], store: &[&str]) -> Value {
        let d: Vec<Value> = desktop.iter().map(|n| json!({ "name": n })).collect();
        let s: Vec<Value> = store.iter().map(|n| json!({ "name": n })).collect();
        json!({ "harvest": { "software": { "desktop": d, "store": s, "truncated": false } } })
    }

    #[test]
    fn the_table_reads() {
        let t = parse(TABLE).expect("data/programs.json must read");
        assert!(t.len() > 30, "{}", t.len());
        for r in &t {
            assert!(r.kind == "not-needed" || !r.linux.is_empty());
            assert!(r.kind != "web" || r.web.starts_with("https://"));
        }
    }

    #[test]
    fn a_bad_table_is_refused() {
        assert!(parse(r#"{"Programs":[{"Match":"(","Kind":"same","Linux":"x","Flathub":"","Web":"","Note":""}]}"#).is_err());
        assert!(parse(r#"{"Programs":[{"Match":"x","Kind":"maybe","Linux":"x","Flathub":"","Web":"","Note":""}]}"#).is_err());
    }

    #[test]
    fn matches_sorts_and_counts() {
        let j = job(
            &["Google Chrome", "Microsoft Visual C++ 2015-2022 Redistributable (x64)", "Microsoft Edge WebView2 Runtime", "Microsoft Edge", "Zoom Workplace", "7-Zip 24.08 (x64)", "MyASUS", "Microsoft Teams", "Some Tool 1.0"],
            &["WhatsApp", "Spotify Music"],
        );
        let p = list(&j);
        assert_eq!(p["result"], "listed");
        assert_eq!(p["not_needed"], 3, "the runtime, WebView2 and the maker's tool, not the Edge browser");
        assert_eq!(p["unknown"], json!(["Some Tool 1.0"]));
        let kinds: Vec<&str> = p["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["same", "same", "same", "same", "web", "web", "built-in"]);
        let t = lines(&p).join("\n");
        assert!(t.contains("  Google Chrome: Google Chrome runs on Linux too. Get it from the Software app, or at flathub.org/apps/com.google.Chrome."), "{t}");
        assert!(t.contains("  Microsoft Teams: use Teams in the browser, at teams.microsoft.com."), "{t}");
        assert!(t.contains("  7-Zip 24.08 (x64): the desktop's file manager does this already."), "{t}");
        assert!(t.contains("We do not know a Linux version of these yet:\n  Some Tool 1.0"), "{t}");
        assert!(t.contains("3 more were drivers, the maker's tools or parts of Windows; Linux needs none of them."), "{t}");
    }

    #[test]
    fn two_names_one_program_is_one_line() {
        let p = list(&job(&["Zoom", "Zoom Workplace"], &[]));
        assert_eq!(p["items"].as_array().unwrap().len(), 1);
        assert!(lines(&p)[1].starts_with("  Zoom, Zoom Workplace: Zoom runs on Linux too."));
    }

    #[test]
    fn nothing_and_old_jobs_and_cut_short() {
        assert_eq!(lines(&list(&json!({ "harvest": {} })))[0], "The list of your Windows programs did not come across, so there is nothing to show here.");
        assert_eq!(lines(&list(&job(&[], &[])))[0], "Windows listed no programs of yours.");
        let mut j = job(&["Some Tool"], &[]);
        j["harvest"]["software"]["truncated"] = json!(true);
        assert!(lines(&list(&j)).last().unwrap().contains("cut short"));
    }
}
