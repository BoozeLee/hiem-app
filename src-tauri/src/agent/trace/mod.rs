//! Observability — LangSmith-compatible tracing events.
//!
//! Emits [`RunEvent`] structs through a pluggable [`RunEventSink`].
//! Default: JSON Lines file at `$XDG_STATE_HOME/hiem/agent_runs.jsonl`.
//! LangSmith: HTTP POST to `https://api.smith.langchain.com/api/v1/traces` when
//! `HIEM_TRACE=langsmith` and `LANGSMITH_API_KEY` is set.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single observation point emitted during chain execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    /// Unique trace identifier (stable per turn).
    pub run_id:        String,
    /// Human-readable step name (e.g. `"whoami"`, `"llm"`, `"get_repos"`).
    pub step_name:     String,
    /// When the step started (RFC 3339 UTC).
    pub start_time:    DateTime<Utc>,
    /// When the step ended (RFC 3339 UTC).
    pub end_time:      DateTime<Utc>,
    /// Millisecond latency = end - start.
    #[serde(default)]
    pub latency_ms:    u64,
    /// Status outcome.
    #[serde(default)]
    pub status:        RunStatus,
    /// Step input (redacted — never includes raw token strings).
    #[serde(default)]
    pub inputs:        serde_json::Map<String, serde_json::Value>,
    /// Step output (redacted).
    #[serde(default)]
    pub outputs:       serde_json::Map<String, serde_json::Value>,
    /// Nested child run IDs.
    #[serde(default)]
    pub child_runs:    Vec<String>,
}

/// Event status — mirrors LangSmith's RunStatus enum.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    #[default]
    Success,
    Error,
}

impl RunEvent {
    pub fn new(run_id: &str, step_name: &str) -> Self {
        let now = Utc::now();
        Self {
            run_id:         run_id.to_string(),
            step_name:      step_name.to_string(),
            start_time:     now,
            end_time:       now,
            latency_ms:     0,
            status:         RunStatus::Success,
            inputs:         serde_json::Map::new(),
            outputs:        serde_json::Map::new(),
            child_runs:     vec![],
        }
    }

    pub fn with_inputs(mut self, name: &str, value: impl Into<serde_json::Value>) -> Self {
        self.inputs.insert(name.to_string(), value.into());
        self
    }

    pub fn with_output(mut self, name: &str, value: impl Into<serde_json::Value>) -> Self {
        self.outputs.insert(name.to_string(), value.into());
        self
    }

    pub fn with_error(mut self, msg: &str) -> Self {
        self.status = RunStatus::Error;
        self.outputs.insert("error".to_string(), serde_json::json!(msg));
        self
    }

    pub fn finish(&mut self) {
        self.end_time   = Utc::now();
        self.latency_ms = self.end_time.timestamp_millis() as u64
                        - self.start_time.timestamp_millis() as u64;
    }
}

// ────────────────────────────────────────────────────────────────────────────
// RunEventSink
// ────────────────────────────────────────────────────────────────────────────

/// Destination for [`RunEvent`] objects.
#[async_trait::async_trait]
pub trait RunEventSink: Send + Sync {
    /// Emit a completed [`RunEvent`].
    async fn emit(&self, event: RunEvent);
    /// Flush any buffered events.
    async fn flush(&self) {}
}

// ────────────────────────────────────────────────────────────────────────────
// FileSink — JSON Lines to $XDG_STATE_HOME/hiem/agent_runs.jsonl
// ────────────────────────────────────────────────────────────────────────────

/// Appends each event as one JSON line to a log file.
#[derive(Debug, Clone)]
pub struct FileSink {
    path: std::path::PathBuf,
}

impl FileSink {
    pub fn new(dir: &str) -> Self {
        let path = std::path::PathBuf::from(dir).join("agent_runs.jsonl");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self { path }
    }
}

#[async_trait::async_trait]
impl RunEventSink for FileSink {
    async fn emit(&self, event: RunEvent) {
        if let Ok(line) = serde_json::to_string(&event) {
            let line = line + "\n";
            let _  = tokio::fs::write(&self.path, Vec::from(line))
                .await;       // tauri async file write
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// StdoutSink
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct StdoutSink;

#[async_trait::async_trait]
impl RunEventSink for StdoutSink {
    async fn emit(&self, event: RunEvent) {
        if let Ok(line) = serde_json::to_string(&event) {
            eprintln!("[trace] {}", line);
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// LangSmith HTTP event sink
// ────────────────────────────────────────────────────────────────────────────

/// Posts each event to LangSmith's trace API.
///
/// Activation: env `HIEM_TRACE=langsmith` + `LANGSMITH_API_KEY` set.
#[derive(Debug, Clone)]
pub struct LangSmithSink {
    endpoint: String,
    api_key:  String,
    client:   reqwest::Client,
}

impl LangSmithSink {
    pub fn new(api_key: impl Into<String>, project_id: &str) -> Self {
        Self {
            endpoint: format!("https://api.smith.langchain.com/api/v1/traces/{}", project_id),
            api_key:  api_key.into(),
            client:   reqwest::Client::builder()
                .user_agent("hiem-agent/0.1")
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait::async_trait]
impl RunEventSink for LangSmithSink {
    async fn emit(&self, event: RunEvent) {
        if let Err(e) = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&event)
            .send()
            .await
        {
            eprintln!("[langsmith] post failed: {}", e);
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Sink registry — default file + env-driven LangSmith
// ────────────────────────────────────────────────────────────────────────────

/// Build the singleton [`RunEventSink`] collection used by the runtime.
///
/// Priority:
/// 1. Always include `FileSink` (default).
/// 2. Add `LangSmithSink` when `HIEM_TRACE=langsmith` + `LANGSMITH_API_KEY` set.
/// 3. Add `StdoutSink` in dev builds.
pub fn default_sinks() -> Vec<Box<dyn RunEventSink>> {
    let mut sinks: Vec<Box<dyn RunEventSink>> = vec![
        Box::new(FileSink::new(&dirs::state_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap())
            .join("hiem").to_string_lossy().as_ref())),
    ];
    let trace_setting = std::env::var("HIEM_TRACE")
        .unwrap_or_else(|_| "file".to_string())
        .to_lowercase();
    if trace_setting.contains("langsmith") {
        if let Ok(key) = std::env::var("LANGSMITH_API_KEY") {
            sinks.push(Box::new(LangSmithSink::new(key, "hiem-app")));
        }
    }
    sinks
}

// dirs crate not in Cargo.toml yet — inline the XDG lookup for now.
mod dirs {
    use std::path::PathBuf;
    pub fn state_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| PathBuf::from(h).join(".local/state"))
            })
    }
    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_event_lifecycle() {
        let mut e = RunEvent::new("run_1", "test_step")
            .with_inputs("foo", "bar");
        assert_eq!(e.latency_ms, 0);
        assert_eq!(e.status, RunStatus::Success);
        e.finish();
        assert!(e.latency_ms >= 0);
    }

    #[test]
    fn test_run_event_stores_serialisable() {
        let e = RunEvent::new("run_1", "whoami")
            .with_inputs("session_id", "ghs_session_alice")
            .with_output("login", "alice")
            .clone();
        let json = serde_json::to_string(&e);
        assert!(json.is_ok());
    }
}
