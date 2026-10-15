//! A draft-07 subset validator for the vendored PGPatcher schemas (DECISIONS P12): exactly the keywords those schemas
//! use, `pbr.schema.json` and `complex_material.schema.json`. A test fails if either starts using another one.
//! Validation is a safety net that should never fire: the writers build documents from the same key tables.
use super::schema::SCHEMA_JSON;
use serde_json::Value;
use std::sync::OnceLock;

/// PGPatcher's complex material schema (upstream `0176567`, identical at 2.2.0), vendored byte for byte.
pub const CM_SCHEMA_JSON: &str = include_str!("../../assets/complex_material.schema.json");

pub const KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "additionalProperties",
    "required",
    "items",
    "minItems",
    "maxItems",
    "minimum",
    "maximum",
    "$ref",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
];

fn schema() -> &'static Value {
    static S: OnceLock<Value> = OnceLock::new();
    S.get_or_init(|| serde_json::from_str(SCHEMA_JSON).expect("the vendored schema parses"))
}

fn resolve<'a>(root: &'a Value, r: &str) -> Option<&'a Value> {
    let path = r.strip_prefix("#/")?;
    path.split('/').try_fold(root, |v, part| v.get(part))
}

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => false,
    }
}

/// The errors of the branch that fails least, so a failed anyOf/oneOf still names the path and key that went
/// wrong instead of only the summary. A branch that is the wrong type for the value (its first error is a type
/// error at this very path) ranks after every branch that is the right type; ties go to the first branch.
fn best_branch_errors(root: &Value, branches: &[Value], v: &Value, at: &str) -> Vec<String> {
    let wrong_type = format!("{at}: expected ");
    let mut best: Option<(bool, Vec<String>)> = None;
    for b in branches {
        let mut e = vec![];
        check(root, b, v, at, &mut e);
        let rank = (
            e.first().is_some_and(|x| x.starts_with(&wrong_type)),
            e.len(),
        );
        if best.as_ref().is_none_or(|(w, x)| rank < (*w, x.len())) {
            best = Some((rank.0, e));
        }
    }
    best.map(|(_, e)| e).unwrap_or_default()
}

fn check(root: &Value, s: &Value, v: &Value, at: &str, errs: &mut Vec<String>) {
    let Some(s) = s.as_object() else {
        if s == &Value::Bool(false) {
            errs.push(format!("{at}: not allowed"));
        }
        return;
    };
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        match resolve(root, r) {
            Some(t) => check(root, t, v, at, errs),
            None => errs.push(format!("{at}: unresolved {r}")),
        }
    }
    if let Some(t) = s.get("type") {
        let ok = match t {
            Value::String(t) => type_ok(t, v),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_ok(t, v)),
            _ => true,
        };
        if !ok {
            errs.push(format!("{at}: expected {t}"));
            return;
        }
    }
    if let Some(x) = v.as_f64() {
        // The alphaprop_* integer ranges (Task 4b).
        if s.get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|m| x < m)
        {
            errs.push(format!("{at}: below the minimum"));
        }
        if s.get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|m| x > m)
        {
            errs.push(format!("{at}: above the maximum"));
        }
    }
    if let (Some(req), Some(o)) = (s.get("required").and_then(Value::as_array), v.as_object()) {
        for r in req.iter().filter_map(Value::as_str) {
            if !o.contains_key(r) {
                errs.push(format!("{at}: missing '{r}'"));
            }
        }
    }
    if let Some(o) = v.as_object() {
        let props = s.get("properties").and_then(Value::as_object);
        for (k, x) in o {
            match props.and_then(|p| p.get(k)) {
                Some(ps) => check(root, ps, x, &format!("{at}.{k}"), errs),
                None => {
                    if s.get("additionalProperties") == Some(&Value::Bool(false)) {
                        errs.push(format!("{at}: unknown key '{k}'"));
                    }
                }
            }
        }
    }
    if let Some(a) = v.as_array() {
        if let Some(n) = s.get("minItems").and_then(Value::as_u64) {
            if (a.len() as u64) < n {
                errs.push(format!("{at}: needs at least {n} items"));
            }
        }
        if let Some(n) = s.get("maxItems").and_then(Value::as_u64) {
            if (a.len() as u64) > n {
                errs.push(format!("{at}: needs at most {n} items"));
            }
        }
        if let Some(items) = s.get("items") {
            for (i, x) in a.iter().enumerate() {
                check(root, items, x, &format!("{at}[{i}]"), errs);
            }
        }
    }
    let passes = |sub: &Value| {
        let mut e = vec![];
        check(root, sub, v, at, &mut e);
        e.is_empty()
    };
    if let Some(all) = s.get("allOf").and_then(Value::as_array) {
        for sub in all {
            check(root, sub, v, at, errs);
        }
    }
    if let Some(any) = s.get("anyOf").and_then(Value::as_array) {
        if !any.iter().any(passes) {
            errs.push(format!("{at}: matches none of anyOf"));
            errs.extend(best_branch_errors(root, any, v, at));
        }
    }
    if let Some(one) = s.get("oneOf").and_then(Value::as_array) {
        let n = one.iter().filter(|x| passes(x)).count();
        if n != 1 {
            errs.push(format!("{at}: matches {n} of oneOf, needs exactly 1"));
            if n == 0 {
                errs.extend(best_branch_errors(root, one, v, at));
            }
        }
    }
    if let Some(not) = s.get("not") {
        if passes(not) {
            errs.push(format!("{at}: must not match 'not'"));
        }
    }
    if let Some(cond) = s.get("if") {
        let branch = if passes(cond) {
            s.get("then")
        } else {
            s.get("else")
        };
        if let Some(b) = branch {
            check(root, b, v, at, errs);
        }
    }
}

fn cm_schema() -> &'static Value {
    static S: OnceLock<Value> = OnceLock::new();
    S.get_or_init(|| {
        serde_json::from_str(CM_SCHEMA_JSON).expect("the vendored complex material schema parses")
    })
}

fn run(root: &Value, doc: &Value) -> Result<(), Vec<String>> {
    let mut errs = vec![];
    check(root, root, doc, "$", &mut errs);
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

pub fn validate(doc: &Value) -> Result<(), Vec<String>> {
    run(schema(), doc)
}

/// A PGPatcher complex material JSON (`<texture>_m.json`) against its vendored schema.
pub fn validate_cm(doc: &Value) -> Result<(), Vec<String>> {
    run(cm_schema(), doc)
}
