//! HIEM agent runtime — LangChain / LangGraph-compatible.
//!
//! ```
//! src-tauri/src/agent/
//!   chain_format/   ChainSpec ser/de, YAML/JSON, LangGraph builder types
//!   adapter/        LlmAdapter trait · Ollama · HF Space stubs
//!   tools/          Tool trait · ToolRegistry · built-in GitHub tools
//!   runtime/        SequentialChainExecutor · ToolAgentChainExecutor
//!   memory/         AgentSessionStore · TokenWindow · FSM state map
//!   rag/            BM25 index · DocumentChunk · LlamaIndex-compatible format
//!   trace/          RunEvent · RunEventSink (file · stdout · LangSmith)
//!   loop_control.rs bounded tool-call loop (LangChain AgentExecutor pattern)
//! ```

#![allow(unused_imports)]

pub mod adapter;
pub mod chain_format;
pub mod memory;
pub mod output;
pub mod rag;
pub mod runtime;
pub mod tools;
pub mod trace;
