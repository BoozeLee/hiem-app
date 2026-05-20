//! Agent chain format types.
//!
//! Mirrors LangChain LCEL ("type: sequential | tool") and LangGraph JSON graph
//! schemas so a chain YAML authored for Python LangChain / LangGraph is
//! structurally identical to the Rust-side spec.
//!
//! Format: JSON or YAML, root type is [`ChainSpec`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ────────────────────────────────────────────────────────────────────────────
// Message
// ────────────────────────────────────────────────────────────────────────────

/// A single message in a chain's step list or execution log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct Message {
    pub role:    Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn system(s: &str)      -> Message { Message { role: Role::System,  content: s.to_string() } }
    pub fn user(s: &str)        -> Message { Message { role: Role::User,    content: s.to_string() } }
    pub fn assistant(s: &str)   -> Message { Message { role: Role::Assistant, content: s.to_string() } }
    pub fn tool(s: &str)        -> Message { Message { role: Role::Tool,     content: s.to_string() } }
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// ChainSpec — LangChain LCEL-inspired
// ────────────────────────────────────────────────────────────────────────────

/// Top-level chain specification.
///
/// Deserialises from either LCEL-style JSON or YAML.
///
/// # Example YAML
///
/// ```yaml
/// type: sequential
/// name: hiem_engineering
/// model: ollama/llama3.2:3b
/// temperature: 0.2
/// max_tokens: 4096
/// steps:
///   - role: system
///     content: "You are HIEM's engineering agent."
///   - role: user
///     content: "{user_message}"
/// tools:
///   - name: whoami
///     endpoint: whoami
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChainSpec {
    #[serde(default = "default_chain_type")]
    pub chain_type:  ChainType,
    pub name:        String,
    pub description: Option<String>,

    /// LLM target: `ollama/llama3.2:3b`, `hf/gpt-4o-mini`, `openai/gpt-4o`
    #[serde(default = "default_model")]
    pub model:       String,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default)]
    pub max_tokens:  Option<u32>,

    /// System prompt + user message steps.
    #[serde(default)]
    pub steps:       Vec<Message>,

    /// Tool binding declarations.
    #[serde(default)]
    pub tools:       Vec<ToolSpec>,

    /// --- LangGraph fields (only used when `chain_type = Graph`) ---

    /// Nodes in the graph (only used by `chain_type = graph`).
    #[serde(default)]
    pub nodes:       Vec<NodeSpec>,

    /// Edges connecting nodes.
    #[serde(default)]
    pub edges:       Vec<EdgeSpec>,

    /// Max tool-call → LLM → tool-call rounds per turn (LangGraph `recursion_limit`).
    #[serde(default = "default_loop_budget")]
    pub loop_budget: u32,

    /// Token budget for the sliding window (in tokens, not chars).
    #[serde(default = "default_token_budget")]
    pub token_budget: u32,
}

pub fn default_chain_type() -> ChainType  { ChainType::Sequential }
pub fn default_model()        -> String   { "ollama/llama3.2:3b".to_string() }
pub fn default_temperature()  -> f32      { 0.2 }
pub fn default_loop_budget()  -> u32      { 6 }
pub fn default_token_budget() -> u32      { 4096 }

// ────────────────────────────────────────────────────────────────────────────
// ChainType
// ────────────────────────────────────────────────────────────────────────────

/// Chain topology.
///
/// | Variant | Equivalent LCEL / LangGraph construct |
/// |---------|----------------------------------------|
/// | `Sequential` | `RunnableSequence`  —— sys-prompt → user → LLM → (optional tool) → reply |
/// | `Tool`       | `create_tool_calling_agent`  —— LLM with tool bindings |
/// | `Graph`      | `StateGraph`  —— arbitrary node/edge topology |
/// | `Router`     | `RunnableRouter`  — (Phase 8) |
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChainType {
    #[default]
    Sequential,
    Tool,
    Graph,
    Router,
}

// ────────────────────────────────────────────────────────────────────────────
// ToolSpec — the tool binding declaration inside a chain
// ────────────────────────────────────────────────────────────────────────────

/// Declares a tool that the LLM can call when this chain is active.
///
/// Appears inside `tools:` in the chain YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Stable name — also the Tauri IPC command or ToolRegistry key.
    pub name:        String,
    /// One-line description shown to the LLM in the system prompt tools block.
    pub description: String,
    /// Tauri IPC command name (e.g. `get_repos`, `whoami`).
    /// Not needed if `ToolRegistry` is used directly.
    #[serde(default)]
    pub endpoint:    Option<String>,
    /// JSON object mapping arg_name → expected_type string (e.g. `"str"`).
    #[serde(default)]
    pub args_schema: HashMap<String, String>,
}

// ────────────────────────────────────────────────────────────────────────────
// GraphSpec — LangGraph-compatible
// ────────────────────────────────────────────────────────────────────────────

/// Node types within a `graph`-type chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NodeSpec {
    pub id:       String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub prompt:   Option<String>,
    /// Call a registered tool by name. Mutually exclusive with `prompt`.
    #[serde(default)]
    pub tool:     Option<String>,
    /// Merge these FSM state keys into the node input.
    #[serde(default)]
    pub merge_state: Vec<String>,
}

/// Node type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Llm,         // call LLM once, pass result to next node(s)
    Toolset,     // bind all registered tools
    Condition,   // if tool_call → execute, else reply
    Passthrough, // pass input through unchanged
}

/// Directed edge in a graph chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from:  String,
    #[serde(default)]
    pub to:    Vec<String>,  // fan-out allowed
}

// ────────────────────────────────────────────────────────────────────────────
// Validation
// ────────────────────────────────────────────────────────────────────────────

/// Basic structural validation — catch missing fields or bad references early.
impl ChainSpec {
    /// Returns `Ok(())` if the spec is internally consistent, or the first
    /// error found as an `Err(String)`.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("chain name must not be empty".to_string());
        }
        if self.temperature < 0.0 || self.temperature > 2.0 {
            return Err(format!("temperature must be between 0.0 and 2.0, got {}", self.temperature));
        }
        if self.token_budget < 256 {
            return Err(format!("token_budget must be ≥ 256, got {}", self.token_budget));
        }
        if self.loop_budget == 0 {
            return Err("loop_budget must be ≥ 1".to_string());
        }

        // --- Graph-specific checks ---
        if self.chain_type == ChainType::Graph {
            // Every edge `from` / `to` must reference a declared node
            let node_ids: std::collections::HashSet<_> =
                self.nodes.iter().map(|n| &n.id).collect();
            for edge in &self.edges {
                if !node_ids.contains(&edge.from) {
                    return Err(format!("edge references unknown node '{}'", edge.from));
                }
                for t in &edge.to {
                    if !node_ids.contains(t) {
                        return Err(format!("edge references unknown node '{}'", t));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal LCEL-style sequential chain — round-trips through YAML.
    #[test]
    fn test_sequential_chain_yaml_roundtrip() {
        let yaml = r#"
type: sequential
name: test_chain
model: ollama/llama3.2:3b
temperature: 0.2
max_tokens: 1024
steps:
  - role: system
    content: "You are a test agent."
  - role: user
    content: "{user_message}"
tools:
  - name: whoami
    endpoint: whoami
    description: Get the current user
"#;
        let spec = serde_yaml::from_str::<ChainSpec>(yaml)
            .expect("must deserialise");
        assert_eq!(spec.chain_type, ChainType::Sequential);
        assert_eq!(spec.name, "test_chain");
        assert_eq!(spec.model, "ollama/llama3.2:3b");
        assert_eq!(spec.tools.len(), 1);
        assert_eq!(spec.tools[0].name, "whoami");
        spec.validate().expect("must be valid");
    }

    /// Graph chain with invalid edge reference fails validation.
    #[test]
    fn test_graph_chain_invalid_edge_fails_validation() {
        let spec = ChainSpec {
            chain_type:   ChainType::Graph,
            name:         "bad_graph".to_string(),
            description:  None,
            model:        default_model(),
            temperature:  0.0,
            max_tokens:   None,
            steps:        vec![],
            tools:        vec![],
            nodes:        vec![NodeSpec { id: "a".into(), node_type: NodeType::Llm, prompt: None, tool: None, merge_state: vec![] }],
            edges:        vec![EdgeSpec { from: "a".into(), to: vec!["ghost".into()] }],
            loop_budget:  4,
            token_budget: 4096,
        };
        let err = spec.validate().unwrap_err();
        assert!(err.contains("ghost"));
    }

    /// Empty name fails validation.
    #[test]
    fn test_empty_name_fails() {
        let spec = ChainSpec {
            chain_type:   ChainType::Sequential,
            name:         "".to_string(),
            description:  None,
            model:        default_model(),
            temperature:  0.0,
            max_tokens:   None,
            steps:        vec![],
            tools:        vec![],
            nodes:        vec![],
            edges:        vec![],
            loop_budget:  6,
            token_budget: 4096,
        };
        let err = spec.validate().unwrap_err();
        assert!(err.contains("name must not be empty"));
    }
}
