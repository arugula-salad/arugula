//! A small JSON Schema checker for tests (#580): does this JSON fit this
//! schema? It reads what schemars writes for the MCP tools' `outputSchema`
//! (`type`, `properties`, `required`, `items`, `enum`, `const`, `$ref` into
//! `$defs`, `anyOf`/`oneOf`/`allOf`), which is all the daemon's results use.
//! A key outside the schema's `properties` is a failure, as is a missing
//! required one, since a client may validate `structuredContent` against the
//! schema and refuse an answer that doesn't fit.

use serde_json::{Map, Value};

/// `Ok` when `value` fits `schema` (the root, with its `$defs`), else why not.
pub fn conforms(value: &Value, schema: &Value) -> Result<(), String> {
    check(value, schema, schema, false, "$")
}

/// `inside`: this schema is a branch or member of another that already
/// checks the object's keys against all of them.
fn check(v: &Value, s: &Value, root: &Value, inside: bool, at: &str) -> Result<(), String> {
    let Some(o) = s.as_object() else {
        return match s {
            Value::Bool(false) => Err(format!("{at}: the schema allows nothing here")),
            _ => Ok(()),
        };
    };
    if let Some(r) = o.get("$ref").and_then(Value::as_str) {
        let target =
            r.strip_prefix('#').and_then(|p| root.pointer(p)).ok_or_else(|| format!("{at}: can't follow {r}"))?;
        return check(v, target, root, inside, at);
    }
    if let Some(c) = o.get("const")
        && c != v
    {
        return Err(format!("{at}: {v} isn't {c}"));
    }
    if let Some(e) = o.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        return Err(format!("{at}: {v} isn't one of {e:?}"));
    }
    if let Some(t) = o.get("type") {
        let types: Vec<&str> = match t {
            Value::String(t) => vec![t.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !types.is_empty() && !types.iter().any(|t| is_type(v, t)) {
            return Err(format!("{at}: {v} isn't {}", types.join(" or ")));
        }
    }
    for (i, sub) in o.get("allOf").and_then(Value::as_array).into_iter().flatten().enumerate() {
        check(v, sub, root, true, at).map_err(|e| format!("{e} (allOf {i})"))?;
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(subs) = o.get(key).and_then(Value::as_array) {
            let fails: Vec<String> = subs.iter().filter_map(|sub| check(v, sub, root, true, at).err()).collect();
            if fails.len() == subs.len() {
                return Err(format!("{at}: fits none of {key}: {}", fails.join("; ")));
            }
        }
    }
    match v {
        Value::Object(m) => {
            for r in o.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if !m.contains_key(r) {
                    return Err(format!("{at}: missing required {r}"));
                }
            }
            let props = o.get("properties").and_then(Value::as_object);
            for (k, x) in m {
                match props.and_then(|p| p.get(k)) {
                    Some(sub) => check(x, sub, root, false, &format!("{at}.{k}"))?,
                    None if inside || known(o, root, k) => {}
                    None => match o.get("additionalProperties") {
                        None | Some(Value::Bool(false)) => return Err(format!("{at}: {k} isn't in the schema")),
                        Some(extra) => check(x, extra, root, false, &format!("{at}.{k}"))?,
                    },
                }
            }
        }
        Value::Array(items) => {
            if let Some(sub) = o.get("items") {
                for (i, x) in items.iter().enumerate() {
                    check(x, sub, root, false, &format!("{at}[{i}]"))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Whether `key` is a property of this schema or of one it combines.
fn known(o: &Map<String, Value>, root: &Value, key: &str) -> bool {
    if o.get("properties").and_then(Value::as_object).is_some_and(|p| p.contains_key(key)) {
        return true;
    }
    if let Some(r) = o.get("$ref").and_then(Value::as_str)
        && let Some(t) = r.strip_prefix('#').and_then(|p| root.pointer(p)).and_then(Value::as_object)
        && known(t, root, key)
    {
        return true;
    }
    ["allOf", "anyOf", "oneOf"]
        .iter()
        .filter_map(|k| o.get(*k).and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_object)
        .any(|sub| known(sub, root, key))
}

fn is_type(v: &Value, t: &str) -> bool {
    match t {
        "null" => v.is_null(),
        "boolean" => v.is_boolean(),
        "string" => v.is_string(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn it_refuses_what_a_client_would() {
        let s = json!({
            "type": "object",
            "properties": { "pane": { "type": "integer" }, "note": { "type": ["string", "null"] } },
            "required": ["pane"]
        });
        assert!(conforms(&json!({ "pane": 3, "note": null }), &s).is_ok());
        assert!(conforms(&json!({ "pane": 3 }), &s).is_ok());
        assert!(conforms(&json!({ "note": "x" }), &s).is_err(), "a required key");
        assert!(conforms(&json!({ "pane": "3" }), &s).is_err(), "a wrong type");
        assert!(conforms(&json!({ "pane": 3, "other": 1 }), &s).is_err(), "a key the schema doesn't have");
    }

    #[test]
    fn it_follows_refs_and_branches() {
        let s = json!({
            "type": "object",
            "properties": { "summary": { "type": "string" } },
            "required": ["summary"],
            "oneOf": [
                { "type": "object", "properties": { "state": { "const": "a" }, "x": { "$ref": "#/$defs/N" } }, "required": ["state"] },
                { "type": "object", "properties": { "state": { "const": "b" } }, "required": ["state"] }
            ],
            "$defs": { "N": { "type": "integer" } }
        });
        assert!(conforms(&json!({ "summary": "s", "state": "a", "x": 1 }), &s).is_ok());
        assert!(conforms(&json!({ "summary": "s", "state": "b" }), &s).is_ok());
        assert!(conforms(&json!({ "summary": "s", "state": "c" }), &s).is_err(), "no branch fits");
        assert!(conforms(&json!({ "summary": "s", "state": "a", "x": "1" }), &s).is_err(), "a ref'd type");
        assert!(conforms(&json!({ "summary": "s", "state": "b", "y": 1 }), &s).is_err(), "no branch knows y");
    }
}
