use chrono::Utc;
use scavenger::ai::evaluator::{extract_json, match_escalation_keywords, NoopEvaluator, BATCH_SIZE};
use scavenger::ai::models::AIEvaluation;
use scavenger::models::{Listing, Profile};

fn make_profile() -> Profile {
    Profile {
        id: "prof-1".into(),
        name: "Vintage Synths".into(),
        keywords: vec![],
        negative_keywords: vec![],
        sources: vec!["ebay".into()],
        escalation_keywords: vec!["moog".into(), "minimoog".into(), "model d".into()],
        ..Default::default()
    }
}

fn make_listing(id: &str, title: &str, description: &str) -> Listing {
    let now = Utc::now();
    Listing {
        id: id.into(),
        profile_id: "prof-1".into(),
        source_id: "ebay".into(),
        title: title.into(),
        description: description.into(),
        price: Some(100.0),
        currency: "USD".into(),
        condition: None,
        url: format!("https://example.com/{id}"),
        image_urls: vec![],
        location: None,
        first_seen: now,
        last_seen: now,
        relevance_score: 80.0,
        status: "new".into(),
        ai_evaluation: None,
    }
}

#[test]
fn test_match_escalation_keywords_case_insensitive() {
    let keywords = vec!["Moog".into(), "Minimoog".into(), "Model D".into()];
    let matched = match_escalation_keywords(
        &keywords,
        "Vintage MINIMOOG synthesizer",
        "Original model d from 1973",
    );
    assert_eq!(matched.len(), 2);
    assert!(matched.contains(&"Minimoog".to_string()));
    assert!(matched.contains(&"Model D".to_string()));
}

#[test]
fn test_match_escalation_keywords_no_match() {
    let keywords = vec!["Moog".into(), "Minimoog".into()];
    let matched = match_escalation_keywords(&keywords, "Roland Juno 106", "Analog polysynth");
    assert!(matched.is_empty());
}

#[test]
fn test_match_escalation_keywords_partial_match() {
    let keywords = vec!["rare".into(), "mint".into(), "NOS".into()];
    let matched = match_escalation_keywords(&keywords, "Rare Vintage Find", "In MINT condition");
    assert_eq!(matched.len(), 2);
    assert!(matched.contains(&"rare".to_string()));
    assert!(matched.contains(&"mint".to_string()));
}

#[test]
fn test_batch_splitting_25_listings() {
    let listings: Vec<Listing> = (0..25)
        .map(|i| make_listing(&format!("id-{i}"), &format!("Item {i}"), "desc"))
        .collect();

    let chunks: Vec<&[Listing]> = listings.chunks(BATCH_SIZE).collect();
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].len(), 10);
    assert_eq!(chunks[1].len(), 10);
    assert_eq!(chunks[2].len(), 5);
}

#[tokio::test]
async fn test_noop_evaluator_returns_passthrough() {
    let evaluator = NoopEvaluator;
    let profile = make_profile();
    let listing = make_listing("noop-1", "Test Item", "A description");

    let result = evaluator.evaluate(&profile, &listing).await;
    assert!(result.relevant);
    assert!(result.reason.is_empty());
    assert!(result.notable.is_none());
    assert!(!result.escalate);
}

#[tokio::test]
async fn test_noop_evaluator_batch() {
    let evaluator = NoopEvaluator;
    let profile = make_profile();
    let listings: Vec<Listing> = (0..5)
        .map(|i| make_listing(&format!("batch-{i}"), &format!("Item {i}"), "desc"))
        .collect();

    let results = evaluator.evaluate_batch(&profile, &listings).await;
    assert_eq!(results.len(), 5);
    for listing in &listings {
        let ev = results.get(&listing.id).expect("missing listing in results");
        assert!(ev.relevant);
        assert!(!ev.escalate);
    }
}

#[test]
fn test_extract_json_plain() {
    let input = r#"{"relevant": true, "reason": "looks good"}"#;
    assert_eq!(extract_json(input), input);
}

#[test]
fn test_extract_json_markdown_fences() {
    let input = "```json\n{\"relevant\": false}\n```";
    assert_eq!(extract_json(input), "{\"relevant\": false}");
}

#[test]
fn test_extract_json_surrounding_text() {
    let input = "Here is my analysis: {\"a\": 1} done";
    assert_eq!(extract_json(input), "{\"a\": 1}");
}

#[test]
fn test_extract_json_nested_braces() {
    let input = r#"{"outer": {"inner": "value"}, "b": 2}"#;
    assert_eq!(extract_json(input), input);
}

#[test]
fn test_filter_response_parsing() {
    let json_str = r#"{"relevant": true, "reason": "Matches profile keywords", "notable": "Rare variant", "escalate": false}"#;
    let eval: AIEvaluation = serde_json::from_str(json_str).unwrap();
    assert!(eval.relevant);
    assert_eq!(eval.reason, "Matches profile keywords");
    assert_eq!(eval.notable.as_deref(), Some("Rare variant"));
    assert!(!eval.escalate);
}

#[test]
fn test_filter_response_with_null_notable() {
    let json_str = r#"{"relevant": false, "reason": "No match", "notable": null, "escalate": false}"#;
    let eval: AIEvaluation = serde_json::from_str(json_str).unwrap();
    assert!(!eval.relevant);
    assert!(eval.notable.is_none());
}

#[test]
fn test_batch_response_array_parsing() {
    let json_str = r#"[
        {"id": "a1", "relevant": true, "reason": "good", "notable": null, "escalate": false},
        {"id": "a2", "relevant": false, "reason": "bad", "notable": null, "escalate": false}
    ]"#;
    let items: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap();
    assert_eq!(items.len(), 2);
}

#[test]
fn test_batch_response_wrapper_parsing() {
    let json_str = r#"{"results": [
        {"id": "b1", "relevant": true, "reason": "match", "notable": null, "escalate": false}
    ]}"#;
    let val: serde_json::Value = serde_json::from_str(json_str).unwrap();
    let obj = val.as_object().unwrap();
    let results = obj.get("results").unwrap().as_array().unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_passthrough_defaults() {
    let pt = AIEvaluation::passthrough();
    assert!(pt.relevant);
    assert!(pt.reason.is_empty());
    assert!(pt.notable.is_none());
    assert!(!pt.escalate);
}
