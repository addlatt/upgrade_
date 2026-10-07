//! Two sets of prologue facts of the same machine, the Rust's and the
//! PowerShell's, compared field for field. Free space may differ between
//! two reads a minute apart.

use serde_json::Value;

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Array(x), y) | (y, Value::Array(x)) if x.len() == 1 && !y.is_array() => same(&x[0], y),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q))),
        _ => a == b,
    }
}

fn drift(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(p), Some(q)) => (p - q).abs() <= p.abs().max(q.abs()) * 0.02 + 1.0,
        _ => false,
    }
}

/// The differences, and the drifts allowed (free space).
pub fn differences(rust: &Value, ps: &Value) -> (Vec<String>, Vec<String>) {
    let (mut out, mut drifted) = (Vec::new(), Vec::new());
    fn walk(path: &str, r: &Value, p: &Value, out: &mut Vec<String>, drifted: &mut Vec<String>) {
        let field = path.rsplit(['.', '[']).next().unwrap_or("");
        match (r, p) {
            (Value::Object(x), Value::Object(y)) => {
                let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
                keys.sort();
                keys.dedup();
                for k in keys {
                    let sub = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    match (x.get(k), y.get(k)) {
                        (Some(a), Some(b)) => walk(&sub, a, b, out, drifted),
                        (a, b) => out.push(format!("{sub}: Rust {} PowerShell {}", a.map_or("(absent)".to_string(), Value::to_string), b.map_or("(absent)".to_string(), Value::to_string))),
                    }
                }
            }
            _ if same(r, p) => {}
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                for (i, (a, b)) in x.iter().zip(y).enumerate() {
                    walk(&format!("{path}[{i}]"), a, b, out, drifted);
                }
            }
            _ if field == "Free" && drift(r, p) => drifted.push(format!("{path}: Rust {r} PowerShell {p}")),
            _ => out.push(format!("{path}:\n    PowerShell: {p}\n    Rust:       {r}")),
        }
    }
    walk("", rust, ps, &mut out, &mut drifted);
    (out, drifted)
}
