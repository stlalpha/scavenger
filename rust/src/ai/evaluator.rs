use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use log::{info, warn};
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::ai::models::{AIConfig, AIEvaluation};
use crate::ai::prompts::{build_batch_prompt, build_escalation_prompt, build_prompt};
use crate::models::{Listing, Profile};

/// AI evaluation health, surfaced through daemon status so failures are
/// never silent: passthrough keeps listings flowing, but the operator must
/// be able to see that filtering is not happening and why.
#[derive(Clone, Debug)]
pub struct AiHealth {
    pub enabled: bool,
    pub healthy: bool,
    pub detail: String,
}

#[async_trait]
pub trait Evaluator: Send + Sync {
    async fn evaluate(&self, profile: &Profile, listing: &Listing) -> AIEvaluation;
    async fn evaluate_batch(&self, profile: &Profile, listings: &[Listing]) -> HashMap<String, AIEvaluation>;
    async fn start(&self) {}
    async fn stop(&self) {}
    /// Verify the model backend is reachable and configured; record health.
    async fn preflight(&self) {}
    fn health(&self) -> AiHealth {
        AiHealth {
            enabled: false,
            healthy: true,
            detail: "AI evaluation disabled".into(),
        }
    }
}

pub const BATCH_SIZE: usize = 10;
pub const ESCALATION_DELAY: Duration = Duration::from_secs(1);
pub const NUM_WORKERS: usize = 3;

/// Extract the first complete JSON object from text, stripping markdown fences.
pub fn extract_json(text: &str) -> &str {
    let mut text = text.trim();

    // Strip leading markdown fence
    if text.starts_with("```") {
        if let Some(nl) = text.find('\n') {
            text = &text[nl + 1..];
        }
    }

    // Strip trailing fence
    let trimmed = text.trim_end();
    if trimmed.ends_with("```") {
        if let Some(pos) = trimmed.rfind("```") {
            text = trimmed[..pos].trim();
        }
    }

    // Find the first '{' or '[' and its balanced closer — models return
    // either a single object or (for batches) a top-level array, often
    // wrapped in prose or fences.
    let start = match text.find(['{', '[']) {
        Some(i) => i,
        None => return text,
    };
    let (open, close) = if text.as_bytes()[start] == b'{' {
        (b'{', b'}')
    } else {
        (b'[', b']')
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
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return &text[start..=i];
            }
        }
    }

    &text[start..]
}

/// Case-insensitive keyword matching against listing title and description.
pub fn match_escalation_keywords(
    keywords: &[String],
    title: &str,
    description: &str,
) -> Vec<String> {
    let text = format!("{title} {description}").to_lowercase();
    keywords
        .iter()
        .filter(|kw| text.contains(&kw.to_lowercase()))
        .cloned()
        .collect()
}

// -- NoopEvaluator --

pub struct NoopEvaluator;

#[async_trait]
impl Evaluator for NoopEvaluator {
    async fn evaluate(&self, _profile: &Profile, _listing: &Listing) -> AIEvaluation {
        AIEvaluation::passthrough()
    }

    async fn evaluate_batch(
        &self,
        _profile: &Profile,
        listings: &[Listing],
    ) -> HashMap<String, AIEvaluation> {
        listings
            .iter()
            .map(|l| (l.id.clone(), AIEvaluation::passthrough()))
            .collect()
    }
}

// -- AIEvaluator --

pub struct AIEvaluator {
    config: AIConfig,
    filter_base_url: String,
    escalation_model: String,
    client: reqwest::Client,
    semaphore: Arc<Semaphore>,
    health: Arc<std::sync::Mutex<(bool, String)>>,
}

/// Check whether a wanted model name is present in Ollama's installed list.
/// "qwen3:32b" matches exactly; "qwen3" also matches "qwen3:latest" or any
/// "qwen3:<tag>".
pub fn model_available(installed: &[String], wanted: &str) -> bool {
    installed.iter().any(|name| {
        name == wanted
            || (!wanted.contains(':') && name.starts_with(&format!("{wanted}:")))
    })
}

#[derive(Deserialize)]
struct OllamaTag {
    name: String,
}

#[derive(Deserialize)]
struct OllamaTags {
    models: Vec<OllamaTag>,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

/// Item in a batch response from the model.
#[derive(Deserialize)]
struct BatchItem {
    id: Option<String>,
    #[serde(default)]
    relevant: bool,
    #[serde(default)]
    reason: String,
    notable: Option<String>,
    #[serde(default)]
    escalate: bool,
}

/// Wrapper for batch responses that come as `{"results": [...]}`.
#[derive(Deserialize)]
struct BatchWrapper {
    results: Option<Vec<serde_json::Value>>,
    evaluations: Option<Vec<serde_json::Value>>,
}

/// Coerce whatever JSON shape the model produced into a list of verdict
/// objects. Models — especially ones whose runner doesn't enforce the
/// output schema — return every shape imaginable: a bare array, a fenced
/// array, `{"results": [...]}`-style wrappers, an object KEYED by listing
/// id, or a single verdict object. Every recoverable shape is recovered;
/// only genuinely verdict-free output is an error (returned as a short
/// shape description for the health detail).
pub fn coerce_batch_verdicts(content: &str) -> Result<Vec<serde_json::Value>, String> {
    let content = extract_json(content);
    let parsed: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("invalid JSON: {e}"))?;
    let looks_like_verdict = |v: &serde_json::Value| v.get("relevant").is_some();

    match parsed {
        serde_json::Value::Array(arr) => Ok(arr),
        serde_json::Value::Object(obj) => {
            // Known wrapper keys first.
            if let Some(arr) = serde_json::from_value::<BatchWrapper>(
                serde_json::Value::Object(obj.clone()),
            )
            .ok()
            .and_then(|w| w.results.or(w.evaluations))
            {
                return Ok(arr);
            }
            // Any array-valued key.
            if let Some(arr) = obj.values().find_map(|v| v.as_array().cloned()) {
                return Ok(arr);
            }
            // The object IS a single verdict.
            let val = serde_json::Value::Object(obj);
            if looks_like_verdict(&val) {
                return Ok(vec![val]);
            }
            // An object keyed by listing id: {"<id>": {verdict}, ...} —
            // inject the key as the verdict's id when it lacks one.
            let obj = match val {
                serde_json::Value::Object(o) => o,
                _ => unreachable!(),
            };
            let keyed: Vec<serde_json::Value> = obj
                .into_iter()
                .filter(|(_, v)| looks_like_verdict(v))
                .map(|(k, v)| {
                    let mut m = v.as_object().cloned().unwrap_or_default();
                    m.entry("id".to_string())
                        .or_insert_with(|| serde_json::Value::String(k));
                    serde_json::Value::Object(m)
                })
                .collect();
            if !keyed.is_empty() {
                return Ok(keyed);
            }
            Err("object with no verdicts".into())
        }
        other => Err(format!("expected list, got {}", value_type_name(&other))),
    }
}

impl AIEvaluator {
    pub fn new(config: AIConfig) -> Self {
        let filter_base_url = config
            .litellm_base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        let escalation_model = config.escalation_model.clone();
        let client = reqwest::Client::new();
        let semaphore = Arc::new(Semaphore::new(NUM_WORKERS));

        Self {
            config,
            filter_base_url,
            escalation_model,
            client,
            semaphore,
            health: Arc::new(std::sync::Mutex::new((
                false,
                "not yet checked".into(),
            ))),
        }
    }

    pub fn start(&self) {
        // Workers are spawned on demand via semaphore -- nothing to do here.
    }

    pub fn stop(&self) {
        // No persistent workers to shut down.
    }

    fn record_health(&self, healthy: bool, detail: impl Into<String>) {
        let mut detail = detail.into();
        detail.truncate(160);
        if let Ok(mut h) = self.health.lock() {
            *h = (healthy, detail);
        }
    }

    fn health_snapshot(&self) -> AiHealth {
        let (healthy, detail) = self
            .health
            .lock()
            .map(|h| h.clone())
            .unwrap_or((false, "health lock poisoned".into()));
        AiHealth { enabled: true, healthy, detail }
    }

    /// Verify Ollama is reachable and has the configured filter model, and
    /// that the escalation key is present when escalation is on. Failures
    /// are recorded in health and logged at ERROR — never silent.
    async fn run_preflight(&self) {
        let url = format!("{}/api/tags", self.filter_base_url);
        let tags = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .and_then(|r| r.error_for_status());
        let detail = match tags {
            Err(e) => Some(format!(
                "Ollama unreachable at {} ({e})",
                self.filter_base_url
            )),
            Ok(resp) => match resp.json::<OllamaTags>().await {
                Err(e) => Some(format!("Ollama tags unreadable: {e}")),
                Ok(tags) => {
                    let names: Vec<String> =
                        tags.models.into_iter().map(|m| m.name).collect();
                    if model_available(&names, &self.config.filter_model) {
                        None
                    } else {
                        Some(format!(
                            "model '{}' not installed in Ollama (have: {})",
                            self.config.filter_model,
                            if names.is_empty() {
                                "none".to_string()
                            } else {
                                names.join(", ")
                            }
                        ))
                    }
                }
            },
        };

        let escalation_gap = (self.config.escalation_enabled
            && self.config.anthropic_api_key.is_empty())
        .then_some(
            "escalation enabled but no API key — sops edit ~/.config/scavenger/secrets.sops.yaml",
        );

        match (detail, escalation_gap) {
            (Some(d), gap) => {
                let full = match gap {
                    Some(g) => format!("{d}; {g}"),
                    None => d,
                };
                log::error!("AI filtering NOT working: {full} — listings pass through unfiltered");
                self.record_health(false, full);
            }
            (None, Some(g)) => {
                warn!("AI filter ready ({}), but {g}", self.config.filter_model);
                self.record_health(true, format!("{} ready; {g}", self.config.filter_model));
            }
            (None, None) => {
                info!("AI filter ready: {} via {}", self.config.filter_model, self.filter_base_url);
                self.record_health(true, format!("{} ready", self.config.filter_model));
            }
        }
    }

    /// JSON schema for a single evaluation object — passed as Ollama's
    /// structured-output `format` so the model cannot answer off-shape.
    fn eval_object_schema(with_id: bool) -> serde_json::Value {
        let mut props = serde_json::json!({
            "relevant": {"type": "boolean"},
            "reason": {"type": "string"},
            "notable": {"type": ["string", "null"]},
            "escalate": {"type": "boolean"},
        });
        let mut required = vec!["relevant", "reason", "notable", "escalate"];
        if with_id {
            props["id"] = serde_json::json!({"type": "string"});
            required.insert(0, "id");
        }
        serde_json::json!({"type": "object", "properties": props, "required": required})
    }

    /// Schema for a batch response: an array of id-tagged evaluations.
    /// JSON-object mode ("format": "json") biases models toward a top-level
    /// object even when the prompt demands an array — which is exactly how
    /// batch verdicts silently degraded to passthrough. A real schema forces
    /// the array.
    fn batch_array_schema(count: usize) -> serde_json::Value {
        // min/maxItems pinned to the batch size — small models otherwise
        // stop early and return verdicts for only the first few listings.
        serde_json::json!({
            "type": "array",
            "minItems": count,
            "maxItems": count,
            "items": Self::eval_object_schema(true),
        })
    }

    async fn post_chat(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> reqwest::Result<OllamaChatResponse> {
        self.client
            .post(url)
            .json(body)
            .timeout(Duration::from_secs_f64(self.config.filter_timeout_sec))
            .send()
            .await?
            .error_for_status()?
            .json::<OllamaChatResponse>()
            .await
    }

    /// POST to Ollama's native chat endpoint with thinking disabled.
    ///
    /// The OpenAI-compatible endpoint leaves reasoning models (qwen3 family)
    /// in thinking mode, which multiplies generation time past any sane
    /// timeout for a 10-listing batch. The native API takes `think: false`
    /// and `format: "json"` directly — measured 1.1s vs >30s per call.
    async fn call_filter(
        &self,
        system: &str,
        user: &str,
        format: serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/chat", self.filter_base_url);
        info!(
            "filter call: model={} prompt_len={}",
            self.config.filter_model,
            user.len()
        );

        let body = serde_json::json!({
            "model": &self.config.filter_model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "stream": false,
            "think": false,
            "format": format,
            "options": {"temperature": 0.1},
        });

        let mut resp = self.post_chat(&url, &body).await;
        if matches!(
            &resp,
            Err(e) if e.status() == Some(reqwest::StatusCode::BAD_REQUEST)
        ) {
            // Non-reasoning models reject the `think` field on some Ollama
            // versions — retry once without it.
            let mut fallback = body;
            fallback.as_object_mut().unwrap().remove("think");
            resp = self.post_chat(&url, &fallback).await;
        }
        let resp = match resp {
            Ok(r) => {
                self.record_health(true, format!("{} ok", self.config.filter_model));
                r
            }
            Err(e) => {
                self.record_health(false, format!("filter call failed: {e}"));
                return Err(e.into());
            }
        };

        let content = resp.message.content;

        info!("filter response: {} chars", content.len());
        Ok(content)
    }

    /// POST to Anthropic Messages API.
    async fn call_frontier(&self, system: &str, user: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "frontier call: model={} prompt_len={}",
            self.escalation_model,
            user.len()
        );

        let body = serde_json::json!({
            "model": &self.escalation_model,
            "system": system,
            "messages": [
                {"role": "user", "content": user},
            ],
            "temperature": 0.1,
            "max_tokens": 1024,
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.config.anthropic_api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .timeout(Duration::from_secs_f64(self.config.escalation_timeout_sec))
            .send()
            .await?
            .error_for_status()?
            .json::<AnthropicResponse>()
            .await?;

        let raw = resp
            .content
            .into_iter()
            .find_map(|b| b.text)
            .unwrap_or_default();

        let content = extract_json(&raw).to_string();
        info!("frontier response: {} chars", content.len());
        Ok(content)
    }

    /// Evaluate a single listing with optional escalation.
    pub async fn evaluate(&self, profile: &Profile, listing: &Listing) -> AIEvaluation {
        let (system, user) = build_prompt(profile, listing);
        let content = match self
            .call_filter(&system, &user, Self::eval_object_schema(false))
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("Filter eval failed: {e}");
                return AIEvaluation::passthrough();
            }
        };

        let evaluation: AIEvaluation = match serde_json::from_str(&content) {
            Ok(ev) => ev,
            Err(e) => {
                warn!("Filter parse failed: {e}");
                return AIEvaluation::passthrough();
            }
        };

        if !evaluation.relevant {
            return evaluation;
        }

        let mut should_escalate = evaluation.escalate;
        let mut matched = Vec::new();
        if self.config.escalation_enabled && !profile.escalation_keywords.is_empty() {
            matched = match_escalation_keywords(
                &profile.escalation_keywords,
                &listing.title,
                &listing.description,
            );
            if !matched.is_empty() {
                should_escalate = true;
            }
        }

        if should_escalate && self.config.escalation_enabled {
            if matched.is_empty() {
                matched = vec!["(model-triggered)".to_string()];
            }
            let escalation = self.escalate(profile, listing, &matched).await;
            return AIEvaluation {
                relevant: evaluation.relevant,
                reason: if escalation.reason.is_empty() {
                    evaluation.reason
                } else {
                    escalation.reason
                },
                notable: escalation.notable.or(evaluation.notable),
                escalate: escalation.escalate,
            };
        }

        evaluation
    }

    /// Evaluate a batch of listings concurrently via semaphore-limited workers.
    pub async fn evaluate_batch(
        &self,
        profile: &Profile,
        listings: &[Listing],
    ) -> HashMap<String, AIEvaluation> {
        if listings.is_empty() {
            return HashMap::new();
        }

        let chunks: Vec<&[Listing]> = listings.chunks(BATCH_SIZE).collect();
        let mut all_results = HashMap::new();

        let futures_vec: Vec<_> = chunks
            .iter()
            .map(|chunk| self.process_batch_with_semaphore(profile, chunk.to_vec()))
            .collect();

        let results = futures::future::join_all(futures_vec).await;
        for result in results {
            match result {
                Ok(map) => all_results.extend(map),
                Err(e) => {
                    warn!("Batch chunk failed: {e}");
                    // passthrough handled inside process_batch already
                }
            }
        }

        // Escalate listings matching escalation keywords
        if self.config.escalation_enabled && !profile.escalation_keywords.is_empty() {
            let mut escalation_count = 0u32;
            for listing in listings {
                let ev = match all_results.get(&listing.id) {
                    Some(ev) if ev.relevant => ev.clone(),
                    _ => continue,
                };
                let matched = match_escalation_keywords(
                    &profile.escalation_keywords,
                    &listing.title,
                    &listing.description,
                );
                if matched.is_empty() {
                    continue;
                }
                info!(
                    "Escalating {} -- matched: {}",
                    &listing.id[..listing.id.len().min(12)],
                    matched.join(", ")
                );
                if escalation_count > 0 {
                    tokio::time::sleep(ESCALATION_DELAY).await;
                }
                let escalation = self.escalate(profile, listing, &matched).await;
                all_results.insert(
                    listing.id.clone(),
                    AIEvaluation {
                        relevant: ev.relevant,
                        reason: if escalation.reason.is_empty() {
                            ev.reason.clone()
                        } else {
                            escalation.reason
                        },
                        notable: escalation.notable.or(ev.notable.clone()),
                        escalate: escalation.escalate,
                    },
                );
                escalation_count += 1;
            }
        }

        all_results
    }

    async fn process_batch_with_semaphore(
        &self,
        profile: &Profile,
        chunk: Vec<Listing>,
    ) -> Result<HashMap<String, AIEvaluation>, Box<dyn std::error::Error + Send + Sync>> {
        let _permit = self.semaphore.acquire().await?;
        Ok(self.process_batch(profile, &chunk).await)
    }

    /// Process a single batch chunk: build prompt, call filter, parse array response.
    async fn process_batch(
        &self,
        profile: &Profile,
        listings: &[Listing],
    ) -> HashMap<String, AIEvaluation> {
        let (system, user) = build_batch_prompt(profile, listings);
        let content = match self
            .call_filter(&system, &user, Self::batch_array_schema(listings.len()))
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("AI batch eval failed: {e}");
                return passthrough_all(listings);
            }
        };

        let parsed = match coerce_batch_verdicts(&content) {
            Ok(items) => items,
            Err(shape) => {
                warn!(
                    "AI batch response unusable ({shape}) — passthrough for {} listings",
                    listings.len()
                );
                self.record_health(false, format!("bad model output: {shape}"));
                return passthrough_all(listings);
            }
        };

        let mut results = HashMap::new();
        for item_val in parsed {
            match serde_json::from_value::<BatchItem>(item_val) {
                Ok(item) => {
                    if let Some(id) = item.id {
                        results.insert(
                            id,
                            AIEvaluation {
                                relevant: item.relevant,
                                reason: item.reason,
                                notable: item.notable,
                                escalate: item.escalate,
                            },
                        );
                    }
                }
                Err(e) => {
                    log::debug!("AI batch: skipping malformed item: {e}");
                }
            }
        }

        // A listing the model skipped (or whose id it mangled) gets a real
        // second chance as a single-item evaluation — the single prompt is
        // more reliable than the batch, and silent passthrough back-fill is
        // exactly how junk listings sneak past filtering. evaluate() itself
        // still falls back to passthrough on model failure, so nothing is
        // ever dropped.
        let missing: Vec<&Listing> = listings
            .iter()
            .filter(|l| !results.contains_key(&l.id))
            .collect();
        if !missing.is_empty() {
            warn!(
                "AI batch: {} of {} verdicts missing from model response — re-evaluating individually",
                missing.len(),
                listings.len()
            );
            for listing in missing {
                let eval = self.evaluate(profile, listing).await;
                results.insert(listing.id.clone(), eval);
            }
        }

        results
    }

    /// Call the frontier model for deeper evaluation.
    async fn escalate(
        &self,
        profile: &Profile,
        listing: &Listing,
        triggered_keywords: &[String],
    ) -> AIEvaluation {
        let (system, user) = build_escalation_prompt(profile, listing, triggered_keywords);
        match self.call_frontier(&system, &user).await {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(ev) => ev,
                Err(e) => {
                    warn!(
                        "Escalation parse failed for {}: {e}",
                        &listing.id[..listing.id.len().min(12)]
                    );
                    AIEvaluation::passthrough()
                }
            },
            Err(e) => {
                warn!(
                    "Escalation failed for {}: {e}",
                    &listing.id[..listing.id.len().min(12)]
                );
                AIEvaluation::passthrough()
            }
        }
    }
}

#[async_trait]
impl Evaluator for AIEvaluator {
    async fn evaluate(&self, profile: &Profile, listing: &Listing) -> AIEvaluation {
        self.evaluate(profile, listing).await
    }

    async fn evaluate_batch(
        &self,
        profile: &Profile,
        listings: &[Listing],
    ) -> HashMap<String, AIEvaluation> {
        self.evaluate_batch(profile, listings).await
    }

    async fn start(&self) {
        self.start();
    }

    async fn stop(&self) {
        self.stop();
    }

    async fn preflight(&self) {
        self.run_preflight().await;
    }

    fn health(&self) -> AiHealth {
        self.health_snapshot()
    }
}

fn passthrough_all(listings: &[Listing]) -> HashMap<String, AIEvaluation> {
    listings
        .iter()
        .map(|l| (l.id.clone(), AIEvaluation::passthrough()))
        .collect()
}

fn value_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_simple() {
        let input = r#"{"relevant": true, "reason": "test"}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_with_fences() {
        let input = "```json\n{\"relevant\": true}\n```";
        assert_eq!(extract_json(input), "{\"relevant\": true}");
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let input = "Here is the result: {\"a\": 1} and some trailing text";
        assert_eq!(extract_json(input), "{\"a\": 1}");
    }

    #[test]
    fn test_extract_json_nested() {
        let input = r#"{"outer": {"inner": 1}}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_top_level_array() {
        let input = "```json\n[{\"id\": \"a\"}, {\"id\": \"b\"}]\n```";
        assert_eq!(extract_json(input), "[{\"id\": \"a\"}, {\"id\": \"b\"}]");
        let prose = "Here are the results: [{\"id\": \"a\"}] hope that helps";
        assert_eq!(extract_json(prose), "[{\"id\": \"a\"}]");
    }

    #[test]
    fn model_available_matches_exact_and_untagged() {
        let installed = vec!["qwen3:32b".to_string(), "llama3:latest".to_string()];
        assert!(model_available(&installed, "qwen3:32b"));
        assert!(model_available(&installed, "llama3"));
        assert!(!model_available(&installed, "qwen3:8b"));
        assert!(!model_available(&installed, "mistral"));
        assert!(!model_available(&[], "qwen3:32b"));
    }

    #[test]
    fn health_starts_unchecked_and_records_transitions() {
        let ev = AIEvaluator::new(AIConfig::default());
        let h = ev.health_snapshot();
        assert!(h.enabled && !h.healthy);

        ev.record_health(true, "model ready");
        assert!(ev.health_snapshot().healthy);

        ev.record_health(false, "connection refused");
        let h = ev.health_snapshot();
        assert!(!h.healthy);
        assert!(h.detail.contains("connection refused"));
    }

    #[test]
    fn noop_evaluator_reports_disabled_and_healthy() {
        let h = Evaluator::health(&NoopEvaluator);
        assert!(!h.enabled);
        assert!(h.healthy);
    }

    #[test]
    fn coerce_handles_every_model_shape() {
        // Bare array.
        let v = coerce_batch_verdicts(r#"[{"id":"a","relevant":true}]"#).unwrap();
        assert_eq!(v.len(), 1);
        // Fenced array.
        let v = coerce_batch_verdicts("```json\n[{\"id\":\"a\",\"relevant\":false}]\n```").unwrap();
        assert_eq!(v.len(), 1);
        // Known wrapper.
        let v = coerce_batch_verdicts(r#"{"results":[{"id":"a","relevant":true},{"id":"b","relevant":false}]}"#)
            .unwrap();
        assert_eq!(v.len(), 2);
        // Unknown wrapper with an array value.
        let v = coerce_batch_verdicts(r#"{"listings":[{"id":"a","relevant":true}]}"#).unwrap();
        assert_eq!(v.len(), 1);
        // Single verdict object.
        let v = coerce_batch_verdicts(r#"{"id":"a","relevant":true,"reason":"x"}"#).unwrap();
        assert_eq!(v.len(), 1);
        // Object keyed by listing id — ids injected from keys.
        let v = coerce_batch_verdicts(
            r#"{"abc":{"relevant":false,"reason":"mattress"},"def":{"relevant":true,"reason":"server"}}"#,
        )
        .unwrap();
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|i| i.get("id").is_some()));
        // Genuinely verdict-free output fails loudly.
        assert!(coerce_batch_verdicts(r#"{"note":"I could not evaluate"}"#).is_err());
        assert!(coerce_batch_verdicts("no json at all").is_err());
    }
}
