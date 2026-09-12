//! # 通用文档域适配器
//!
//! 按标题（Markdown #/##/###）或段落拆分切片。

use crate::sutra_library::domain::{DomainParser, DomainTokenizer};
use crate::sutra_library::models::*;
use anyhow::Result;

pub struct GeneralDomainParser;

impl Default for GeneralDomainParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GeneralDomainParser {
    pub fn new() -> Self {
        Self
    }
}

impl DomainParser for GeneralDomainParser {
    fn domain_name(&self) -> &str {
        "general"
    }

    fn split_semantic_chunks(&self, raw: &str, _ctx: &ParseContext) -> Vec<SemanticChunk> {
        let mut chunks = Vec::new();
        let mut current_title = String::from("root");
        let mut current_content = String::new();
        let mut sort_order = 0u32;

        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("# ")
                || trimmed.starts_with("## ")
                || trimmed.starts_with("### ")
            {
                // 保存上一个 chunk
                if !current_content.is_empty() {
                    chunks.push(SemanticChunk {
                        title: current_title.clone(),
                        content: current_content.trim().to_string(),
                        node_type: "document_section".to_string(),
                        parent_path: None,
                        sort_order,
                        metadata: serde_json::json!({}),
                    });
                    sort_order += 1;
                }
                current_title = trimmed.trim_start_matches('#').trim().to_string();
                current_content = String::new();
            } else {
                if !current_content.is_empty() {
                    current_content.push('\n');
                }
                current_content.push_str(line);
            }
        }

        // 最后一个 chunk
        if !current_content.is_empty() {
            chunks.push(SemanticChunk {
                title: current_title,
                content: current_content.trim().to_string(),
                node_type: "document_section".to_string(),
                parent_path: None,
                sort_order,
                metadata: serde_json::json!({}),
            });
        }

        // 如果没有标题，整个作为一段
        if chunks.is_empty() && !raw.is_empty() {
            chunks.push(SemanticChunk {
                title: "document".to_string(),
                content: raw.to_string(),
                node_type: "document".to_string(),
                parent_path: None,
                sort_order: 0,
                metadata: serde_json::json!({}),
            });
        }

        chunks
    }

    fn enrich_chunk(&self, chunk: &SemanticChunk) -> (String, serde_json::Value) {
        let summary = chunk.content.chars().take(100).collect::<String>();
        let metadata = serde_json::json!({
            "char_count": chunk.content.len(),
            "word_count": chunk.content.split_whitespace().count(),
        });
        (summary, metadata)
    }

    fn extract_edges(&self, _nodes: &[MemoryNode]) -> Vec<RefEdge> {
        Vec::new()
    }

    fn incremental_parse(&self, _old: &MemoryNode, _diff: &str) -> Result<MemoryNode> {
        anyhow::bail!("通用域不支持增量解析")
    }

    fn format_context(&self, node: &MemoryNode, _mode: ContextMode) -> String {
        format!("[{}] {}\n  {}", node.node_type, node.title, node.summary)
    }

    fn compress_nodes(&self, nodes: &[MemoryNode]) -> MemoryNode {
        let title = nodes.first().map(|n| n.title.clone()).unwrap_or_default();
        let content = nodes
            .iter()
            .map(|n| format!("## {}\n{}", n.title, n.summary))
            .collect::<Vec<_>>()
            .join("\n\n");
        let summary = content.chars().take(200).collect::<String>();
        MemoryNode {
            title: title.clone(),
            content: content.clone(),
            summary,
            ..nodes
                .first()
                .cloned()
                .unwrap_or_else(|| panic!("compress_nodes: empty input"))
        }
    }

    fn rerank_nodes(&self, _query: &str, _candidates: &mut Vec<ScoredNode>) {
        // 通用域不做特殊重排序
    }
}

pub struct GeneralDomainTokenizer;

impl Default for GeneralDomainTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GeneralDomainTokenizer {
    pub fn new() -> Self {
        Self
    }
}

impl DomainTokenizer for GeneralDomainTokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn stop_words(&self) -> &[&str] {
        &[
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "could", "should", "may", "might",
            "shall", "can", "to", "of", "in", "for", "on", "with", "at", "by", "from", "as",
            "into", "through", "during", "before", "after", "above", "below", "between", "out",
            "off", "over", "under", "again", "further", "then", "once", "here", "there", "when",
            "where", "why", "how", "all", "each", "every", "both", "few", "more", "most", "other",
            "some", "such", "no", "nor", "not", "only", "own", "same", "so", "than", "too", "very",
            "just", "because", "and", "but", "or", "if", "while", "that", "this", "these", "those",
            "it", "its", "i", "me", "my", "we", "our", "you", "your", "he", "him", "his", "she",
            "her", "they", "them", "their", "what", "which", "who", "whom", "about",
        ]
    }

    fn expand_query(&self, query: &str) -> Vec<String> {
        vec![query.to_string()]
    }
}
