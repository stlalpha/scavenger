use scavenger::ai::models::{AIConfig, AIEvaluation};

#[test]
fn passthrough_values() {
    let eval = AIEvaluation::passthrough();
    assert!(eval.relevant);
    assert_eq!(eval.reason, "");
    assert_eq!(eval.notable, None);
    assert!(!eval.escalate);
}

#[test]
fn serde_round_trip() {
    let eval = AIEvaluation {
        relevant: false,
        reason: "not matching profile".to_string(),
        notable: Some("rare variant".to_string()),
        escalate: true,
        model: "qwen3.5:35b".to_string(),
    };
    let json = serde_json::to_string(&eval).unwrap();
    let deserialized: AIEvaluation = serde_json::from_str(&json).unwrap();
    assert_eq!(eval, deserialized);
    // Rows written before the model field still parse (defaults to empty).
    let legacy = r#"{"relevant":true,"reason":"x","notable":null,"escalate":false}"#;
    let parsed: AIEvaluation = serde_json::from_str(legacy).unwrap();
    assert_eq!(parsed.model, "");
}

#[test]
fn serde_round_trip_null_notable() {
    let eval = AIEvaluation::passthrough();
    let json = serde_json::to_string(&eval).unwrap();
    assert!(json.contains("\"notable\":null"));
    let deserialized: AIEvaluation = serde_json::from_str(&json).unwrap();
    assert_eq!(eval, deserialized);
}

#[test]
fn ai_config_defaults() {
    let config = AIConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.litellm_base_url, "http://localhost:11434/v1");
    assert_eq!(config.filter_model, "qwen3.5:9b");
    assert_eq!(config.filter_timeout_sec, 120.0);
    assert_eq!(config.escalation_model, "claude-haiku-4-5-20251001");
    assert!(!config.escalation_enabled);
    assert_eq!(config.escalation_min_keyword_score, 70.0);
    assert_eq!(config.escalation_timeout_sec, 30.0);
    assert_eq!(config.anthropic_api_key, "");
    assert_eq!(config.api_key, "noop");
}

#[test]
fn ai_config_deserialize_with_defaults() {
    let json = r#"{"enabled": true}"#;
    let config: AIConfig = serde_json::from_str(json).unwrap();
    assert!(config.enabled);
    assert_eq!(config.filter_model, "qwen3.5:9b");
    assert_eq!(config.api_key, "noop");
}
