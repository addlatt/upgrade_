//! The job.json / outcome.json contract, read in Rust.
//!
//! `schemas/job.schema.json` and `schemas/outcome.schema.json` stay the
//! contract. This crate carries both files inside itself and checks a
//! document against them, so there is one set of rules and no second copy
//! to drift. `schemas/check.py` is the contract's own test in Python; every
//! case it runs is replayed here (`tests/contract.rs`, RISKS R32).
//!
//! Three rules, all from CLAUDE.md rule #1 (refuse by default):
//! - a schema keyword this reader does not know is a refusal when the
//!   contract is loaded, never something skipped;
//! - a document is only handed out as a [`Job`] or an [`Outcome`] after it
//!   passed, so code that holds one holds a checked document;
//! - `format: date-time` is checked for real (RFC 3339). The Python checker
//!   skips it when its optional date library is missing. Stricter, never
//!   looser.

mod validate;

use serde_json::Value;
use std::sync::OnceLock;

pub use validate::{Contract, Violation};

pub const JOB_SCHEMA: &str = include_str!("../../job.schema.json");
pub const OUTCOME_SCHEMA: &str = include_str!("../../outcome.schema.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Job,
    Outcome,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Job => "job",
            Kind::Outcome => "outcome",
        }
    }
}

/// Why a document was not accepted, in a form a screen or a log can show.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    NotJson(String),
    Violations(Vec<Violation>),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NotJson(e) => write!(f, "not JSON ({e})"),
            Refusal::Violations(v) => {
                let shown: Vec<String> = v.iter().take(6).map(|x| x.to_string()).collect();
                write!(f, "{}", shown.join("; "))?;
                if v.len() > 6 {
                    write!(f, " (+{} more)", v.len() - 6)?;
                }
                Ok(())
            }
        }
    }
}

/// The contract for one kind of document. The schema files are compiled
/// into the program, so a failure to load them is a build mistake, caught
/// by the first test that runs.
pub fn contract(kind: Kind) -> &'static Contract {
    static JOB: OnceLock<Contract> = OnceLock::new();
    static OUTCOME: OnceLock<Contract> = OnceLock::new();
    match kind {
        Kind::Job => JOB.get_or_init(|| Contract::new(JOB_SCHEMA).expect("job.schema.json loads")),
        Kind::Outcome => OUTCOME.get_or_init(|| Contract::new(OUTCOME_SCHEMA).expect("outcome.schema.json loads")),
    }
}

/// Every way `doc` breaks the contract. Empty means it is accepted.
pub fn violations(kind: Kind, doc: &Value) -> Vec<Violation> {
    contract(kind).violations(doc)
}

fn checked(kind: Kind, doc: Value) -> Result<Value, Refusal> {
    let v = violations(kind, &doc);
    if v.is_empty() { Ok(doc) } else { Err(Refusal::Violations(v)) }
}

fn parsed(kind: Kind, text: &str) -> Result<Value, Refusal> {
    // a UTF-8 byte order mark is what Windows PowerShell 5.1 writes first
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let doc: Value = serde_json::from_str(text).map_err(|e| Refusal::NotJson(e.to_string()))?;
    checked(kind, doc)
}

/// A `job.json` that passed the contract. There is no other way to make one.
#[derive(Debug, Clone, PartialEq)]
pub struct Job(Value);

/// An `outcome.json` that passed the contract.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome(Value);

macro_rules! document {
    ($name:ident, $kind:expr) => {
        impl $name {
            pub fn read(text: &str) -> Result<$name, Refusal> {
                parsed($kind, text).map($name)
            }
            pub fn from_value(doc: Value) -> Result<$name, Refusal> {
                checked($kind, doc).map($name)
            }
            pub fn as_value(&self) -> &Value {
                &self.0
            }
            pub fn into_value(self) -> Value {
                self.0
            }
            /// The field at a JSON pointer (`/scan/verdict`), if present.
            pub fn at(&self, pointer: &str) -> Option<&Value> {
                self.0.pointer(pointer)
            }
            pub fn job_id(&self) -> &str {
                // required by both schemas, so present in a checked document
                self.0.get("job_id").and_then(Value::as_str).unwrap_or("")
            }
        }
    };
}
document!(Job, Kind::Job);
document!(Outcome, Kind::Outcome);
