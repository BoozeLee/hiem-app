//! Structured output: parse tool-call blobs and JSON schemas.
//!
//! Moves the `isGitHubToolCall` / `isGitHubJSON` / `renderGitHubJSON`
//! logic from `ChatPage.tsx` into Rust so the agent brain also emits typed
//! tool-call messages in the LangChain multi-turn function-call format.

use serde::Deserialize;
use serde_json::Value;

// ────────────────────────────────────────────────────────────────────────────
// Parsing
// ────────────────────────────────────────────────────────────────────────────

/// Parse a chat message `content` that may contain an embedded tool-call blob.
///
/// LangChain function-call format (parsed from the LLM's text output):
///
/// ```text
/// [TOOL: get_prs]
/// {"owner": "tokio-rs", "repo": "tokio"}
/// ```
pub fn parse_tool_call(content: &str) -> (String, Option<ToolCallStmt>) {
    match find_tool_block(content) {
        None => (content.to_string(), None),
        Some((prefix, name, json_block)) => {
            let clean = prefix.trim_end().to_string();
            match serde_json::from_str::<Value>(json_block) {
                Ok(val) => (
                    clean,
                    Some(ToolCallStmt { name, arguments: val }),
                ),
                Err(_) => (content.to_string(), None),
            }
        }
    }
}

/// Out-of-band tool-call block inside a message.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallStmt {
    pub name:      String,
    pub arguments: Value,
}

impl ToolCallStmt {
    /// Convert to the `ToolCall` shape consumed by the runtime.
    pub fn to_runtime_call(&self) -> super::tools::ToolCall {
        super::tools::ToolCall {
            name:      self.name.clone(),
            arguments: self.arguments.to_string(),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

fn find_tool_block(content: &str) -> Option<(&str, String, &str)> {
    let start = content.find("[TOOL: ")?;
    let after = &content[start + "[TOOL: ".len()..];
    let name_end = after.find(']')?;
    let name = after[..name_end].trim().to_string();
    let json_start = after.get(name_end + 1..)?.trim_start();
    // JSON block may be on the next line or immediately after ']'
    let json_start = if json_start.starts_with('\n') { &json_start[1..] } else { json_start };
    Some((&content[..start], name, json_start.trim()))
}

// ────────────────────────────────────────────────────────────────────────────
// JSON Schema / Tool-Spec builder helpers
// ────────────────────────────────────────────────────────────────────────────

/// Build a `{"type": "object", "properties": {...}}` JSON Schema for a tool.
pub fn object_schema(properties: Vec<(&str, &str)>) -> serde_json::Value {
    let mut props = serde_json::Map::new();
    for (name, ty) in properties {
        props.insert(name.to_string(), serde_json::json!({ "type": ty }));
    }
    serde_json::json!({ "type": "object", "properties": props })
}

// ────────────────────────────────────────────────────────────────────────────
// Message formatting helpers
// ────────────────────────────────────────────────────────────────────────────

/// Format a ToolResult as the `[TOOL: name]\n{json}` block the ChatPage
/// frontend marker-parser already handles.
pub fn format_tool_result(result: &super::tools::ToolResult) -> String {
    let json = serde_json::json!({
        "tool":  result.tool_name,
        "ok":    result.ok,
        "output": result.output,
    });
    format!("[TOOL: {}]\n{}\n", result.tool_name, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_call_blob() {
        let content = "I will check that now.\n[TOOL: whoami]\n{}\n";
        let (rest, tc) = parse_tool_call(content);
        assert!(rest.contains("I will check that now"));
        let tc = tc.expect("must parse");
        assert_eq!(tc.name, "whoami");
        assert_eq!(tc.arguments, Value::Object(serde_json::Map::new()));
    }

    #[test]
    fn test_parse_no_tool_call() {
        let (rest, tc) = parse_tool_call("just a message");
        assert_eq!(rest, "just a message");
        assert!(tc.is_none());
    }

    #[test]
    fn test_format_tool_result() {
        let result = super::tools::ToolResult {
            tool_name:  "whoami".into(),
            ok:         true,
            output:     "alice".into(),
            state_data: None,
        };
        let formatted = format_tool_result(&result);
        assert!(formatted.starts_with("[TOOL: whoami]"));
        assert!(formatted.contains("alice"));
    }
}
