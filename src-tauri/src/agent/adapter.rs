//! LLM adapter trait and implementations.
//!
//! Each adapter is a thin HTTP client wrapping a different LLM endpoint:
//! Ollama (local), HF Space (cloud passthrough), OpenAI (future).
//!
//! All adapters implement the same `LlmAdapter` trait, so chain execution code
//! never needs to know which backend is active.

use serde::{Deserialize, Serialize};
use async_trait::async_trait;

// Re-export types from chain_format so callers don't have to traverse modules.
pub use super::chain_format::spec::{Message, Role};

// ────────────────────────────────────────────────────────────────────────────
// LlmAdapter trait
// ────────────────────────────────────────────────────────────────────────────

/// A chat-completions-capable LLM backend.
///
/// Implementors are stateless — state (conversation history, token budget) is
/// managed by the caller via [`crate::agent::memory::AgentSession`].
///
/// Canonical call path:
///
/// ```ignore
/// let resp = adapter.chat(&session.messages, &CallOpts { … }).await?;
/// session.messages.push(Message { role: Role::Assistant, content: resp.text });
/// ```
#[async_trait]
pub trait LlmAdapter: Send + Sync {
    /// Send `messages` to the LLM and return the raw text response.
    ///
    /// `opts.model` and `opts.temperature` must be honoured.
    /// If `opts.tools` is non-empty the adapter MAY attach them as OpenAI-style
    /// `functions` / `tools` JSON to influence the model's response format.
    async fn chat(
        &self,
        messages: &[Message],
        opts:     &CallOpts<'_>,
    ) -> Result<LlmResponse, AdapterError>;
}

// ────────────────────────────────────────────────────────────────────────────
// Call options
// ────────────────────────────────────────────────────────────────────────────

/// Per-call options forwarded to the adapter.
#[derive(Debug, Clone)]
pub struct CallOpts<'a> {
    /// Model identifier in the form expected by the adapter
    /// (e.g. `"llama3.2:3b"` for Ollama, `"gpt-4o"` for OpenAI).
    pub model:       ModelId<'a>,
    /// Sampling temperature.  0.0 = greedy, 1.0 = creative.
    #[allow(dead_code)]
    pub temperature: f32,
    /// Hard cap on output tokens.  `None` = adapter default.
    #[allow(dead_code)]
    pub max_tokens:  Option<u32>,
    /// Tool declarations forwarded to the LLM as function-call schemas.
    #[allow(dead_code)]
    pub tools:       Vec<ToolSpec>,
}

/// Model identifier — a borrowed string slice for zero-copy from config.
#[derive(Debug, Clone, Copy)]
pub struct ModelId<'a>(&'a str);

impl<'a> ModelId<'a> {
    pub fn new(s: &'a str) -> Self { ModelId(s) }
    pub fn as_str(&self) -> &'a str { self.0 }
}

impl<'a> AsRef<str> for ModelId<'a> {
    fn as_ref(&self) -> &str { self.0 }
}

// ────────────────────────────────────────────────────────────────────────────
// ToolSpec (adapter view)
// ────────────────────────────────────────────────────────────────────────────

/// A tool declaration in the OpenAI / Ollama function-calling shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name:        String,
    pub description: String,
    /// JSON Schema for the tool's parameters object.
    pub parameters:  serde_json::Value,
}

impl ToolSpec {
    pub fn new(name: &str, description: &str, parameters: serde_json::Value) -> Self {
        Self { name: name.to_string(), description: description.to_string(), parameters }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// LlmResponse
// ────────────────────────────────────────────────────────────────────────────

/// Response from an LLM call.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// Plain text content.
    pub text: String,
    /// If the model emitted structured tool/function calls, they appear here.
    #[allow(dead_code)]
    pub tool_calls: Vec<crate::agent::tools::ToolCall>,
    /// Model that produced this response.
    #[allow(dead_code)]
    pub model:      String,
    /// Prompt + completion token usage (if the adapter reports it).
    #[allow(dead_code)]
    pub usage:      Option<TokenUsage>,
}

/// Token usage breakdown.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens:     u32,
    pub completion_tokens: u32,
    pub total_tokens:      u32,
}

// ────────────────────────────────────────────────────────────────────────────
// Errors
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("HTTP request failed: {0}")]
    Transport(String),

    #[error("LLM returned error: {0}")]
    Model(String),

    #[error("Invalid response: {0}")]
    Parse(String),
}

// ────────────────────────────────────────────────────────────────────────────
// Ollama adapter
// ────────────────────────────────────────────────────────────────────────────

/// Adapter for a local Ollama server at `http://localhost:11434`.
///
/// Must be reachable for chat calls.  Run `ollama serve` before starting.
#[derive(Debug, Clone)]
pub struct OllamaAdapter {
    base_url: String,
    client:   reqwest::Client,
}

impl Default for OllamaAdapter {
    fn default() -> Self {
        Self {
            base_url: std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            client:   reqwest::Client::builder()
                .user_agent("hiem-agent/0.1")
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl LlmAdapter for OllamaAdapter {
    async fn chat(&self, messages: &[Message], opts: &CallOpts<'_>) -> Result<LlmResponse, AdapterError> {
        let body = serde_json::json!({
            "model":       opts.model.as_ref(),
            "messages":    messages.iter().map(|m| serde_json::json!({
                "role":    m.role.as_str(),
                "content": m.content,
            })).collect::<Vec<_>>(),
            "stream":     false,
            "options":    {
                "temperature": opts.temperature,
            }
        });

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/')))
            .json(&body)
            .send()
            .await
            .map_err(|e| AdapterError::Transport(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body   = resp.text().await.unwrap_or_default();
            return Err(AdapterError::Model(format!("HTTP {}: {}", status, body)));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AdapterError::Parse(e.to_string()))?;

        let text = json["choices"]
            .get(0)
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse {
            text,
            tool_calls: vec![],
            model:      opts.model.as_ref().to_string(),
            usage:  None,
        })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// HF Space adapter (stub — activated lazily)
// ────────────────────────────────────────────────────────────────────────────

/// Adapter for a Hugging Face Space `/api/chat` endpoint.
///
/// This is a passthrough stub — the HF Space already serves the frontend.
/// Bind a full adapter here if the Space is extended with `/agent/run`.
#[derive(Debug, Clone)]
pub struct HfSpaceAdapter {
    base_url: String,
    client:   reqwest::Client,
}

impl Default for HfSpaceAdapter {
    fn default() -> Self {
        Self {
            base_url: std::env::var("HF_SPACE_URL")
                .unwrap_or_else(|_| "https://boozelee-hiem-app.hf.space".to_string()),
            client:   reqwest::Client::builder()
                .user_agent("hiem-agent/0.1")
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl LlmAdapter for HfSpaceAdapter {
    async fn chat(&self, _messages: &[Message], opts: &CallOpts<'_>) -> Result<LlmResponse, AdapterError> {
        let _ = opts; // stub — enables compile while HF Space endpoint is absent
        Err(AdapterError::Model(
            "HF Space adapter is a stub — implement POST /api/chat body parsing.".to_string()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// smoke-test: the OllamaAdapter struct builds and has the right base URL.
    #[test]
    fn test_ollama_adapter_default_base_url() {
        let a = OllamaAdapter::default();
        assert!(a.base_url.contains("localhost:11434"));
    }

    #[test]
    fn test_model_id_as_str() {
        let id = ModelId::new("llama3.2:3b");
        assert_eq!(id.as_str(), "llama3.2:3b");
    }

    #[test]
    fn test_tool_spec_new() {
        let ts = ToolSpec::new(
            "whoami",
            "Get authenticated user",
            serde_json::json!({"type": "object", "properties": {}}),
        );
        assert_eq!(ts.name, "whoami");
    }
}
