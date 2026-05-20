//! In-memory RAG index — simple keyword search.
//!
//! Uses a simple text search approach. For production use, consider
//! integrating with a proper search engine like Tantivy or Meilisearch.

use serde::{Deserialize, Serialize};

// ────────────────────────────────────────────────────────────────────────────
// LlamaIndex-compatible DocumentChunk
// ────────────────────────────────────────────────────────────────────────────

/// A text chunk with metadata — compatible with LlamaIndex `TextNode` JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub id:         String,
    pub text:       String,
    #[serde(default)]
    pub metadata:   serde_json::Map<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding:  Option<Vec<f32>>,
}

// ────────────────────────────────────────────────────────────────────────────
// SimpleDocumentIndex — basic text search
// ────────────────────────────────────────────────────────────────────────────

/// Simple document index for keyword search.
pub struct DocumentIndex {
    chunks: Vec<DocumentChunk>,
}

impl Default for DocumentIndex {
    fn default() -> Self {
        Self { chunks: vec![] }
    }
}

impl DocumentIndex {
    pub fn new() -> Self { Self::default() }

    pub fn add_document(&mut self, chunk: DocumentChunk) -> usize {
        let id = self.chunks.len();
        self.chunks.push(chunk);
        id
    }

    pub fn build(&mut self) {
    }

    pub fn search(&self, query: &str, k: usize) -> Vec<(&DocumentChunk, f32)> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<(&DocumentChunk, f32)> = self.chunks
            .iter()
            .filter_map(|chunk| {
                let matches = chunk.text.to_lowercase().matches(&query_lower).count();
                if matches > 0 {
                    let score = matches as f32 / query_lower.split_whitespace().count().max(1) as f32;
                    Some((chunk, score))
                } else {
                    None
                }
            })
            .collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.into_iter().take(k).collect()
    }

    pub fn len(&self) -> usize { self.chunks.len() }
    pub fn is_empty(&self) -> bool { self.chunks.is_empty() }
}

// ────────────────────────────────────────────────────────────────────────────
// Loader helpers (Markdown reader)
// ────────────────────────────────────────────────────────────────────────────

/// Chunk a large text into ~512-char slices with overlap.
pub fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    if text.len() <= chunk_size {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let end = (start + chunk_size).min(text.len());
        chunks.push(text[start..end].to_string());
        start = end.saturating_sub(overlap);
    }
    chunks
}

/// Load a README or Markdown file as a series of chunks.
pub fn load_markdown_file(path: &str) -> Result<Vec<DocumentChunk>, String> {
    let raw  = std::fs::read(path)
        .map_err(|e| format!("cannot read {}: {}", path, e))?;
    let text = String::from_utf8(raw).map_err(|e| format!("not valid UTF-8: {}", e))?;
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let chunks = chunk_text(&text, 2000, 200);
    Ok(chunks
        .into_iter()
        .enumerate()
        .map(|(i, c)| DocumentChunk {
            id: format!("{}_{:04}", filename, i),
            text: c,
            metadata: {
                let mut m = serde_json::Map::new();
                m.insert("source".to_string(), serde_json::json!(filename));
                m.insert("source_type".to_string(), serde_json::json!("markdown"));
                m.insert("chunk_index".to_string(), serde_json::json!(i));
                m
            },
            embedding: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_small() {
        let c = chunk_text("hello world", 50, 10);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0], "hello world");
    }

    #[test]
    fn test_chunk_text_overlap() {
        // 200 chars, chunk_size=80 → 3 chunks (80+80+40), overlap=10
        let text = "A".repeat(200);
        let chunks = chunk_text(&text, 80, 10);
        assert!(chunks.len() >= 3);
    }

    #[test]
    fn test_document_index_search_returns_results() {
        let mut idx = DocumentIndex::new();
        idx.add_document(DocumentChunk {
            id:       "c0".into(),
            text:     "The get_prs endpoint returns pull requests.".into(),
            metadata: Default::default(),
            embedding: None,
        });
        idx.add_document(DocumentChunk {
            id:       "c1".into(),
            text:     "The whoami command returns the current user.".into(),
            metadata: Default::default(),
            embedding: None,
        });
        idx.build();
        let results = idx.search("pull requests", 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].0.text.contains("pull requests"));
    }
}
