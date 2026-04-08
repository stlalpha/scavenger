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

#[async_trait]
pub trait Evaluator: Send + Sync {
    async fn evaluate(&self, profile: &Profile, listing: &Listing) -> AIEvaluation;
    async fn evaluate_batch(&self, profile: &Profile, listings: &[Listing]) -> HashMap<String, AIEvaluation>;
    async fn start(&self) {}
    async fn stop(&self) {}
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

    // Find first '{' and matching '}'
    let start = match text.find('{') {
        Some(i) => i,
        None => return text,
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
    filter_model: String,
    filter_base_url: String,
    escalation_model: String,
    client: reqwest::Client,
    semaphore: Arc<Semaphore>,
}

#[derive(Deserialize)]
struct OllamaChoice {
    message: OllamaMessage,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Deserialize)]
struct OllamaResponse {
    choices: Vec<OllamaChoice>,
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

impl AIEvaluator {
    pub fn new(config: AIConfig) -> Self {
        let filter_model = format!("ollama/{}", config.filter_model);
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
            filter_model,
            filter_base_url,
            escalation_model,
            client,
            semaphore,
        }
    }

    pub fn start(&self) {
        // Workers are spawned on demand via semaphore -- nothing to do here.
    }

    pub fn stop(&self) {
        // No persistent workers to shut down.
    }

    /// POST to Ollama's OpenAI-compatible chat/completions endpoint.
    async fn call_filter(&self, system: &str, user: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/v1/chat/completions", self.filter_base_url);
        info!(
            "filter call: model={} prompt_len={}",
            self.filter_model,
            user.len()
        );

        let body = serde_json::json!({
            "model": &self.config.filter_model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0.1,
            "response_format": {"type": "json_object"},
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .timeout(Duration::from_secs_f64(self.config.filter_timeout_sec))
            .send()
            .await?
            .error_for_status()?
            .json::<OllamaResponse>()
            .await?;

        let content = resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

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
        let content = match self.call_filter(&system, &user).await {
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
        let content = match self.call_filter(&system, &user).await {
            Ok(c) => c,
            Err(e) => {
                warn!("AI batch eval failed: {e}");
                return passthrough_all(listings);
            }
        };

        let parsed: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(serde_json::Value::Array(arr)) => arr,
            Ok(serde_json::Value::Object(obj)) => {
                let val = serde_json::Value::Object(obj);
                if let Ok(wrapper) = serde_json::from_value::<BatchWrapper>(val) {
                    wrapper
                        .results
                        .or(wrapper.evaluations)
                        .unwrap_or_default()
                } else {
                    warn!("AI batch: expected list, got object");
                    return passthrough_all(listings);
                }
            }
            Ok(other) => {
                warn!("AI batch: expected list, got {}", value_type_name(&other));
                return passthrough_all(listings);
            }
            Err(e) => {
                warn!("AI batch eval failed (json): {e}");
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

        // Fill in passthrough for any missing listings
        for listing in listings {
            results
                .entry(listing.id.clone())
                .or_insert_with(AIEvaluation::passthrough);
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
}
