//! Two sets of job facts of the same machine, the Rust's and the
//! PowerShell's, compared field for field. Clocks and what each program
//! wrote about itself are skipped; free space and folder sizes may drift
//! between two reads a minute apart; the two program inventories compare
//! as sets of names.

use serde_json::Value;

/// The facts a side-by-side comparison skips or allows to drift: clocks,
/// free space, and what the program itself wrote.
pub fn facts_differences(rust: &Value, ps: &Value) -> (Vec<String>, Vec<String>) {
    let (mut out, mut drifted) = (Vec::new(), Vec::new());
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
    fn walk(path: &str, r: &Value, p: &Value, out: &mut Vec<String>, drifted: &mut Vec<String>) {
        let field = path.rsplit(['.', '[']).next().unwrap_or("");
        let clock = matches!(field, "NowUtc" | "HarvestedUtc" | "Report") || path == "Harvest.Harvester";
        if clock {
            return;
        }
        match (r, p) {
            (Value::Object(x), Value::Object(y)) => {
                let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
                keys.sort();
                keys.dedup();
                for k in keys {
                    let sub = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    if sub == "Harvest.Harvester" {
                        continue;
                    }
                    match (x.get(k), y.get(k)) {
                        (Some(a), Some(b)) => walk(&sub, a, b, out, drifted),
                        (a, b) => out.push(format!("{sub}: Rust {} PowerShell {}", a.map_or("(absent)".to_string(), Value::to_string), b.map_or("(absent)".to_string(), Value::to_string))),
                    }
                }
            }
            _ if same(r, p) => {}
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() && field != "desktop" && field != "store" => {
                for (i, (a, b)) in x.iter().zip(y).enumerate() {
                    walk(&format!("{path}[{i}]"), a, b, out, drifted);
                }
            }
            (Value::Array(x), Value::Array(y)) if field == "desktop" || field == "store" => {
                // the two inventories compare as sets of names
                let names = |l: &Vec<Value>| -> std::collections::BTreeSet<String> { l.iter().map(|e| e["name"].as_str().unwrap_or("").to_lowercase()).collect() };
                let (a, b) = (names(x), names(y));
                for n in a.difference(&b) {
                    out.push(format!("{path}: only Rust lists {n}"));
                }
                for n in b.difference(&a) {
                    out.push(format!("{path}: only PowerShell lists {n}"));
                }
            }
            _ if matches!(field, "ShrinkGB" | "EspFree" | "Files" | "Bytes" | "MaxFileBytes" | "StickBytes" | "FreeBytes" | "FilesBytes" | "NeededBytes" | "GapBytes") && drift(r, p) => drifted.push(format!("{path}: Rust {r} PowerShell {p}")),
            _ => out.push(format!("{path}:\n    PowerShell: {p}\n    Rust:       {r}")),
        }
    }
    walk("", rust, ps, &mut out, &mut drifted);
    (out, drifted)
}

