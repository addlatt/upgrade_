//! A JSON Schema draft-07 reader for exactly the keywords the two schema
//! files use. Small on purpose: the trust model is "read the source".
//! A keyword outside this list refuses the schema when it is loaded.

use regress::Regex;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// Where in the document, as a JSON pointer. Empty is the root.
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let at = if self.path.is_empty() { "<root>" } else { &self.path };
        write!(f, "{}: {}", at, self.message)
    }
}

pub struct Contract {
    root: Value,
    patterns: HashMap<String, Regex>,
}

const ANNOTATIONS: [&str; 7] = ["$schema", "$id", "title", "description", "$comment", "default", "examples"];
const TYPES: [&str; 7] = ["string", "number", "integer", "boolean", "object", "array", "null"];

impl Contract {
    pub fn new(schema_text: &str) -> Result<Contract, String> {
        let root: Value = serde_json::from_str(schema_text).map_err(|e| format!("the schema is not JSON ({e})"))?;
        match root.get("$schema").and_then(Value::as_str) {
            Some("http://json-schema.org/draft-07/schema#") => {}
            other => return Err(format!("the schema names a draft this reader does not know: {other:?}")),
        }
        let mut c = Contract { root, patterns: HashMap::new() };
        let mut patterns = HashMap::new();
        c.check_schema(&c.root, "#", &mut patterns)?;
        c.patterns = patterns;
        Ok(c)
    }

    fn resolve(&self, reference: &str) -> Option<&Value> {
        let name = reference.strip_prefix("#/definitions/")?;
        self.root.get("definitions")?.get(name)
    }

    fn check_schema(&self, node: &Value, at: &str, patterns: &mut HashMap<String, Regex>) -> Result<(), String> {
        let obj = match node {
            Value::Bool(_) => return Ok(()),
            Value::Object(o) => o,
            _ => return Err(format!("{at}: a schema must be an object or true/false")),
        };
        for (key, v) in obj {
            let here = format!("{at}/{key}");
            match key.as_str() {
                k if ANNOTATIONS.contains(&k) => {}
                "definitions" | "properties" => {
                    let map = v.as_object().ok_or(format!("{here}: must be an object"))?;
                    for (name, sub) in map {
                        self.check_schema(sub, &format!("{here}/{name}"), patterns)?;
                    }
                }
                "additionalProperties" | "not" | "if" | "then" | "else" | "contains" | "additionalItems" => {
                    self.check_schema(v, &here, patterns)?
                }
                "items" => match v {
                    Value::Array(list) => {
                        for (i, sub) in list.iter().enumerate() {
                            self.check_schema(sub, &format!("{here}/{i}"), patterns)?;
                        }
                    }
                    _ => self.check_schema(v, &here, patterns)?,
                },
                "allOf" | "anyOf" | "oneOf" => {
                    let list = v.as_array().filter(|l| !l.is_empty()).ok_or(format!("{here}: must be a non-empty list"))?;
                    for (i, sub) in list.iter().enumerate() {
                        self.check_schema(sub, &format!("{here}/{i}"), patterns)?;
                    }
                }
                "type" => {
                    let names: Vec<&Value> = match v {
                        Value::Array(l) => l.iter().collect(),
                        _ => vec![v],
                    };
                    for n in names {
                        if !n.as_str().is_some_and(|s| TYPES.contains(&s)) {
                            return Err(format!("{here}: unknown type {n}"));
                        }
                    }
                }
                "required" => {
                    if !v.as_array().is_some_and(|l| l.iter().all(Value::is_string)) {
                        return Err(format!("{here}: must be a list of names"));
                    }
                }
                "enum" => {
                    if !v.is_array() {
                        return Err(format!("{here}: must be a list"));
                    }
                }
                "const" => {}
                "pattern" => {
                    let p = v.as_str().ok_or(format!("{here}: must be a string"))?;
                    if !patterns.contains_key(p) {
                        let re = Regex::new(p).map_err(|e| format!("{here}: {e}"))?;
                        patterns.insert(p.to_string(), re);
                    }
                }
                "format" => {
                    if v.as_str() != Some("date-time") {
                        return Err(format!("{here}: a format this reader cannot check: {v}"));
                    }
                }
                "minimum" | "maximum" => {
                    if !v.is_number() {
                        return Err(format!("{here}: must be a number"));
                    }
                }
                "minLength" | "maxLength" | "minItems" | "maxItems" => {
                    if v.as_u64().is_none() {
                        return Err(format!("{here}: must be a whole number, 0 or more"));
                    }
                }
                "uniqueItems" => {
                    if !v.is_boolean() {
                        return Err(format!("{here}: must be true or false"));
                    }
                }
                "$ref" => {
                    let r = v.as_str().ok_or(format!("{here}: must be a string"))?;
                    if self.resolve(r).is_none() {
                        return Err(format!("{here}: {r} does not lead anywhere"));
                    }
                }
                _ => return Err(format!("{here}: a schema keyword this reader does not know")),
            }
        }
        Ok(())
    }

    pub fn violations(&self, doc: &Value) -> Vec<Violation> {
        let mut out = Vec::new();
        self.check(&self.root, doc, "", &mut out);
        out
    }

    fn passes(&self, schema: &Value, doc: &Value) -> bool {
        let mut scratch = Vec::new();
        self.check(schema, doc, "", &mut scratch);
        scratch.is_empty()
    }

    fn check(&self, schema: &Value, doc: &Value, path: &str, out: &mut Vec<Violation>) {
        let mut fail = |message: String| out.push(Violation { path: path.to_string(), message });
        let s = match schema {
            Value::Bool(true) => return,
            Value::Bool(false) => return fail("nothing is allowed here".into()),
            Value::Object(o) => o,
            _ => return fail("the schema is not readable here".into()),
        };
        // draft-07: next to $ref, every other keyword is ignored
        if let Some(r) = s.get("$ref").and_then(Value::as_str) {
            match self.resolve(r) {
                Some(target) => self.check(target, doc, path, out),
                None => out.push(Violation { path: path.to_string(), message: format!("{r} does not lead anywhere") }),
            }
            return;
        }

        if let Some(t) = s.get("type") {
            let names: Vec<&str> = match t {
                Value::Array(l) => l.iter().filter_map(Value::as_str).collect(),
                _ => t.as_str().into_iter().collect(),
            };
            if !names.iter().any(|n| is_type(doc, n)) {
                fail(format!("{} is not of type {}", brief(doc), names.join(" or ")));
            }
        }
        if let Some(c) = s.get("const") {
            if !json_eq(c, doc) {
                fail(format!("{} was expected, found {}", brief(c), brief(doc)));
            }
        }
        if let Some(list) = s.get("enum").and_then(Value::as_array) {
            if !list.iter().any(|c| json_eq(c, doc)) {
                fail(format!("{} is not one of the allowed values", brief(doc)));
            }
        }

        if let Some(n) = doc.as_f64().filter(|_| doc.is_number()) {
            if let Some(min) = s.get("minimum").and_then(Value::as_f64) {
                if n < min {
                    fail(format!("{} is less than the minimum of {min}", brief(doc)));
                }
            }
            if let Some(max) = s.get("maximum").and_then(Value::as_f64) {
                if n > max {
                    fail(format!("{} is more than the maximum of {max}", brief(doc)));
                }
            }
        }

        if let Some(text) = doc.as_str() {
            let len = text.chars().count() as u64;
            if let Some(min) = s.get("minLength").and_then(Value::as_u64) {
                if len < min {
                    fail(format!("{} is shorter than {min}", brief(doc)));
                }
            }
            if let Some(max) = s.get("maxLength").and_then(Value::as_u64) {
                if len > max {
                    fail(format!("{} is longer than {max}", brief(doc)));
                }
            }
            if let Some(p) = s.get("pattern").and_then(Value::as_str) {
                // search, not full match: the patterns carry their own anchors
                let hit = self.patterns.get(p).is_some_and(|re| re.find(text).is_some());
                if !hit {
                    fail(format!("{} does not match {p}", brief(doc)));
                }
            }
            if s.get("format").and_then(Value::as_str) == Some("date-time") && !is_date_time(text) {
                fail(format!("{} is not a date-time", brief(doc)));
            }
        }

        if let Some(items) = doc.as_array() {
            if let Some(min) = s.get("minItems").and_then(Value::as_u64) {
                if (items.len() as u64) < min {
                    fail(format!("the list has {} items, fewer than {min}", items.len()));
                }
            }
            if let Some(max) = s.get("maxItems").and_then(Value::as_u64) {
                if (items.len() as u64) > max {
                    fail(format!("the list has {} items, more than {max}", items.len()));
                }
            }
            if s.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
                let repeat = items.iter().enumerate().any(|(i, a)| items[..i].iter().any(|b| json_eq(a, b)));
                if repeat {
                    fail("the list repeats an item".into());
                }
            }
            if let Some(c) = s.get("contains") {
                if !items.iter().any(|i| self.passes(c, i)) {
                    fail("no item in the list is the required one".into());
                }
            }
            match s.get("items") {
                Some(Value::Array(each)) => {
                    for (i, item) in items.iter().enumerate() {
                        let sub = match each.get(i) {
                            Some(sub) => Some(sub),
                            None => s.get("additionalItems"),
                        };
                        if let Some(sub) = sub {
                            self.check(sub, item, &format!("{path}/{i}"), out);
                        }
                    }
                }
                Some(sub) => {
                    for (i, item) in items.iter().enumerate() {
                        self.check(sub, item, &format!("{path}/{i}"), out);
                    }
                }
                None => {}
            }
        }

        if let Some(map) = doc.as_object() {
            if let Some(req) = s.get("required").and_then(Value::as_array) {
                for name in req.iter().filter_map(Value::as_str) {
                    if !map.contains_key(name) {
                        out.push(Violation { path: path.to_string(), message: format!("'{name}' is required") });
                    }
                }
            }
            let props = s.get("properties").and_then(Value::as_object);
            for (name, value) in map {
                let here = format!("{path}/{}", name.replace('~', "~0").replace('/', "~1"));
                match props.and_then(|p| p.get(name)) {
                    Some(sub) => self.check(sub, value, &here, out),
                    None => match s.get("additionalProperties") {
                        Some(Value::Bool(false)) => {
                            out.push(Violation { path: path.to_string(), message: format!("'{name}' is not a known field") })
                        }
                        Some(sub) => self.check(sub, value, &here, out),
                        None => {}
                    },
                }
            }
        }

        if let Some(all) = s.get("allOf").and_then(Value::as_array) {
            for sub in all {
                self.check(sub, doc, path, out);
            }
        }
        if let Some(any) = s.get("anyOf").and_then(Value::as_array) {
            if !any.iter().any(|sub| self.passes(sub, doc)) {
                out.push(Violation { path: path.to_string(), message: "none of the allowed shapes fits".into() });
            }
        }
        if let Some(one) = s.get("oneOf").and_then(Value::as_array) {
            let fits = one.iter().filter(|sub| self.passes(sub, doc)).count();
            if fits != 1 {
                out.push(Violation { path: path.to_string(), message: format!("exactly one of the allowed shapes must fit; {fits} do") });
            }
        }
        if let Some(not) = s.get("not") {
            if self.passes(not, doc) {
                out.push(Violation { path: path.to_string(), message: "this is a shape the contract forbids".into() });
            }
        }
        if let Some(cond) = s.get("if") {
            let branch = if self.passes(cond, doc) { s.get("then") } else { s.get("else") };
            if let Some(sub) = branch {
                self.check(sub, doc, path, out);
            }
        }
    }
}

fn brief(v: &Value) -> String {
    let t = v.to_string();
    if t.chars().count() > 60 { format!("{}...", t.chars().take(57).collect::<String>()) } else { t }
}

fn is_whole(v: &Value) -> bool {
    v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0)
}

fn is_type(v: &Value, name: &str) -> bool {
    match name {
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_number() && is_whole(v),
        "boolean" => v.is_boolean(),
        "object" => v.is_object(),
        "array" => v.is_array(),
        "null" => v.is_null(),
        _ => false,
    }
}

/// JSON equality: 1 equals 1.0, true never equals 1, key order is ignored.
fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if let (Some(i), Some(j)) = (x.as_i64(), y.as_i64()) {
                return i == j;
            }
            if let (Some(i), Some(j)) = (x.as_u64(), y.as_u64()) {
                return i == j;
            }
            x.as_f64() == y.as_f64()
        }
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_eq(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| json_eq(p, q)))
        }
        _ => a == b,
    }
}

/// RFC 3339 `date-time`: 2026-10-04T12:00:00Z, with optional fraction and
/// a `Z` or `+hh:mm` offset. Checked by hand so the calendar is checked too.
pub(crate) fn is_date_time(text: &str) -> bool {
    let b = text.as_bytes();
    let num = |from: usize, len: usize| -> Option<u32> {
        let part = b.get(from..from + len)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    let lit = |at: usize, c: &[u8]| b.get(at).is_some_and(|x| c.contains(x));
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (num(0, 4), num(5, 2), num(8, 2), num(11, 2), num(14, 2), num(17, 2)) else {
        return false;
    };
    if !(lit(4, b"-") && lit(7, b"-") && lit(10, b"Tt") && lit(13, b":") && lit(16, b":")) {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if leap { 29 } else { 28 },
        _ => return false,
    };
    if d < 1 || d > days || h > 23 || mi > 59 || s > 60 {
        return false;
    }
    let mut at = 19;
    if lit(at, b".") {
        at += 1;
        let start = at;
        while b.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return false;
        }
    }
    if lit(at, b"Zz") {
        return at + 1 == b.len();
    }
    if lit(at, b"+-") {
        return matches!((num(at + 1, 2), num(at + 4, 2)), (Some(oh), Some(om)) if oh < 24 && om < 60) && lit(at + 3, b":") && at + 6 == b.len();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_time_is_checked_for_real() {
        for ok in ["2026-10-04T12:00:00Z", "2024-02-29T23:59:60Z", "2026-10-04t12:00:00.250+02:00"] {
            assert!(is_date_time(ok), "{ok}");
        }
        for bad in ["", "nope", "2026-10-04", "2026-10-04 12:00:00Z", "2026-13-01T00:00:00Z", "2025-02-29T00:00:00Z", "2026-10-04T24:00:00Z", "2026-10-04T12:00:00", "2026-10-04T12:00:00.Z", "2026-10-04T12:00:00Zx", "2026-10-04T12:00:00+2:00"] {
            assert!(!is_date_time(bad), "{bad}");
        }
    }

    #[test]
    fn an_unknown_keyword_refuses_the_schema() {
        let s = r##"{"$schema":"http://json-schema.org/draft-07/schema#","type":"object","propertyNames":{"pattern":"^a"}}"##;
        assert!(Contract::new(s).err().is_some_and(|e| e.contains("does not know")));
    }

    #[test]
    fn an_unknown_draft_a_dangling_ref_and_an_unknown_format_refuse_the_schema() {
        assert!(Contract::new(r##"{"$schema":"https://json-schema.org/draft/2020-12/schema"}"##).is_err());
        assert!(Contract::new(r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/missing"}"##).is_err());
        assert!(Contract::new(r##"{"$schema":"http://json-schema.org/draft-07/schema#","format":"email"}"##).is_err());
    }

    #[test]
    fn numbers_compare_as_json_not_as_text() {
        let c = Contract::new(r##"{"$schema":"http://json-schema.org/draft-07/schema#","properties":{"a":{"const":1},"b":{"type":"integer"},"c":{"enum":[true]}}}"##).unwrap();
        assert!(c.violations(&serde_json::json!({"a": 1.0, "b": 2.0, "c": true})).is_empty());
        assert_eq!(c.violations(&serde_json::json!({"a": true, "b": 2.5, "c": 1})).len(), 3);
    }
}
