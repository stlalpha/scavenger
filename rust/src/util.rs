/// Extract the first JSON object from text, ignoring markdown fences and trailing content.
pub fn extract_json(text: &str) -> String {
    let mut text = text.trim();

    // Strip leading markdown fence
    if text.starts_with("```") {
        if let Some(nl) = text.find('\n') {
            text = &text[nl + 1..];
        }
    }

    // Strip trailing markdown fence
    let trimmed = text.trim_end();
    if trimmed.ends_with("```") {
        if let Some(pos) = trimmed.rfind("```") {
            text = trimmed[..pos].trim();
        }
    }

    let text = text.trim();

    let start = match text.find('{') {
        Some(i) => i,
        None => return text.to_string(),
    };

    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut escape = false;

    for i in start..bytes.len() {
        let ch = bytes[i];
        if escape {
            escape = false;
            continue;
        }
        if ch == b'\\' {
            escape = true;
            continue;
        }
        if ch == b'"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if ch == b'{' {
            depth += 1;
        } else if ch == b'}' {
            depth -= 1;
            if depth == 0 {
                return text[start..=i].to_string();
            }
        }
    }

    text[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_raw_json() {
        let input = r#"{"key": "value", "num": 42}"#;
        let result = extract_json(input);
        assert_eq!(result, input);
    }

    #[test]
    fn extract_fenced_json() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        let result = extract_json(input);
        assert_eq!(result, "{\"key\": \"value\"}");
    }

    #[test]
    fn extract_json_with_trailing_text() {
        let input = "Here is the result: {\"a\": 1} and some more text";
        let result = extract_json(input);
        assert_eq!(result, "{\"a\": 1}");
    }

    #[test]
    fn extract_nested_objects() {
        let input = r#"{"outer": {"inner": {"deep": true}}, "b": 2}"#;
        let result = extract_json(input);
        assert_eq!(result, input);
    }

    #[test]
    fn handles_escaped_quotes_in_strings() {
        let input = r#"{"key": "val\"ue"}"#;
        let result = extract_json(input);
        assert_eq!(result, input);
    }

    #[test]
    fn handles_braces_in_strings() {
        let input = r#"{"key": "value with { and } inside"}"#;
        let result = extract_json(input);
        assert_eq!(result, input);
    }

    #[test]
    fn no_json_returns_text() {
        let input = "no json here";
        let result = extract_json(input);
        assert_eq!(result, input);
    }
}
