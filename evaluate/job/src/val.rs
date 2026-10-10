//! Reading a JSON fact the way PowerShell reads a property: a missing field
//! is nothing, nothing prints as an empty text, and `if ($x)` has its own
//! idea of true.

use serde_json::Value;

static NULL: Value = Value::Null;

/// `$v.A.B`: the field at a dotted path, or nothing.
pub fn at<'a>(v: &'a Value, path: &str) -> &'a Value {
    let mut cur = v;
    for key in path.split('.') {
        cur = cur.get(key).unwrap_or(&NULL);
    }
    cur
}

/// A value inside a "..." string.
pub fn s(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(t) => t.clone(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.to_string(),
            (None, Some(f)) => format!("{f}"),
            _ => n.to_string(),
        },
        Value::Array(l) => l.iter().map(s).collect::<Vec<_>>().join(" "),
        Value::Object(_) => "System.Object".to_string(),
    }
}

/// `if ($v)`.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(t) => !t.is_empty(),
        Value::Array(l) => match l.len() {
            0 => false,
            1 => truthy(&l[0]),
            _ => true,
        },
        Value::Object(_) => true,
    }
}

/// `[int]$v` and `[long]$v`: nothing is 0, a fraction rounds to the even
/// neighbour, a text of digits is read.
pub fn int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().unwrap_or_else(|| n.as_f64().map_or(0, |f| f.round_ties_even() as i64)),
        Value::String(t) => t.trim().parse().unwrap_or(0),
        Value::Bool(b) => *b as i64,
        _ => 0,
    }
}

/// `@($v)`: a list stays a list, nothing is a list of one nothing (as in
/// PowerShell), anything else is a list of one.
pub fn list(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(l) => l.iter().collect(),
        other => vec![other],
    }
}

/// The items of a list, with nothing for a missing one.
pub fn items(v: &Value) -> Vec<&Value> {
    match v {
        Value::Null => Vec::new(),
        other => list(other),
    }
}

/// `-eq` on text: case does not matter.
pub fn eq_ci(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// `-in @(...)` on text.
pub fn one_of(text: &str, options: &[&str]) -> bool {
    options.iter().any(|o| eq_ci(o, text))
}
