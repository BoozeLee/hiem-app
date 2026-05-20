//! Bounded agent loop executor.
//!
//! Implements the LangChain `AgentExecutor` pattern:
//! LLM → (tool_calls? → invoke → append results → repeat) until loop_budget exhausted.

use super::{AgentExecError, SequentialChainExecutor, ToolAgentChainExecutor};
use super::super::chain_format::spec::ChainSpec;
use super::super::memory::AgentSession;
use super::super::tools::ToolRegistry;
use super::super::trace::{RunEventSink, default_sinks};
use super::super::adapter::{LlmAdapter, ModelId, CallOpts, LlmResponse, AdapterError};
use crate::agent::tools::ToolCall;

use std::sync::Arc;

/// Bounded tool-call loop executor.
///
/// This is the main entry point for agent execution. It runs up to `loop_budget`
/// iterations of: LLM call → tool invocation → result append → repeat.
pub struct AgentExecutor {
    adapter: Arc<dyn LlmAdapter>,
    registry: ToolRegistry,
}

impl AgentExecutor {
    pub fn new(adapter: Arc<dyn LlmAdapter>, registry: ToolRegistry) -> Self {
        Self { adapter, registry }
    }

    /// Execute a chain with tool-calling loop.
    ///
    /// This is the LangChain `AgentExecutor` pattern:
    /// 1. Send message history + tool specs to LLM
    /// 2. If LLM returns tool_calls, invoke them and append results
    /// 3. Repeat until no tool_calls or loop_budget exhausted
    pub async fn run(
        &self,
        spec: &ChainSpec,
        mut session: AgentSession,
        sinks: &[Box<dyn RunEventSink>],
    ) -> Result<String, AgentExecError> {
        let run_id = uuid::Uuid::new_v4().to_string();
        self.emit(sinks, &run_id, "agent_loop_start").await;

        for turn in 0..spec.loop_budget {
            let opts = CallOpts {
                model: ModelId::new(&spec.model),
                temperature: spec.temperature,
                max_tokens: spec.max_tokens,
                tools: self.registry.specs.clone(),
            };

            let resp = self
                .adapter
                .chat(&session.messages, &opts)
                .await
                .map_err(|e| AgentExecError::Llm(e.to_string()))?;

            let turn_ev = super::super::trace::RunEvent::new(&run_id, &format!("turn_{}", turn))
                .with_inputs("model", spec.model.clone())
                .with_output("response_length", resp.text.len());
            for sink in sinks { sink.emit(turn_ev.clone()).await; }

            session.messages.push(crate::agent::chain_format::spec::Role::assistant(&resp.text));

            if resp.tool_calls.is_empty() {
                self.emit(sinks, &run_id, &format!("turn_{}_complete", turn)).await;
                return Ok(resp.text);
            }

            for call in &resp.tool_calls {
                let tool_result = self.invoke_tool(call, &session.session_id).await;
                let result_content = format!("[TOOL: {}]\n{}", call.name, tool_result.output);
                session.messages.push(crate::agent::chain_format::spec::Role::tool(&result_content));
            }
        }

        let last_msg = session
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "Loop budget exhausted — no final answer.".to_string());
        Ok(last_msg)
    }

    async fn invoke_tool(&self, call: &ToolCall, session_id: &str) -> crate::agent::tools::ToolResult {
        self.registry.invoke(call, session_id).await
    }

    async fn emit(&self, sinks: &[Box<dyn RunEventSink>], run_id: &str, step: &str) {
        let _ = (sinks, run_id, step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::adapter::OllamaAdapter;

    #[test]
    fn test_executor_can_be_built() {
        let adapter = Arc::new(OllamaAdapter::default());
        let registry = ToolRegistry::default();
        let _executor = AgentExecutor::new(adapter, registry);
    }
}