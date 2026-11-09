//! JSON state paths and `{path}` templates.

use serde_json::Value;

use super::GraphError;

/// Read `path` from a JSON object. An empty path returns `value` itself.
pub(crate) fn lookup<'a>(value: &'a Value, path: &str) -> Result<&'a Value, GraphError> {
    if path.is_empty() {
        return Ok(value);
    }
    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            return Err(GraphError::MissingKey(path.to_string()));
        }
        let Value::Object(map) = current else {
            return Err(GraphError::MissingKey(path.to_string()));
        };
        current = map
            .get(segment)
            .ok_or_else(|| GraphError::MissingKey(path.to_string()))?;
    }
    Ok(current)
}

/// Insert a string as text. Insert every other JSON value as compact JSON.
pub(crate) fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Fill `{path}` placeholders from `state`.
///
/// `{{` and `}}` are literal braces. A missing path fails the fill.
pub(crate) fn render_template(template: &str, state: &Value) -> Result<String, GraphError> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '{' && chars.get(index + 1) == Some(&'{') {
            out.push('{');
            index += 2;
            continue;
        }
        if chars[index] == '}' && chars.get(index + 1) == Some(&'}') {
            out.push('}');
            index += 2;
            continue;
        }
        if chars[index] == '{' {
            let rest = &chars[index + 1..];
            let Some(end) = rest.iter().position(|ch| *ch == '}') else {
                return Err(GraphError::BadTemplate);
            };
            let path: String = rest[..end].iter().collect();
            let value = lookup(state, path.trim())?;
            out.push_str(&render_value(value));
            index += end + 2;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    Ok(out)
}

/// A node id or write key used inside a template path.
pub(crate) fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && !key.contains(['.', '{', '}', ' '])
        && key.chars().all(|ch| ch.is_ascii_graphic())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn template_inserts_text_and_json() {
        let state = json!({"draft": "hello", "grade": {"pass": true}});
        let text = render_template("A {draft} B {grade.pass} C {{x}}", &state).unwrap();
        assert_eq!(text, "A hello B true C {x}");
    }

    #[test]
    fn missing_path_fails() {
        let error = render_template("{missing}", &json!({})).unwrap_err();
        assert!(matches!(error, GraphError::MissingKey(path) if path == "missing"));
    }
}
