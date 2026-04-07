from scavenger.util import extract_json

def test_extract_plain_json():
    assert extract_json('{"a": 1}') == '{"a": 1}'

def test_extract_from_fenced_block():
    text = '```json\n{"a": 1}\n```'
    assert extract_json(text) == '{"a": 1}'

def test_extract_with_trailing_text():
    text = 'Here is the result: {"a": 1} hope that helps!'
    assert extract_json(text) == '{"a": 1}'

def test_extract_nested_braces():
    text = '{"a": {"b": 1}}'
    assert extract_json(text) == '{"a": {"b": 1}}'

def test_extract_no_json_returns_input():
    assert extract_json("no json here") == "no json here"

def test_extract_with_string_containing_braces():
    text = '{"msg": "use {x} for templating"}'
    assert extract_json(text) == '{"msg": "use {x} for templating"}'
