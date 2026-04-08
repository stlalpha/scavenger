use scavenger::util::extract_json;

#[test]
fn extract_raw_json() {
    let input = r#"{"key": "value", "num": 42}"#;
    assert_eq!(extract_json(input), input);
}

#[test]
fn extract_fenced_json() {
    let input = "```json\n{\"key\": \"value\"}\n```";
    assert_eq!(extract_json(input), "{\"key\": \"value\"}");
}

#[test]
fn extract_fenced_no_language_tag() {
    let input = "```\n{\"a\": 1}\n```";
    assert_eq!(extract_json(input), "{\"a\": 1}");
}

#[test]
fn extract_json_with_trailing_text() {
    let input = "Here is the result: {\"a\": 1} and some more text after";
    assert_eq!(extract_json(input), "{\"a\": 1}");
}

#[test]
fn extract_nested_objects() {
    let input = r#"{"outer": {"inner": {"deep": true}}, "b": 2}"#;
    assert_eq!(extract_json(input), input);
}

#[test]
fn handles_escaped_quotes() {
    let input = r#"{"key": "val\"ue"}"#;
    assert_eq!(extract_json(input), input);
}

#[test]
fn handles_braces_inside_strings() {
    let input = r#"{"key": "value with { and } inside"}"#;
    assert_eq!(extract_json(input), input);
}

#[test]
fn no_json_returns_original() {
    let input = "just plain text";
    assert_eq!(extract_json(input), input);
}

#[test]
fn fenced_with_leading_text() {
    let input = "The AI responded:\n```json\n{\"score\": 85}\n```\nDone.";
    // The trailing ``` fence stripping only works when it's at the end;
    // here the { finder + depth tracking handles it.
    let result = extract_json(input);
    assert_eq!(result, "{\"score\": 85}");
}
