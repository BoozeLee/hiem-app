//! Tool trait + ToolRegistry.
//!
//! Mirrors LangChain's `StructuredTool` and `ToolNode` — a tool is a struct
//! with a name, description, JSON Schema for its parameters, and an `invoke`
//! method.  The registry holds all registered tools and dispatches by name.

use serde::{Deserialize, Serialize};

// Re-export the LlmAdapter types from the parent adapter module.
pub use super::adapter::ToolSpec;

// ────────────────────────────────────────────────────────────────────────────
// ToolCall — the runtime representation of an LLM tool-call turn
// ────────────────────────────────────────────────────────────────────────────

/// A tool invocation request produced by the LLM or submitted programmatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name — must be a key in [`ToolRegistry`].
    pub name:      String,
    /// Raw argument JSON string.  Deserialise against the tool's schema.
    pub arguments: String,
}

// ────────────────────────────────────────────────────────────────────────────
// ToolResult — what ToolRegistry::invoke returns
// ────────────────────────────────────────────────────────────────────────────

/// Outcome of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name:  String,
    pub ok:         bool,
    /// Human-readable output (shown in chat and inspector).
    pub output:     String,
    /// Structured data returned to the FSM (cached in ToolStateMap).
    pub state_data: Option<serde_json::Value>,
}

// ────────────────────────────────────────────────────────────────────────────
// Tool trait
// ────────────────────────────────────────────────────────────────────────────

/// A stateless, serialisable tool the agent can invoke.
///
/// Each tool also produces a [`ToolSpec`] consumed by the LLM adapter as a
/// function-call schema.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Stable key for [`ToolRegistry`] dispatch.
    fn name(&self) -> &'static str;

    /// One-line description for the LLM system prompt.
    fn description(&self) -> &'static str;

    /// JSON Schema for the argument object.  Return `serde_json::json!({})`
    /// for a no-argument tool.
    fn schema(&self) -> serde_json::Value;

    /// Invoke the tool with a JSON argument string.
    ///
    /// Arguments are pre-validated against [`Self::schema`] by the caller.
    async fn invoke(&self, args: &str) -> ToolResult;
}

// ────────────────────────────────────────────────────────────────────────────
// ToolRegistry
// ────────────────────────────────────────────────────────────────────────────

/// Dispatch table mapping tool names → tool implementations.
///
/// All GitHub API tools live here.  Third-party tools (RAG, web-search, etc.)
/// register the same way.
///
/// # Example
/// ```
/// # use hiem_tools::ToolRegistry;
/// let registry = ToolRegistry::default();
/// // registry.get("whoami") is always available
/// assert!(registry.get("whoami").is_some());
/// ```
pub struct ToolRegistry {
    tools:    std::collections::HashMap<&'static str, Box<dyn Tool>>,
    pub specs: Vec<ToolSpec>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        let mut reg = Self { tools: Default::default(), specs: vec![] };
        reg.register(WhoAmI);
        reg.register(GetRepos);
        reg.register(GetPrs);
        reg.register(GetIssues);
        reg.register(GetBranches);
        reg
    }
}

impl ToolRegistry {
    /// Register a tool and add its `ToolSpec` to the LLM-facing spec list.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        let name    = tool.name();
        let schema  = ToolSpec {
            name:        name.to_string(),
            description: tool.description().to_string(),
            parameters:  tool.schema(),
        };
        self.specs.push(schema);
        self.tools.insert(name, Box::new(tool));
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|b| &**b)
    }

    /// Dispatch a tool call: parse args → validate → invoke.
    pub async fn invoke(
        &self,
        call:      &ToolCall,
        session_id: &str,
    ) -> ToolResult {
        let Some(tool) = self.get(&call.name) else {
            return ToolResult {
                tool_name:  call.name.clone(),
                ok:         false,
                output:     format!("ERROR: unknown tool '{}'", call.name),
                state_data: None,
            };
        };

        // Basic JSON parse validation
        match serde_json::from_str::<serde_json::Value>(&call.arguments) {
            Ok(args) => {
                // Pass the serialised JSON through to the tool.
                // Individual tool impls are responsible for extracting fields.
                let result = tool.invoke(&call.arguments).await;
                // Attach resolved args to state_data so FSM can use them.
                ToolResult {
                    tool_name:  call.name.clone(),
                    ok:         result.ok,
                    output:     result.output,
                    state_data: Some(serde_json::json!({
                        "session_id": session_id,
                        "arguments":  args,
                    })),
                }
            }
            Err(e) => ToolResult {
                tool_name:  call.name.clone(),
                ok:         false,
                output:     format!("ERROR: invalid JSON arguments for '{}': {}", call.name, e),
                state_data: None,
            },
        }
    }

    /// Return all tool names for iteration.
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().copied().collect()
    }

    /// Convert specs to a JSON string for the LLM system prompt.
    pub fn to_prompt_block(&self) -> String {
        let mut buf = String::from("You have access to the following tools:\n\n");
        for spec in &self.specs {
            buf.push_str(&format!("- **{}**: {}\n", spec.name, spec.description));
        }
        buf.push_str(
            "\nWhen you need to use a tool, respond with the following exact format:\n"
        );
        buf.push_str("[TOOL: name]\n{json_arguments}\n");
        buf
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Built-in tool: Whoami
// ────────────────────────────────────────────────────────────────────────────

/// Invokes Tauri command `whoami` with the current session token.
#[derive(Debug, Clone, Default)]
pub struct WhoAmI;

#[async_trait::async_trait]
impl Tool for WhoAmI {
    fn name(&self) -> &'static str { "whoami" }

    fn description(&self) -> &'static str {
        "Get the currently authenticated GitHub user (login and avatar)."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn invoke(&self, _args: &str) -> ToolResult {
        ToolResult {
            tool_name:  "whoami".to_string(),
            ok:         false,
            output:     "whoami is an external Rust Tauri command — call via chat().".to_string(),
            state_data: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Built-in tool: GetRepos
// ────────────────────────────────────────────────────────────────────────────

/// Invokes Tauri command `get_repos` with the current session token.
#[derive(Debug, Clone, Default)]
pub struct GetRepos;

#[async_trait::async_trait]
impl Tool for GetRepos {
    fn name(&self) -> &'static str { "get_repos" }

    fn description(&self) -> &'static str {
        "List all repositories accessible to the current GitHub token."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn invoke(&self, _args: &str) -> ToolResult {
        ToolResult {
            tool_name:  "get_repos".to_string(),
            ok:         false,
            output:     "get_repos is an external Rust Tauri command — call via chat().".to_string(),
            state_data: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Built-in tool: GetPrs
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GetPrs;

#[async_trait::async_trait]
impl Tool for GetPrs {
    fn name(&self) -> &'static str { "get_prs" }
    fn description(&self) -> &'static str { "List open pull requests for an owner/repo." }
    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo":  { "type": "string" }
            },
            "required": ["owner", "repo"]
        })
    }
    async fn invoke(&self, args: &str) -> ToolResult {
        let v: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return ToolResult { tool_name: "get_prs".into(), ok: false, output: format!("Invalid JSON: {}", e), state_data: None },
        };
        let owner = v["owner"].as_str().unwrap_or("");
        let repo = v["repo"].as_str().unwrap_or("");
        ToolResult {
            tool_name: "get_prs".into(),
            ok: false,
            output: format!("get_prs({}, {}) is a Tauri command — call via chat().", owner, repo),
            state_data: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Built-in tool: GetIssues
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GetIssues;

#[async_trait::async_trait]
impl Tool for GetIssues {
    fn name(&self) -> &'static str { "get_issues" }
    fn description(&self) -> &'static str { "List open issues for an owner/repo." }
    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo":  { "type": "string" }
            },
            "required": ["owner", "repo"]
        })
    }
    async fn invoke(&self, args: &str) -> ToolResult {
        let v: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return ToolResult { tool_name: "get_issues".into(), ok: false, output: format!("Invalid JSON: {}", e), state_data: None },
        };
        let owner = v["owner"].as_str().unwrap_or("");
        let repo = v["repo"].as_str().unwrap_or("");
        ToolResult {
            tool_name: "get_issues".into(),
            ok: false,
            output: format!("get_issues({}, {}) is a Tauri command — call via chat().", owner, repo),
            state_data: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Built-in tool: GetBranches
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GetBranches;

#[async_trait::async_trait]
impl Tool for GetBranches {
    fn name(&self) -> &'static str { "get_branches" }
    fn description(&self) -> &'static str { "List branches for an owner/repo." }
    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo":  { "type": "string" }
            },
            "required": ["owner", "repo"]
        })
    }
    async fn invoke(&self, args: &str) -> ToolResult {
        let v: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return ToolResult { tool_name: "get_branches".into(), ok: false, output: format!("Invalid JSON: {}", e), state_data: None },
        };
        let owner = v["owner"].as_str().unwrap_or("");
        let repo = v["repo"].as_str().unwrap_or("");
        ToolResult {
            tool_name: "get_branches".into(),
            ok: false,
            output: format!("get_branches({}, {}) is a Tauri command — call via chat().", owner, repo),
            state_data: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unknown_tool_returns_error() {
        let reg = ToolRegistry::default();
        let result = reg.invoke(&ToolCall { name: "bogus".into(), arguments: "{}".into() }, "test").await;
        assert!(!result.ok);
        assert!(result.output.contains("unknown tool"));
    }
}
