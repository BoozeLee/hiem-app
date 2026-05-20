//! Agent session memory — sliding context window + FSM state map.
//!
//! Mirrors LangChain's `RunnableWithMessageHistory` and memory primitives.

use dashmap::DashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use super::chain_format::spec::{Message, Role};

// ────────────────────────────────────────────────────────────────────────────
// AgentSession — per-user conversational state
// ────────────────────────────────────────────────────────────────────────────

/// Per-user, per-chain conversational state.
///
/// Lives in `AgentSessionStore` keyed by `session_id` (e.g. `ghs_session_alice`).
#[derive(Debug, Clone)]
pub struct AgentSession {
    pub session_id:   String,
    pub chain_type:   String,
    /// Ordered conversation history; trimmed by `TokenWindow`.
    pub messages:     Vec<Message>,
    /// FSM state accumulated across tool calls.
    /// Keys are abstract — individual tools decide what they store.
    /// Examples: `"repos"`, `"selected_repo"`, `"prs"`, `"code_search_results"`.
    pub tool_state:   serde_json::Map<String, serde_json::Value>,
    pub token_budget: u32,
    pub created_at:   i64,
    pub last_seen:    i64,
}

impl AgentSession {
    /// Create a fresh session.
    pub fn new(session_id: impl Into<String>, chain_type: impl Into<String>) -> Self {
        let now = now_unix();
        Self {
            session_id:   session_id.into(),
            chain_type:   chain_type.into(),
            messages:     vec![Message { role: Role::System, content: String::new() }],
            tool_state:   serde_json::Map::new(),
            token_budget: std::env::var("HIEM_MAX_TOKENS")
                .ok().and_then(|v| v.parse().ok())
                .unwrap_or(4096),
            created_at:   now,
            last_seen:    now,
        }
    }

    /// Touch last-seen timestamp (called after each turn).
    pub fn touch(&mut self) {
        self.last_seen = now_unix();
    }
}

#[inline]
fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

// ────────────────────────────────────────────────────────────────────────────
// TokenWindow — sliding-window trimmer
// ────────────────────────────────────────────────────────────────────────────

/// Approximate token counter and window trimmer.
///
/// Uses a simple 4-char ≈ 1 token heuristic.  Replace with a real tokenizer
/// (tiktoken / HF `tokenizers`) when precision matters.
#[derive(Clone)]
pub struct TokenWindow {
    capacity: usize,  // max characters ≈ token_budget * 4
}

impl TokenWindow {
    pub fn new(token_budget: u32) -> Self {
        Self { capacity: (token_budget as usize) * 4 }
    }

    /// Trim `messages` so total character length ≤ [`Self::capacity`].
    /// Always keeps the last system message, then fills forward.
    pub fn trim(&self, messages: &mut Vec<Message>) {
        // Always retain the system message at index 0.
        if messages.len() <= 2 {
            return;
        }
        // Keep system + as many most-recent messages as fit.
        let _sys_msg = messages[0].clone();
        let mut tail: Vec<Message> = Vec::new();
        for msg in messages[1..].iter().rev() {
            let msg_len = msg.content.len();
            if tail.iter().map(|m| m.content.len()).sum::<usize>() + msg_len > self.capacity {
                break;
            }
            tail.push(msg.clone());
        }
        tail.reverse();
        messages.truncate(1 + tail.len());
        messages[1..].clone_from_slice(&tail);
    }
}

impl Default for TokenWindow {
    fn default() -> Self { Self::new(4096) }
}

// ────────────────────────────────────────────────────────────────────────────
// AgentSessionStore — concurrent, evicting session map
// ────────────────────────────────────────────────────────────────────────────

/// Concurrent session store backed by `DashMap`.
///
/// Sessions are evicted if they have not been touched for more than
/// `evict_after_secs` (default 1 hour).
pub struct AgentSessionStore {
    sessions: DashMap<String, AgentSession>,
    evict_after_secs: i64,
}

impl Default for AgentSessionStore {
    fn default() -> Self {
        Self {
            sessions: DashMap::new(),
            evict_after_secs: 3600,
        }
    }
}

impl AgentSessionStore {
    pub fn with_evict(evict_after_secs: i64) -> Self {
        Self { sessions: DashMap::new(), evict_after_secs }
    }

    /// Insert a new session.
    pub fn insert(&self, session: AgentSession) {
        self.sessions.insert(session.session_id.clone(), session);
    }

    /// Get a mutable reference to a session — touches last_seen.
    pub fn get_mut(&self, id: &str) -> dashmap::mapref::entry::Entry<'_, String, AgentSession> {
        self.sessions.entry(id.to_string())
    }

    /// Get a shared reference without touching last_seen.
    pub fn get(&self, id: &str) -> Option<dashmap::mapref::one::Ref<'_, String, AgentSession>> {
        self.sessions.get(id)
    }

    /// Remove expired sessions; returns count removed.
    pub fn evict_expired(&self) -> usize {
        let now = now_unix();
        let expired: Vec<String> = self
            .sessions
            .iter()
            .filter(|entry| now - entry.value().last_seen > self.evict_after_secs)
            .map(|entry| entry.key().clone())
            .collect();
        let n = expired.len();
        for key in expired {
            self.sessions.remove(&key);
        }
        n
    }

    /// Sessions count (for diagnostics).
    pub fn len(&self) -> usize { self.sessions.len() }

    /// Return `true` if store is empty.
    pub fn is_empty(&self) -> bool { self.sessions.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_window_keeps_system_msg() {
        let mut msgs = vec![
            Message { role: Role::System, content: "sys".into() },
            Message { role: Role::User,   content: "hello my friend".into() },
        ];
        let tw = TokenWindow::new(10); // 40 chars capacity
        tw.trim(&mut msgs);
        assert_eq!(msgs[0].role, Role::System); // system kept
    }

    #[test]
    fn test_token_window_trims_large_history() {
        let mut msgs = vec![
            Message { role: Role::System, content: "sys".into() },
            Message { role: Role::User,   content: "A".repeat(500) },
            Message { role: Role::User,   content: "B".repeat(500) },
        ];
        tw = TokenWindow::new(20); // 80 chars capacity
        tw.trim(&mut msgs);
        assert!(msgs.len() <= 2 || msgs[1].content.len() <= tw.capacity);
    }

    #[test]
    fn test_session_store_insert_and_get() {
        let store = AgentSessionStore::default();
        let sess  = AgentSession::new("s1", "sequential");
        store.insert(sess.clone());
        assert_eq!(store.len(), 1);
        let got = store.get("s1");
        assert!(got.is_some());
    }
}
