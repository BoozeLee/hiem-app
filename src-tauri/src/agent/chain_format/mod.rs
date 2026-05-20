//! Chain format: ser/de layer for LangChain / LangGraph compatible chain specs.
//!
//! Entry point: [`super::agent::chain_format::ChainSpec`]
pub mod spec;
pub mod graph;

use std::path::Path;

// Re-exports in one shot.
pub use spec::{ChainSpec, ChainType, EdgeSpec, Message, NodeSpec, NodeType, Role, ToolSpec};
pub use graph::*;

// ────────────────────────────────────────────────────────────────────────────
// Loading helpers
// ────────────────────────────────────────────────────────────────────────────

/// Deserialise a ChainSpec from a YAML or JSON file.
pub fn load(path: &str) -> Result<ChainSpec, String> {
    let raw = std::fs::read(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
    let content = std::str::from_utf8(&raw).map_err(|e| format!("not UTF-8: {}", e))?.to_string();

    // Try YAML first, then JSON
    serde_yaml::from_str(&content)
        .or_else(|_| serde_json::from_str(&content))
        .map_err(|e| format!("failed to parse chain spec from {}: {}", path, e))
}

/// Deserialise a ChainSpec from a JSON `&str`.
#[inline]
pub fn from_json(s: &str) -> Result<ChainSpec, String> {
    serde_json::from_str(s).map_err(|e| format!("chain spec JSON parse error: {}", e))
}

/// Serialise a ChainSpec to pretty JSON `String`.
#[inline]
pub fn to_json(spec: &ChainSpec) -> Result<String, String> {
    serde_json::to_string_pretty(spec).map_err(|e| e.to_string())
}
