use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Result of AI evaluation for a single listing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIEvaluation {
    pub relevant: bool,
    pub reason: String,
    pub notable: Option<String>,
    pub escalate: bool,
}

impl AIEvaluation {
    /// Safe fallback: never drop a listing due to model error.
    pub fn passthrough() -> Self {
        Self {
            relevant: true,
            reason: String::new(),
            notable: None,
            escalate: false,
        }
    }
}

/// AI subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_litellm_base_url")]
    pub litellm_base_url: String,
    #[serde(default = "default_filter_model")]
    pub filter_model: String,
    #[serde(default = "default_filter_timeout")]
    pub filter_timeout_sec: f64,
    #[serde(default = "default_escalation_model")]
    pub escalation_model: String,
    #[serde(default)]
    pub escalation_enabled: bool,
    #[serde(default = "default_escalation_min_keyword_score")]
    pub escalation_min_keyword_score: f64,
    #[serde(default = "default_escalation_timeout")]
    pub escalation_timeout_sec: f64,
    #[serde(default)]
    pub anthropic_api_key: String,
    #[serde(default = "default_api_key")]
    pub api_key: String,
}

fn default_litellm_base_url() -> String {
    "http://localhost:11434/v1".to_string()
}
fn default_filter_model() -> String {
    "qwen3.5:9b".to_string()
}
fn default_filter_timeout() -> f64 {
    // Local models chew through 10-listing batches; 30s starves a batch
    // queued behind Ollama's serial request handling.
    120.0
}
fn default_escalation_model() -> String {
    "claude-haiku-4-5-20251001".to_string()
}
fn default_escalation_min_keyword_score() -> f64 {
    70.0
}
fn default_escalation_timeout() -> f64 {
    30.0
}
fn default_api_key() -> String {
    "noop".to_string()
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            litellm_base_url: default_litellm_base_url(),
            filter_model: default_filter_model(),
            filter_timeout_sec: default_filter_timeout(),
            escalation_model: default_escalation_model(),
            escalation_enabled: false,
            escalation_min_keyword_score: default_escalation_min_keyword_score(),
            escalation_timeout_sec: default_escalation_timeout(),
            anthropic_api_key: String::new(),
            api_key: default_api_key(),
        }
    }
}

impl AIConfig {
    /// Resolve API keys. Secrets come exclusively from SOPS: either the
    /// ANTHROPIC_API_KEY environment variable (as injected by
    /// `sops exec-env`) or `~/.config/scavenger/secrets.sops.yaml`
    /// decrypted via the `sops` binary. Plaintext key files are refused —
    /// a legacy `~/.config/scavenger/.env` is ignored with a loud warning.
    /// Call after deserialization.
    pub fn resolve_env_keys(&mut self) {
        if !self.anthropic_api_key.is_empty() {
            log::warn!(
                "anthropic_api_key is set in plaintext in config.toml — move it to \
                 ~/.config/scavenger/secrets.sops.yaml (sops-encrypted); plaintext \
                 config values will stop being honored in a future version"
            );
        }
        if self.anthropic_api_key.is_empty() {
            if let Ok(val) = std::env::var("ANTHROPIC_API_KEY") {
                self.anthropic_api_key = val;
            }
        }
        if self.anthropic_api_key.is_empty() {
            if let Some(val) = sops_extract_key("anthropic_api_key") {
                self.anthropic_api_key = val;
            }
        }
        if let Some(home) = dirs_path() {
            let legacy = home.join(".config/scavenger/.env");
            if legacy.exists() {
                log::warn!(
                    "plaintext {} is IGNORED — secrets are sops-only now; move the key \
                     into ~/.config/scavenger/secrets.sops.yaml (`sops edit` it) and \
                     delete the .env file",
                    legacy.display()
                );
            }
        }
    }
}

/// Decrypt a single key from the sops-encrypted secrets file by shelling
/// out to the user's `sops` binary, so their real key infrastructure
/// (age, PGP, KMS, keyservices) is honored. Returns None if the file is
/// absent, sops is missing, decryption fails, or the value is empty —
/// callers surface the missing key through AI health, never silently.
fn sops_extract_key(key: &str) -> Option<String> {
    // SCAVENGER_SECRETS_FILE overrides the default location — used by
    // deployments with a different secrets layout, and by tests to avoid
    // touching the real home directory.
    let secrets = match std::env::var("SCAVENGER_SECRETS_FILE") {
        Ok(p) => PathBuf::from(p),
        Err(_) => dirs_path()?.join(".config/scavenger/secrets.sops.yaml"),
    };
    if !secrets.exists() {
        return None;
    }
    let out = std::process::Command::new("sops")
        .arg("decrypt")
        .arg("--extract")
        .arg(format!("[\"{key}\"]"))
        .arg(&secrets)
        .output();
    match out {
        Ok(out) if out.status.success() => {
            let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if val.is_empty() { None } else { Some(val) }
        }
        Ok(out) => {
            log::warn!(
                "sops failed to decrypt {} ({}): {}",
                secrets.display(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
            None
        }
        Err(e) => {
            log::warn!("sops binary not runnable ({e}) — cannot read {}", secrets.display());
            None
        }
    }
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
