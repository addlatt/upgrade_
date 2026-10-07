//! Two folder maps of the same machine, the Rust's and the PowerShell's,
//! compared field for field. Sizes drift between two reads a minute apart
//! (a browser writes its cache, a download lands), so counts and bytes
//! are allowed 2 percent and one unit; everything else must be equal.

use serde_json::Value;

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        // ConvertTo-Json writes a one-item array as the item
        (Value::Array(x), y) | (y, Value::Array(x)) if x.len() == 1 && !y.is_array() => same(&x[0], y),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q))),
        _ => a == b,
    }
}

fn within_drift(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(p), Some(q)) => (p - q).abs() <= p.abs().max(q.abs()) * 0.02 + 1.0,
        _ => false,
    }
}

/// The differences, and the drifts that were allowed.
pub fn map_differences(rust: &Value, ps: &Value) -> (Vec<String>, Vec<String>) {
    let (mut out, mut drifted) = (Vec::new(), Vec::new());
    let drifts = |path: &str| {
        let field = path.rsplit('.').next().unwrap_or("");
        matches!(field, "Files" | "Bytes" | "MaxFileBytes" | "StickBytes" | "FreeBytes" | "FilesBytes" | "NeededBytes" | "GapBytes")
    };
    fn walk(path: &str, r: &Value, p: &Value, drifts: &dyn Fn(&str) -> bool, out: &mut Vec<String>, drifted: &mut Vec<String>) {
        match (r, p) {
            (Value::Object(x), Value::Object(y)) => {
                let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
                keys.sort();
                keys.dedup();
                for k in keys {
                    let sub = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    match (x.get(k), y.get(k)) {
                        (Some(a), Some(b)) => walk(&sub, a, b, drifts, out, drifted),
                        (a, b) => out.push(format!("{sub}: Rust {} PowerShell {}", a.map_or("(absent)".to_string(), Value::to_string), b.map_or("(absent)".to_string(), Value::to_string))),
                    }
                }
            }
            _ if same(r, p) => {}
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                for (i, (a, b)) in x.iter().zip(y).enumerate() {
                    walk(&format!("{path}[{i}]"), a, b, drifts, out, drifted);
                }
            }
            _ if drifts(path) && within_drift(r, p) => drifted.push(format!("{path}: Rust {r} PowerShell {p}")),
            _ if path.ends_with(".Reason") && r.as_str().is_some_and(|s| s.starts_with("the folders need")) && p.as_str().is_some_and(|s| s.starts_with("the folders need")) => drifted.push(format!("{path}: Rust {r} PowerShell {p}")),
            _ => out.push(format!("{path}:\n    PowerShell: {p}\n    Rust:       {r}")),
        }
    }
    // the Rust's own fields, and the clock
    let mut r = rust.clone();
    if let Some(m) = r.as_object_mut() {
        m.remove("Harvester");
    }
    let mut p = ps.clone();
    for side in [&mut r, &mut p] {
        if let Some(m) = side.as_object_mut() {
            m.remove("HarvestedUtc");
        }
    }
    // folders by name, so an order difference is named as such
    let by_name = |v: &Value| -> Vec<String> { v["UserFolders"].as_array().into_iter().flatten().map(|f| f["Name"].as_str().unwrap_or("").to_string()).collect() };
    if by_name(&r) != by_name(&p) {
        out.push(format!("UserFolders: Rust lists {:?}, PowerShell {:?}", by_name(&r), by_name(&p)));
    }
    walk("", &r, &p, &drifts, &mut out, &mut drifted);
    (out, drifted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn equal_maps_are_the_same_and_sizes_may_drift() {
        let a = json!({"HarvestVersion": "0.3.0", "Harvester": "x", "HarvestedUtc": "a", "Owner": {"ProcessSid": "S-1"}, "UserFolders": [{"Name": "Desktop", "Bytes": 1000, "Files": 10}], "CloudFiles": {"Result": "none-found"}, "Stick": null, "StickFit": null});
        let b = json!({"HarvestVersion": "0.3.0", "HarvestedUtc": "b", "Owner": {"ProcessSid": "S-1"}, "UserFolders": [{"Name": "Desktop", "Bytes": 1010, "Files": 10}], "CloudFiles": {"Result": "none-found"}, "Stick": null, "StickFit": null});
        let (d, drifted) = map_differences(&a, &b);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(drifted.len(), 1);
        let c = json!({"HarvestVersion": "0.3.0", "HarvestedUtc": "b", "Owner": {"ProcessSid": "S-2"}, "UserFolders": [{"Name": "Desktop", "Bytes": 5000, "Files": 10}], "CloudFiles": {"Result": "none-found"}, "Stick": null, "StickFit": null});
        let (d, _) = map_differences(&a, &c);
        assert_eq!(d.len(), 2, "{d:?}");
    }
}
