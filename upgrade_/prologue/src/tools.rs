//! Every Windows tool the prologue calls (`bcdedit`, `diskpart`,
//! `manage-bde`, `chkdsk`, `chkntfs`, `vssadmin`, `fsutil`, `schtasks`,
//! `icacls`, `powercfg`, `mountvol`, `shutdown`) runs through here, and
//! every call keeps its command line, exit code and both output streams.
//! That record travels with the state (`tools.jsonl` in the state
//! directory) and to the stick (`upgrade_/report/tools.jsonl`), so a later
//! port can replay the decisions against what the tools really printed
//! (RISKS R32: the PowerShell prologue never kept this, and its physical
//! runs cannot be replayed).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// One call of a tool, as it happened.
#[derive(Debug, Clone)]
pub struct ToolRun {
    pub utc: String,
    pub program: String,
    pub args: Vec<String>,
    /// nothing when the tool could not be started or did not finish
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// why it could not run, when it could not
    pub error: Option<String>,
}

impl ToolRun {
    pub fn to_json(&self) -> Value {
        json!({"utc": self.utc, "program": self.program, "args": self.args, "exit": self.exit, "stdout": self.stdout, "stderr": self.stderr, "error": self.error})
    }

    /// Both streams, as `2>&1` joins them, line by line.
    pub fn lines(&self) -> Vec<String> {
        let mut l: Vec<String> = self.stdout.lines().map(|x| x.trim_end_matches('\r').to_string()).collect();
        l.extend(self.stderr.lines().map(|x| x.trim_end_matches('\r').to_string()));
        l
    }

    pub fn text(&self) -> String {
        self.lines().join("\n")
    }

    pub fn ok(&self) -> bool {
        self.exit == Some(0)
    }
}

/// The record of every tool run so far, and where it is written.
#[derive(Debug, Default)]
pub struct Recorder {
    pub runs: Vec<ToolRun>,
    /// `tools.jsonl` in the state directory, appended to as runs happen
    pub file: Option<PathBuf>,
    /// the same on the stick, when it is known
    pub stick_file: Option<PathBuf>,
}

fn now_utc() -> String {
    let (_, o) = upgrade_scan::collect::now();
    o
}

/// What a tool printed, as the console would show it. Windows tools print
/// in the console code page (OEM) and some in UTF-16; this reads UTF-8 or
/// ASCII as such and anything else by its bytes, never losing a line.
fn decode(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|b| *b as char).collect(),
    }
}

impl Recorder {
    pub fn new(file: Option<PathBuf>) -> Recorder {
        Recorder { runs: Vec::new(), file, stick_file: None }
    }

    fn keep(&mut self, run: ToolRun) -> &ToolRun {
        let line = run.to_json().to_string() + "\n";
        for p in [&self.file, &self.stick_file].into_iter().flatten() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = f.write_all(line.as_bytes());
            }
        }
        self.runs.push(run);
        self.runs.last().expect("just pushed")
    }

    /// Run a tool with arguments and keep what it said.
    pub fn run(&mut self, program: &str, args: &[&str]) -> &ToolRun {
        let utc = now_utc();
        let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output();
        let run = match out {
            Ok(o) => ToolRun { utc, program: program.to_string(), args: args.iter().map(|a| a.to_string()).collect(), exit: o.status.code(), stdout: decode(&o.stdout), stderr: decode(&o.stderr), error: None },
            Err(e) => ToolRun { utc, program: program.to_string(), args: args.iter().map(|a| a.to_string()).collect(), exit: None, stdout: String::new(), stderr: String::new(), error: Some(e.to_string()) },
        };
        self.keep(run)
    }

    /// Run a command line through `cmd /c` (for a pipe such as
    /// `echo Y| chkdsk C: /f`) and keep what it said.
    pub fn run_cmd(&mut self, command_line: &str) -> &ToolRun {
        self.run("cmd", &["/c", command_line])
    }

    /// Write everything kept so far to one file (the stick's copy).
    pub fn write_all(&self, path: &Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let text: String = self.runs.iter().map(|r| r.to_json().to_string() + "\n").collect();
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_keeps_its_streams_and_exit() {
        let mut r = Recorder::new(None);
        #[cfg(windows)]
        let t = r.run("cmd", &["/c", "echo hi && exit 3"]).clone();
        #[cfg(not(windows))]
        let t = r.run("sh", &["-c", "echo hi; exit 3"]).clone();
        assert_eq!(t.exit, Some(3));
        assert_eq!(t.lines(), vec!["hi"]);
        assert!(!t.ok());
        assert_eq!(r.runs.len(), 1);
        let missing = r.run("no-such-program-upgrade", &[]).clone();
        assert!(missing.error.is_some() && missing.exit.is_none());
    }

    #[test]
    fn utf16_output_is_read() {
        let mut b = vec![0xFF, 0xFE];
        for u in "ok\r\n".encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&b), "ok\r\n");
        assert_eq!(decode(b"plain"), "plain");
    }
}
