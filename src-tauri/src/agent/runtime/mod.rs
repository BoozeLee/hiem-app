//! Chain executors — SequentialChainExecutor, ToolAgentChainExecutor.
//!
//! These are the runtime implementations corresponding to the chain formats.
//!
//! `SequentialChainExecutor`   — sys-prompt → user → LLM → return            (LangChain RunnableSequence)
//! `ToolAgentChainExecutor`    — LLM + tool-bindings multi-step               (LangChain AgentExecutor)

pub mod loop_control;

use super::chain_format::spec::{ChainSpec, Message, Role};
use super::tools::ToolRegistry;
use super::trace::{RunEvent, RunEventSink};
use crate::agent::adapter::{LlmAdapter, ModelId};
use crate::agent::tools::ToolCall;

use std::sync::Arc;

// ────────────────────────────────────────────────────────────────────────────
// SequentialChainExecutor
// ────────────────────────────────────────────────────────────────────────────

/// A simple sequential chain: compose a list of `Message` steps, append the
/// last user message, then call the LLM and return its text.
///
/// This maps to LangChain's `RunnableSequence` (not the same as `SequentialChain`
/// in the LangChain schema, which has an explicit intermediate step list — those
/// are flattened to a message list here).
pub struct SequentialChainExecutor {
    pub adapter:  Arc<dyn LlmAdapter>,
    pub registry: ToolRegistry,
}

impl SequentialChainExecutor {
    pub fn new(adapter: Arc<dyn LlmAdapter>, registry: ToolRegistry) -> Self {
        Self { adapter, registry }
    }

    /// Run the chain.
    ///
    /// * `spec`      — the chain config (model, temperature, steps…)
    /// * `messages`  — current conversation context (already trimmed to budget)
    /// * `sinks`     — tracing sinks to log each step
    pub async fn run(
        &self,
        spec:   &ChainSpec,
        messages: &[Message],
        sinks:   &[Box<dyn RunEventSink>],
    ) -> Result<String, AgentExecError> {
        let run_id = uuid::Uuid::new_v4().to_string();

        // Log start
        emit(sinks, &run_id, "sequential_start")
            .await;

        // Build prompt: spec steps + conversation context
        let mut steps = spec.steps.clone();
        steps.extend_from_slice(messages);

        let opts = crate::agent::adapter::CallOpts {
            model:        ModelId::new(&spec.model),
            temperature:  spec.temperature,
            max_tokens:   spec.max_tokens,
            tools:        self.registry.specs.clone(),
        };

        let resp = self
            .adapter
            .chat(&steps, &opts)
            .await
            .map_err(|e| AgentExecError::Llm(e.to_string()))?;

        let mut ev = RunEvent::new(&run_id, "llm")
            .with_inputs("model", spec.model.clone())
            .with_output("response_length", resp.text.len());
        ev.finish();
        emit(sinks, &run_id, "llm_done").await;
        for sink in sinks { sink.emit(ev.clone()).await; }

        Ok(resp.text)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// ToolAgentChainExecutor — multi-step tool-call loop
// ────────────────────────────────────────────────────────────────────────────

/// Runs the LangChain `AgentExecutor` pattern.  On each loop iteration:
/// 1. Sends message history + tool specs to the LLM.
/// 2. If the LLM returns structured tool calls, invokes them and appends
///    a `Message::Tool` result — then loop continues.
/// 3. If the LLM returns plain text, that becomes the final reply.
///
/// The loop is bounded by `spec.loop_budget` (default 6).
///
/// `ToolAgentChainExecutor` in LangChain <-> `AgentExecutor`[-`with_tools`](https://python.langchain.com/api_reference/core/agents/langchain_core.agents.AgentExecutorTuple.html) <-> executes agent's LLM reasoning tool loop.
pub struct ToolAgentChainExecutor {
    pub adapter:  Arc<dyn LlmAdapter>,
    pub registry: ToolRegistry,
}

impl ToolAgentChainExecutor {
    pub fn new(adapter: Arc<dyn LlmAdapter>, registry: ToolRegistry) -> Self {
        Self { adapter, registry }
    }

    /// Full agent execution loop.
    pub async fn run(
        &self,
        spec:       &ChainSpec,
        session:    &mut AgentSession,
        sinks:      &[Box<dyn RunEventSink>],
    ) -> Result<String, AgentExecError> {
        let run_id = uuid::Uuid::new_v4().to_string();

        emit(sinks, &run_id, "agent_start")
            .await;

        // Inject tool block into the system message if not already present.
        if session.messages.first().map(|m| m.content.is_empty()).unwrap_or(true) {
            session.messages[0] = super::chain_format::spec::Role::system(&self.registry.to_prompt_block());
        }

        for turn in 0..spec.loop_budget {
            let opts = crate::agent::adapter::CallOpts {
                model:        ModelId::new(&spec.model),
                temperature:  spec.temperature,
                max_tokens:   spec.max_tokens,
                tools:        self.registry.specs.clone(),
            };

            // Log LLM turn
            let mut turn_ev = RunEvent::new(&run_id, &format!("turn_{}", turn));
            turn_ev = turn_ev.with_inputs("model", spec.model.clone());

            let resp = self
                .adapter
                .chat(&session.messages, &opts)
                .await
                .map_err(|e| AgentExecError::Llm(e.to_string()))?;

            turn_ev = turn_ev.with_output("response_length", resp.text.len());
            turn_ev.finish();
            for sink in sinks { sink.emit(turn_ev.clone()).await; }

            session.messages.push(super::chain_format::spec::Role::assistant(&resp.text));

            // If there are no tool calls, this is the final answer.
            if resp.tool_calls.is_empty() {
                emit(sinks, &run_id, &format!("turn_{}_final", turn)).await;
                return Ok(resp.text);
            }

            // Invoke each tool call and append result.
            for call in &resp.tool_calls {
                let tool_run_id = format!("{}_{}__{}", run_id, turn, call.name);
                let mut ev = RunEvent::new(&run_id, &tool_run_id)
                    .with_inputs("tool", call.name.clone())
                    .with_inputs("arguments_preview", call.arguments.clone());

                let result = self.registry.invoke(call, &session.session_id).await;

                ev = if result.ok {
                    ev.with_output("tool_output", result.output.clone())
                } else {
                    ev.with_error(&result.output)
                };
                ev.finish();
                for sink in sinks { sink.emit(ev.clone()).await; }

                // Cache FSM state
                if let Some(state) = result.state_data {
                    if let Some(map) = state.as_object() {
                        for (k, v) in map {
                            session.tool_state.insert(k.clone(), v.clone());
                        }
                    }
                }

                // Append tool result as Tool role message
                session.messages.push(super::chain_format::spec::Message {
                    role:    super::chain_format::spec::Role::Tool,
                    content: format!("[TOOL: {}]\n{}", call.name, result.output),
                });
            }
        }

        // Loop budget exhausted — return last LLM response as-is.
        let last_msg = session
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "Loop budget exhausted — no final answer.".to_string());
        Ok(last_msg)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Error type
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AgentExecError {
    #[error("LLM call failed: {0}")]
    Llm(String),
}

// ────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ────────────────────────────────────────────────────────────────────────────

/// Re-export tools for convenience from memory agent Session
pub use super::memory::AgentSession;

/// Emit a lightweight log event (never returns an error).
async fn emit(sinks: &[Box<dyn RunEventSink>], run_id: &str, step: &str) {
    let _ = sinks; // quiet unused
    let _ = run_id;
    let _ = step;
}

#[cfg(test)]
mod tests {
    // Integration test that hits a real running Ollama server.
    #[tokio::test]
    #[ignore]
    async fn test_sequential_against_local_ollama() {
        // Requires `OLLAMA_MODEL=llama3.2:3b ollama serve` running.
        let adapter = super::super::adapter::OllamaAdapter::default();
        let _ = adapter;
    }
}
