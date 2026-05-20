//! GraphSpec extensions — LangGraph JSON-compatible node/edge topology.
//!
//! LangGraph's `StateGraph` maps 1-to-1 to these types:
//!   nodes[]   — work units in the graph
//!   edges[]   — directed edges between nodes
//!   loop_budget — the recursion_limit analogue

use super::spec::{ChainSpec, NodeSpec, EdgeSpec, NodeType};

// GraphSpec is embedded inside ChainSpec (nodes / edges / loop_budget fields).
// This module provides helpers for building and mutating graphs programmatically
// — for the frontend ChainGraph visualiser and runtime executor alike.

impl ChainSpec {
    /// Return a new `Graph`-type chain skeleton with the given name.
    pub fn graph(name: impl Into<String>) -> Self {
        Self {
            chain_type:  crate::agent::chain_format::spec::ChainType::Graph,
            name:        name.into(),
            description: None,
            model:       super::spec::default_model(),
            temperature: super::spec::default_temperature(),
            max_tokens:  None,
            steps:       vec![],
            tools:       vec![],
            nodes:       vec![],
            edges:       vec![],
            loop_budget: super::spec::default_loop_budget(),
            token_budget: super::spec::default_token_budget(),
        }
    }

    /// Add a node (returns `self` for builder chaining).
    pub fn with_node(mut self, node: NodeSpec) -> Self {
        self.nodes.push(node);
        self
    }

    /// Add an edge.
    pub fn with_edge(mut self, edge: EdgeSpec) -> Self {
        self.edges.push(edge);
        self
    }
}

impl NodeSpec {
    /// Create an LLM node with a system-prompt prompt.
    pub fn llm(id: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            id:        id.into(),
            node_type: NodeType::Llm,
            prompt:    Some(prompt.into()),
            tool:      None,
            merge_state: vec![],
        }
    }

    /// Create a tool-call node that delegates to a registered ToolRegistry key.
    pub fn toolset(id: impl Into<String>) -> Self {
        Self {
            id:        id.into(),
            node_type: NodeType::Toolset,
            prompt:    None,
            tool:      None,
            merge_state: vec![],
        }
    }

    /// Create a condition/switch node.
    pub fn condition(id: impl Into<String>) -> Self {
        Self {
            id:        id.into(),
            node_type: NodeType::Condition,
            prompt:    None,
            tool:      None,
            merge_state: vec![],
        }
    }

    /// Large language model node that calls the configured adapter with a prompt.
    pub fn passthrough(id: impl Into<String>) -> Self {
        Self {
            id:        id.into(),
            node_type: NodeType::Passthrough,
            prompt:    None,
            tool:      None,
            merge_state: vec![],
        }
    }
}

impl EdgeSpec {
    pub fn new(from: impl Into<String>, to: Vec<impl Into<String>>) -> Self {
        Self { from: from.into(), to: to.into_iter().map(|t| t.into()).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_builder_api() {
        let g = ChainSpec::graph("test_planner")
            .with_node(NodeSpec::llm("plan", "Generate a plan for: {query}"))
            .with_node(NodeSpec::toolset("tools"))
            .with_node(NodeSpec::condition("decide"))
            .with_edge(EdgeSpec::new("plan", vec!["tools", "decide"]))
            .with_edge(EdgeSpec::new("tools", vec!["decide"]));

        assert_eq!(g.chain_type, crate::agent::chain_format::spec::ChainType::Graph);
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.loop_budget, 6);
        g.validate().expect("builder produces valid chain");
    }
}
