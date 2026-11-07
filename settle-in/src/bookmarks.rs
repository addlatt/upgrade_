//! The browsers' bookmarks, and nothing else of theirs (decided 2026-11-07,
//! the owner; RISKS R20 narrowed: no cookies, passwords, history or
//! extensions, which Windows encrypts or which do not survive a copy).
//!
//! Read from the Windows drive (mounted read-only by the pull) under the
//! person's own profile folder:
//!
//! - Chrome and Edge: each profile's `Bookmarks`, a plain JSON file.
//! - Firefox: each profile's newest `bookmarkbackups/*.jsonlz4`, the
//!   backup Firefox makes of itself (a JSON tree, LZ4-compressed behind an
//!   8-byte `mozLz40\0` and a 4-byte length). It can be some days old;
//!   the words say its date.
//!
//! Written as one bookmarks file per profile (the Netscape HTML every
//! browser imports) into the person's Documents, never into a browser's
//! own profile: how a Linux browser takes a profile written by another
//! program is what R20 says is unchecked, and a wrong guess could replace
//! bookmarks the person already has. The count read is held against the
//! count written.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// One bookmark tree: folders and links, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Folder { name: String, children: Vec<Node> },
    Link { name: String, url: String, added: Option<i64> },
}

pub fn count(nodes: &[Node]) -> usize {
    nodes.iter().map(|n| match n {
        Node::Folder { children, .. } => count(children),
        Node::Link { .. } => 1,
    }).sum()
}

/// A link a browser can open; Firefox's own `place:` queries and the like are not bookmarks.
fn wanted(url: &str) -> bool {
    ["http://", "https://", "ftp://", "file://"].iter().any(|p| url.to_ascii_lowercase().starts_with(p))
}

/// Chrome's and Edge's `Bookmarks` file. Its times are microseconds since 1601.
pub fn from_chromium(text: &str) -> Result<Vec<Node>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("it is not JSON ({})", e))?;
    let roots = v.get("roots").and_then(Value::as_object).ok_or("it has no bookmark roots")?;
    fn walk(n: &Value) -> Option<Node> {
        match n.get("type").and_then(Value::as_str) {
            Some("url") => {
                let url = n.get("url").and_then(Value::as_str)?.to_string();
                if !wanted(&url) {
                    return None;
                }
                let added = n.get("date_added").and_then(Value::as_str).and_then(|d| d.parse::<i64>().ok()).filter(|d| *d > 11_644_473_600_000_000).map(|d| (d - 11_644_473_600_000_000) / 1_000_000);
                Some(Node::Link { name: n.get("name").and_then(Value::as_str).unwrap_or("").to_string(), url, added })
            }
            Some("folder") => Some(Node::Folder {
                name: n.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                children: n.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(walk).collect(),
            }),
            _ => None,
        }
    }
    let mut out = Vec::new();
    // the bar, then the rest, as Chrome shows them
    for key in ["bookmark_bar", "other", "synced"] {
        if let Some(n) = roots.get(key).and_then(walk) {
            if matches!(&n, Node::Folder { children, .. } if !children.is_empty()) {
                out.push(n);
            }
        }
    }
    Ok(out)
}

/// Firefox's `.jsonlz4` backup. Its times are microseconds since 1970.
pub fn from_firefox_backup(bytes: &[u8]) -> Result<Vec<Node>, String> {
    if bytes.len() < 12 || &bytes[..8] != b"mozLz40\0" {
        return Err("it is not one of Firefox's compressed backups".into());
    }
    let size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if size > 256 << 20 {
        return Err("it says it is larger than any bookmark backup".into());
    }
    let raw = lz4_flex::block::decompress(&bytes[12..], size).map_err(|e| format!("it does not decompress ({})", e))?;
    let v: Value = serde_json::from_slice(&raw).map_err(|e| format!("it is not JSON ({})", e))?;
    fn walk(n: &Value) -> Option<Node> {
        match n.get("typeCode").and_then(Value::as_i64) {
            Some(1) => {
                let url = n.get("uri").and_then(Value::as_str)?.to_string();
                if !wanted(&url) {
                    return None;
                }
                Some(Node::Link { name: n.get("title").and_then(Value::as_str).unwrap_or("").to_string(), url, added: n.get("dateAdded").and_then(Value::as_i64).map(|d| d / 1_000_000) })
            }
            Some(2) => Some(Node::Folder {
                name: n.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                children: n.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(walk).collect(),
            }),
            _ => None,
        }
    }
    // placesRoot's children: the menu, the toolbar, other bookmarks, mobile; Firefox's own names
    let names = [("toolbarFolder", "Bookmarks Toolbar"), ("menuFolder", "Bookmarks Menu"), ("bookmarksMenuFolder", "Bookmarks Menu"), ("unfiledBookmarksFolder", "Other Bookmarks"), ("mobileFolder", "Mobile Bookmarks")];
    let mut out = Vec::new();
    for c in v.get("children").and_then(Value::as_array).into_iter().flatten() {
        if let Some(Node::Folder { name, children }) = walk(c) {
            if children.is_empty() {
                continue;
            }
            let root = c.get("root").and_then(Value::as_str).unwrap_or("");
            let shown = names.iter().find(|(r, _)| *r == root).map(|(_, n)| n.to_string()).unwrap_or(name);
            out.push(Node::Folder { name: shown, children });
        }
    }
    Ok(out)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The Netscape bookmarks file every browser imports.
pub fn html(title: &str, nodes: &[Node]) -> String {
    fn put(t: &mut String, nodes: &[Node], depth: usize) {
        let pad = "    ".repeat(depth);
        for n in nodes {
            match n {
                Node::Folder { name, children } => {
                    t.push_str(&format!("{pad}<DT><H3>{}</H3>\n{pad}<DL><p>\n", esc(name)));
                    put(t, children, depth + 1);
                    t.push_str(&format!("{pad}</DL><p>\n"));
                }
                Node::Link { name, url, added } => {
                    let a = added.map(|d| format!(" ADD_DATE=\"{}\"", d)).unwrap_or_default();
                    t.push_str(&format!("{pad}<DT><A HREF=\"{}\"{}>{}</A>\n", esc(url), a, esc(if name.is_empty() { url } else { name })));
                }
            }
        }
    }
    let mut t = format!("<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>{0}</TITLE>\n<H1>{0}</H1>\n<DL><p>\n", esc(title));
    put(&mut t, nodes, 1);
    t.push_str("</DL><p>\n");
    t
}

/// One browser profile's bookmarks, found on the Windows drive.
#[derive(Debug)]
pub struct Found {
    pub browser: &'static str,
    pub profile: String,
    pub nodes: Result<Vec<Node>, String>,
    /// Firefox: the backup's date, from its file name.
    pub dated: Option<String>,
}

fn dirs(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(p).map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// Every browser profile under the person's Windows profile folder (`user`,
/// the volume-relative `Users/<name>`), read but never opened for writing.
/// Only regular files are read (a link is not followed).
pub fn find(win: &Path, user: &str) -> Vec<Found> {
    let home = win.join(user);
    let mut out = Vec::new();
    let regular = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file());
    for (browser, rel) in [("Chrome", "AppData/Local/Google/Chrome/User Data"), ("Edge", "AppData/Local/Microsoft/Edge/User Data")] {
        let base = home.join(rel);
        for prof in dirs(&base).into_iter().filter(|d| d == "Default" || d.starts_with("Profile ")) {
            let f = base.join(&prof).join("Bookmarks");
            if regular(&f) {
                let nodes = std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| from_chromium(&t));
                out.push(Found { browser, profile: prof, nodes, dated: None });
            }
        }
    }
    let ff = home.join("AppData/Roaming/Mozilla/Firefox/Profiles");
    for prof in dirs(&ff) {
        let b = ff.join(&prof).join("bookmarkbackups");
        // bookmarks-YYYY-MM-DD_N_hash.jsonlz4: the newest date is the largest name
        let mut backups: Vec<String> = std::fs::read_dir(&b).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("bookmarks-") && n.ends_with(".jsonlz4")).collect()).unwrap_or_default();
        backups.sort();
        let profile = prof.split_once('.').map(|(_, n)| n.to_string()).unwrap_or(prof.clone());
        match backups.last() {
            Some(n) if regular(&b.join(n)) => {
                let nodes = std::fs::read(b.join(n)).map_err(|e| e.to_string()).and_then(|t| from_firefox_backup(&t));
                out.push(Found { browser: "Firefox", profile, nodes, dated: n.get(10..20).map(str::to_string) });
            }
            Some(_) => out.push(Found { browser: "Firefox", profile, nodes: Err("its backup is not an ordinary file".into()), dated: None }),
            // a profile Firefox never backed up holds no bookmarks to bring
            None if std::fs::symlink_metadata(ff.join(&prof).join("places.sqlite")).is_ok() => {
                out.push(Found { browser: "Firefox", profile, nodes: Err("Firefox made no backup of its bookmarks in this profile (open Firefox in Windows once and leave it a few minutes, then try again)".into()), dated: None })
            }
            None => {}
        }
    }
    out
}

/// `Users/<name>` from the folder map (the first folder under Users/).
pub fn user_dir(job: &Value) -> Option<String> {
    job.pointer("/harvest/folders").and_then(Value::as_array).into_iter().flatten()
        .filter_map(|f| crate::pull::on_volume(f.get("path").and_then(Value::as_str).unwrap_or("")))
        .find_map(|v| {
            let p: Vec<&str> = v.split('/').collect();
            (p.len() >= 2 && p[0].eq_ignore_ascii_case("Users")).then(|| format!("{}/{}", p[0], p[1]))
        })
}

fn label(f: &Found, several: bool) -> String {
    if several { format!("{} ({})", f.browser, f.profile) } else { f.browser.to_string() }
}

/// What the plan says about bookmarks (read only).
pub fn summary(found: &[Found]) -> Value {
    let several = |b: &str| found.iter().filter(|f| f.browser == b).count() > 1;
    json!(found.iter().map(|f| json!({
        "browser": label(f, several(f.browser)),
        "bookmarks": f.nodes.as_ref().ok().map(|n| count(n)),
        "why_not": f.nodes.as_ref().err(),
        "dated": f.dated,
    })).collect::<Vec<_>>())
}

/// Write one file per profile with bookmarks into `dir`, never over a file.
pub fn write(found: &[Found], dir: &Path, owner: Option<crate::pullrun::Owner>) -> Value {
    let several = |b: &str| found.iter().filter(|f| f.browser == b).count() > 1;
    let mut out = Vec::new();
    for f in found {
        let l = label(f, several(f.browser));
        let nodes = match &f.nodes {
            Ok(n) if count(n) > 0 => n,
            Ok(_) => continue,
            Err(e) => {
                out.push(json!({ "browser": l, "result": "not-read", "why": e }));
                continue;
            }
        };
        let text = html(&format!("Bookmarks from Windows ({})", l), nodes);
        // the count written, read back from the file itself
        let written = text.matches("<DT><A HREF=").count();
        if written != count(nodes) {
            out.push(json!({ "browser": l, "result": "failed", "why": format!("{} bookmarks read, {} written", count(nodes), written) }));
            continue;
        }
        match place(dir, &format!("Bookmarks from Windows - {}.html", l), text.as_bytes(), owner) {
            Ok(p) => out.push(json!({ "browser": l, "result": "written", "bookmarks": written, "file": p.to_string_lossy(), "dated": f.dated })),
            Err(e) => out.push(json!({ "browser": l, "result": "failed", "why": e })),
        }
    }
    json!(out)
}

fn place(dir: &Path, name: &str, bytes: &[u8], owner: Option<crate::pullrun::Owner>) -> Result<PathBuf, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = dir.join(format!(".upgrade_-bookmarks-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(bytes).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
    drop(f);
    if let (Some(o), Ok(c)) = (owner, std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(tmp.as_os_str()))) {
        // SAFETY: c is NUL-terminated.
        unsafe {
            libc::chown(c.as_ptr(), o.uid, o.gid);
        }
    }
    let r = (0..64u32)
        .map(|n| dir.join(if n == 0 { name.to_string() } else { crate::pullrun::clash_name(name, n) }))
        .find_map(|to| match std::fs::hard_link(&tmp, &to) {
            Ok(()) => Some(Ok(to)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
            Err(e) => Some(Err(e.to_string())),
        })
        .unwrap_or_else(|| Err("64 files of that name are already there".into()));
    let _ = std::fs::remove_file(&tmp);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: &str = r#"{ "checksum": "x", "roots": {
        "bookmark_bar": { "type": "folder", "name": "Bookmarks bar", "children": [
            { "type": "url", "name": "Bank & <Co>", "url": "https://bank.example/", "date_added": "13380000000000000" },
            { "type": "folder", "name": "Recipes", "children": [ { "type": "url", "name": "Soup", "url": "https://soup.example/" } ] },
            { "type": "url", "name": "a script", "url": "javascript:alert(1)" } ] },
        "other": { "type": "folder", "name": "Other bookmarks", "children": [ { "type": "url", "name": "", "url": "http://plain.example/" } ] },
        "synced": { "type": "folder", "name": "Mobile bookmarks", "children": [] } }, "version": 1 }"#;

    fn firefox_backup() -> Vec<u8> {
        let tree = json!({ "guid": "root________", "title": "", "typeCode": 2, "root": "placesRoot", "children": [
            { "title": "menu", "typeCode": 2, "root": "bookmarksMenuFolder", "children": [
                { "title": "News", "typeCode": 1, "uri": "https://news.example/", "dateAdded": 1_700_000_000_000_000i64 },
                { "title": "Recent tags", "typeCode": 1, "uri": "place:type=6&sort=14" },
                { "typeCode": 3 } ] },
            { "title": "toolbar", "typeCode": 2, "root": "toolbarFolder", "children": [
                { "title": "Work", "typeCode": 2, "children": [ { "title": "Mail", "typeCode": 1, "uri": "https://mail.example/" } ] } ] },
            { "title": "unfiled", "typeCode": 2, "root": "unfiledBookmarksFolder", "children": [] } ] });
        let raw = serde_json::to_vec(&tree).unwrap();
        let mut b = b"mozLz40\0".to_vec();
        b.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        b.extend_from_slice(&lz4_flex::block::compress(&raw));
        b
    }

    #[test]
    fn chromium_bookmarks() {
        let n = from_chromium(CHROME).unwrap();
        assert_eq!(count(&n), 3, "the script is not a bookmark; the empty mobile folder is dropped");
        assert_eq!(n.len(), 2);
        match &n[0] {
            Node::Folder { name, children } => {
                assert_eq!(name, "Bookmarks bar");
                assert_eq!(children[0], Node::Link { name: "Bank & <Co>".into(), url: "https://bank.example/".into(), added: Some(1_735_526_400) });
            }
            _ => panic!(),
        }
        assert!(from_chromium("{}").is_err());
    }

    #[test]
    fn firefox_backups() {
        let n = from_firefox_backup(&firefox_backup()).unwrap();
        assert_eq!(count(&n), 2, "the place: query and the separator are not bookmarks");
        let names: Vec<&str> = n.iter().map(|x| match x { Node::Folder { name, .. } => name.as_str(), _ => "" }).collect();
        assert_eq!(names, ["Bookmarks Menu", "Bookmarks Toolbar"], "Firefox's display names; the empty folder dropped");
        assert!(from_firefox_backup(b"mozLz40\0\xff\xff\xff\x7fxx").is_err());
        assert!(from_firefox_backup(b"{\"not\":1}").is_err());
    }

    #[test]
    fn the_file_every_browser_imports() {
        let t = html("Bookmarks from Windows (Chrome)", &from_chromium(CHROME).unwrap());
        assert!(t.starts_with("<!DOCTYPE NETSCAPE-Bookmark-file-1>\n"));
        assert!(t.contains("<DT><A HREF=\"https://bank.example/\" ADD_DATE=\"1735526400\">Bank &amp; &lt;Co&gt;</A>"), "{t}");
        assert!(t.contains("<DT><A HREF=\"http://plain.example/\">http://plain.example/</A>"), "a nameless link shows its address");
        assert_eq!(t.matches("<DT><A HREF=").count(), 3);
    }

    #[test]
    fn found_on_a_windows_tree_and_written() {
        let d = std::env::temp_dir().join(format!("settle-in-bookmarks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let u = d.join("win/Users/Ann");
        std::fs::create_dir_all(u.join("AppData/Local/Google/Chrome/User Data/Default")).unwrap();
        std::fs::create_dir_all(u.join("AppData/Local/Google/Chrome/User Data/System Profile")).unwrap();
        std::fs::write(u.join("AppData/Local/Google/Chrome/User Data/Default/Bookmarks"), CHROME).unwrap();
        std::fs::write(u.join("AppData/Local/Google/Chrome/User Data/System Profile/Bookmarks"), CHROME).unwrap();
        let ff = u.join("AppData/Roaming/Mozilla/Firefox/Profiles/ab12cd.default-release/bookmarkbackups");
        std::fs::create_dir_all(&ff).unwrap();
        std::fs::write(ff.join("bookmarks-2026-10-01_5_aaa.jsonlz4"), b"old").unwrap();
        std::fs::write(ff.join("bookmarks-2026-11-05_7_bbb.jsonlz4"), firefox_backup()).unwrap();
        let empty = u.join("AppData/Roaming/Mozilla/Firefox/Profiles/zz.unused");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("places.sqlite"), b"").unwrap();
        let found = find(&d.join("win"), "Users/Ann");
        let s = summary(&found);
        assert_eq!(s, json!([
            { "browser": "Chrome", "bookmarks": 3, "why_not": null, "dated": null },
            { "browser": "Firefox (default-release)", "bookmarks": 2, "why_not": null, "dated": "2026-11-05" },
            { "browser": "Firefox (unused)", "bookmarks": null, "why_not": "Firefox made no backup of its bookmarks in this profile (open Firefox in Windows once and leave it a few minutes, then try again)", "dated": null } ]), "{s:#}");
        let docs = d.join("home/Documents");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("Bookmarks from Windows - Chrome.html"), "mine").unwrap();
        let w = write(&found, &docs, None);
        assert_eq!(w[0]["result"], "written");
        assert_eq!(w[0]["file"], docs.join("Bookmarks from Windows - Chrome (from Windows).html").to_string_lossy().to_string(), "never over a file");
        assert_eq!(std::fs::read_to_string(docs.join("Bookmarks from Windows - Chrome.html")).unwrap(), "mine");
        assert_eq!(w[1]["bookmarks"], 2);
        assert_eq!(w[2]["result"], "not-read");
        assert_eq!(user_dir(&json!({ "harvest": { "folders": [{ "path": "C:\\Users\\Ann\\OneDrive\\Documents" }] } })).as_deref(), Some("Users/Ann"));
    }
}
