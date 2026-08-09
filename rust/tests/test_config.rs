use std::path::PathBuf;

use scavenger::config;
use scavenger::error::ScavengerError;
use scavenger::models::{AlertPriority, KeywordGroup};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn test_load_valid_config() {
    let cfg = config::load_config(&fixture("valid_config.toml")).unwrap();

    assert_eq!(cfg.global_config.log_level, "DEBUG");
    assert_eq!(cfg.global_config.home_zip.as_deref(), Some("90210"));
    assert_eq!(cfg.global_config.tui_refresh_sec, 3.0);
    assert_eq!(cfg.profiles.len(), 2);

    let guitars = &cfg.profiles[0];
    assert_eq!(guitars.id, "guitars");
    assert_eq!(guitars.name, "Vintage Guitars");
    assert_eq!(guitars.price_min, Some(100.0));
    assert_eq!(guitars.price_max, Some(5000.0));
    assert_eq!(guitars.poll_interval_sec, 1800);
    assert!(matches!(guitars.alert_priority, AlertPriority::High));
    assert_eq!(guitars.negative_keywords, vec!["broken", "parts only"]);
    assert_eq!(guitars.tags, vec!["instruments", "vintage"]);
    assert_eq!(guitars.escalation_keywords, vec!["rare", "mint"]);
    assert_eq!(guitars.location_radius_mi, Some(50));

    // Check mixed keywords: ["guitar", ["fender", "gibson"], "vintage"]
    assert_eq!(guitars.keywords.len(), 3);
    assert_eq!(guitars.keywords[0], KeywordGroup::Single("guitar".into()));
    assert_eq!(
        guitars.keywords[1],
        KeywordGroup::Any(vec!["fender".into(), "gibson".into()])
    );
    assert_eq!(guitars.keywords[2], KeywordGroup::Single("vintage".into()));
}

#[test]
fn test_load_minimal_config_defaults() {
    let cfg = config::load_config(&fixture("minimal_config.toml")).unwrap();

    // Global defaults
    assert_eq!(
        cfg.global_config.db_path,
        "~/.local/share/scavenger/scavenger.db"
    );
    assert_eq!(cfg.global_config.log_level, "INFO");
    assert_eq!(cfg.global_config.tui_refresh_sec, 2.0);
    assert!(cfg.global_config.home_zip.is_none());

    // Profile defaults
    let p = &cfg.profiles[0];
    assert_eq!(p.poll_interval_sec, 3600);
    assert!(matches!(p.alert_priority, AlertPriority::Normal));
    assert!(p.enabled);
    assert!(p.negative_keywords.is_empty());
    assert!(p.tags.is_empty());
}

#[test]
fn test_load_invalid_toml() {
    let result = config::load_config(&fixture("invalid.toml"));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ScavengerError::Config(_)));
    let msg = err.to_string();
    assert!(msg.contains("Invalid TOML"), "got: {msg}");
}

#[test]
fn test_load_missing_file() {
    let result = config::load_config(&fixture("does_not_exist.toml"));
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("not found") || msg.contains("Cannot read"), "got: {msg}");
}

#[test]
fn test_load_ai_config() {
    // Hermetic: never consult the developer's real sops secrets file.
    std::env::set_var("SCAVENGER_SECRETS_FILE", "/nonexistent/secrets.sops.yaml");
    let ai = config::load_ai_config(&fixture("valid_config.toml")).unwrap();
    assert!(ai.enabled);
    assert_eq!(ai.filter_model, "llama3:8b");
    assert_eq!(ai.filter_timeout_sec, 45.0);
    assert!(ai.escalation_enabled);
    assert_eq!(ai.escalation_min_keyword_score, 80.0);
}

#[test]
fn test_load_ai_config_missing_section() {
    // Hermetic: never consult the developer's real sops secrets file.
    std::env::set_var("SCAVENGER_SECRETS_FILE", "/nonexistent/secrets.sops.yaml");
    let ai = config::load_ai_config(&fixture("no_ai_config.toml")).unwrap();
    assert!(!ai.enabled);
    assert_eq!(ai.filter_model, "qwen3.5:9b");
}

#[test]
fn test_load_ai_config_missing_file() {
    // Hermetic: never consult the developer's real sops secrets file.
    std::env::set_var("SCAVENGER_SECRETS_FILE", "/nonexistent/secrets.sops.yaml");
    let ai = config::load_ai_config(&fixture("does_not_exist.toml")).unwrap();
    assert!(!ai.enabled);
}

#[test]
fn test_db_path_expands_tilde() {
    let cfg = config::load_config(&fixture("minimal_config.toml")).unwrap();
    let db = cfg.db_path();
    assert!(!db.to_str().unwrap().starts_with('~'));
    assert!(db.to_str().unwrap().ends_with("scavenger/scavenger.db"));
}

#[test]
fn test_append_profile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[global]\nlog_level = \"INFO\"\n\n[[profiles]]\nid = \"existing\"\nname = \"Existing\"\nkeywords = [\"stuff\"]\nsources = [\"ebay\"]\nenabled = true\n",
    )
    .unwrap();

    let new_profile = scavenger::models::Profile {
        id: "new-one".into(),
        name: "New Profile".into(),
        keywords: vec![
            KeywordGroup::Single("widget".into()),
            KeywordGroup::Any(vec!["red".into(), "blue".into()]),
        ],
        negative_keywords: vec![],
        sources: vec!["craigslist".into()],
        price_min: None,
        price_max: Some(500.0),
        poll_interval_sec: 3600,
        alert_priority: AlertPriority::Normal,
        enabled: true,
        tags: vec![],
        escalation_keywords: vec![],
        location_radius_mi: None,
    };

    let result = config::append_profile(&path, &new_profile).unwrap();
    assert_eq!(result.id, "new-one");

    // Re-read and verify
    let cfg = config::load_config(&path).unwrap();
    assert_eq!(cfg.profiles.len(), 2);
    assert_eq!(cfg.profiles[1].id, "new-one");
    assert_eq!(cfg.profiles[1].price_max, Some(500.0));

    // Verify original profile preserved
    assert_eq!(cfg.profiles[0].id, "existing");
}

#[test]
fn test_append_profile_duplicate_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[[profiles]]\nid = \"dup\"\nname = \"Dup\"\nkeywords = [\"x\"]\nsources = [\"ebay\"]\nenabled = true\n",
    )
    .unwrap();

    let profile = scavenger::models::Profile {
        id: "dup".into(),
        name: "Duplicate".into(),
        keywords: vec![KeywordGroup::Single("x".into())],
        negative_keywords: vec![],
        sources: vec!["ebay".into()],
        price_min: None,
        price_max: None,
        poll_interval_sec: 3600,
        alert_priority: AlertPriority::Normal,
        enabled: true,
        tags: vec![],
        escalation_keywords: vec![],
        location_radius_mi: None,
    };

    let result = config::append_profile(&path, &profile);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));
}

#[test]
fn test_delete_profile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[[profiles]]\nid = \"keep\"\nname = \"Keep\"\nkeywords = [\"a\"]\nsources = [\"ebay\"]\nenabled = true\n\n[[profiles]]\nid = \"remove\"\nname = \"Remove\"\nkeywords = [\"b\"]\nsources = [\"ebay\"]\nenabled = true\n",
    )
    .unwrap();

    config::delete_profile(&path, "remove").unwrap();

    let cfg = config::load_config(&path).unwrap();
    assert_eq!(cfg.profiles.len(), 1);
    assert_eq!(cfg.profiles[0].id, "keep");
}

#[test]
fn test_delete_profile_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[[profiles]]\nid = \"only\"\nname = \"Only\"\nkeywords = [\"x\"]\nsources = [\"ebay\"]\nenabled = true\n",
    )
    .unwrap();

    let result = config::delete_profile(&path, "nonexistent");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn test_update_profile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[[profiles]]\nid = \"up\"\nname = \"Original\"\nkeywords = [\"old\"]\nsources = [\"ebay\"]\nenabled = true\n",
    )
    .unwrap();

    let updated = scavenger::models::Profile {
        id: "up".into(),
        name: "Updated Name".into(),
        keywords: vec![KeywordGroup::Single("new".into())],
        negative_keywords: vec![],
        sources: vec!["ebay".into(), "craigslist".into()],
        price_min: Some(50.0),
        price_max: None,
        poll_interval_sec: 3600,
        alert_priority: AlertPriority::Normal,
        enabled: true,
        tags: vec![],
        escalation_keywords: vec![],
        location_radius_mi: None,
    };

    config::update_profile(&path, &updated).unwrap();

    let cfg = config::load_config(&path).unwrap();
    assert_eq!(cfg.profiles.len(), 1);
    assert_eq!(cfg.profiles[0].name, "Updated Name");
    assert_eq!(cfg.profiles[0].sources, vec!["ebay", "craigslist"]);
    assert_eq!(cfg.profiles[0].price_min, Some(50.0));
}

#[test]
fn test_update_profile_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "").unwrap();

    let profile = scavenger::models::Profile {
        id: "ghost".into(),
        name: "Ghost".into(),
        keywords: vec![KeywordGroup::Single("x".into())],
        negative_keywords: vec![],
        sources: vec!["ebay".into()],
        price_min: None,
        price_max: None,
        poll_interval_sec: 3600,
        alert_priority: AlertPriority::Normal,
        enabled: true,
        tags: vec![],
        escalation_keywords: vec![],
        location_radius_mi: None,
    };

    let result = config::update_profile(&path, &profile);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}
