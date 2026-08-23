//! # Rust 编程域适配器
//!
//! 使用 tree-sitter 解析 Rust 代码，提取模块 → 结构体 → 函数 → 方法 → 变量。

use crate::sutra_library::domain::{DomainParser, DomainTokenizer};
use crate::sutra_library::models::*;
use anyhow::Result;
use regex::Regex;
use sha2::Digest;

/// Rust 代码解析器
///
/// 使用正则表达式（未来可替换为 tree-sitter）解析代码结构。
pub struct RustDomainParser;

impl RustDomainParser {
    pub fn new() -> Self {
        Self
    }

    /// 提取 Rust 项（函数、结构体、trait、impl、mod、enum）
    fn extract_items(&self, code: &str) -> Vec<RustItem> {
        let mut items = Vec::new();

        // 模块声明
        let re = Regex::new(r"(?m)^\s*(?:pub\s+)?mod\s+(\w+)\s*(\{|;)").unwrap();
        for cap in re.captures_iter(code) {
            items.push(RustItem {
                name: cap[1].to_string(),
                item_type: "mod".to_string(),
                content: cap[0].to_string(),
                line: code[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 结构体
        let re =
            Regex::new(r"(?ms)^\s*(?:pub\s+)?struct\s+(\w+)(?:<[^>]*>)?\s*\{([^}]*)\}").unwrap();
        for cap in re.captures_iter(code) {
            items.push(RustItem {
                name: cap[1].to_string(),
                item_type: "struct".to_string(),
                content: cap[0].to_string(),
                line: code[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 枚举
        let re = Regex::new(r"(?ms)^\s*(?:pub\s+)?enum\s+(\w+)(?:<[^>]*>)?\s*\{([^}]*)\}").unwrap();
        for cap in re.captures_iter(code) {
            items.push(RustItem {
                name: cap[1].to_string(),
                item_type: "enum".to_string(),
                content: cap[0].to_string(),
                line: code[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 函数
        let re = Regex::new(r"(?ms)^\s*(?:pub\s+(?:unsafe\s+)?)?(?:async\s+)?fn\s+(\w+)(?:<[^>]*>)?\s*\(([^)]*)\)\s*(?:->\s*[^{]+)?\s*\{").unwrap();
        for cap in re.captures_iter(code) {
            items.push(RustItem {
                name: cap[1].to_string(),
                item_type: "fn".to_string(),
                content: cap[0].to_string(),
                line: code[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // Trait
        let re = Regex::new(
            r"(?ms)^\s*(?:pub\s+)?(?:unsafe\s+)?trait\s+(\w+)(?:<[^>]*>)?\s*\{([^}]*)\}",
        )
        .unwrap();
        for cap in re.captures_iter(code) {
            items.push(RustItem {
                name: cap[1].to_string(),
                item_type: "trait".to_string(),
                content: cap[0].to_string(),
                line: code[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // impl
        let re = Regex::new(r"(?ms)^\s*(?:pub\s+)?(?:unsafe\s+)?impl(?:<[^>]*>)?\s+(\w+(?:<[^>]*>)?)\s*(?:for\s+(\w+))?\s*\{([^}]*)\}").unwrap();
        for cap in re.captures_iter(code) {
            let target = if let Some(t) = cap.get(2) {
                format!("{} for {}", cap[1].to_string(), t.as_str())
            } else {
                cap[1].to_string()
            };
            items.push(RustItem {
                name: target,
                item_type: "impl".to_string(),
                content: cap[0].to_string(),
                line: code[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        items
    }
}

struct RustItem {
    name: String,
    item_type: String,
    content: String,
    line: usize,
}

impl DomainParser for RustDomainParser {
    fn domain_name(&self) -> &str {
        "rust"
    }

    fn split_semantic_chunks(&self, raw: &str, _ctx: &ParseContext) -> Vec<SemanticChunk> {
        let items = self.extract_items(raw);
        if items.is_empty() {
            // 无结构，整体作为文件节点
            return vec![SemanticChunk {
                title: "file".to_string(),
                content: raw.to_string(),
                node_type: "rust_file".to_string(),
                parent_path: None,
                sort_order: 0,
                metadata: serde_json::json!({"line_count": raw.lines().count()}),
            }];
        }

        items
            .into_iter()
            .enumerate()
            .map(|(i, item)| SemanticChunk {
                title: item.name,
                content: item.content,
                node_type: format!("rust_{}", item.item_type),
                parent_path: None,
                sort_order: i as u32,
                metadata: serde_json::json!({"line": item.line}),
            })
            .collect()
    }

    fn enrich_chunk(&self, chunk: &SemanticChunk) -> (String, serde_json::Value) {
        let summary = chunk.content.chars().take(100).collect::<String>();
        let metadata = serde_json::json!({
            "char_count": chunk.content.len(),
            "line_count": chunk.content.lines().count(),
        });
        (summary, metadata)
    }

    fn extract_edges(&self, nodes: &[MemoryNode]) -> Vec<RefEdge> {
        let mut edges = Vec::new();
        // 检测函数调用关系
        for node in nodes {
            if node.node_type == "rust_fn" {
                for other in nodes {
                    if other.node_type == "rust_fn" && other.node_id != node.node_id {
                        // 检查是否调用了其他函数
                        if node.content.contains(&other.title) {
                            edges.push(RefEdge {
                                target_node_id: other.node_id.clone(),
                                target_collection_id: other.collection_id.clone(),
                                edge_type: RefType::Calls,
                                weight: 0.8,
                            });
                        }
                    }
                }
            }
        }
        edges
    }

    fn incremental_parse(&self, old: &MemoryNode, diff: &str) -> Result<MemoryNode> {
        let mut updated = old.clone();
        updated.content = format!("{}\n{}", old.content, diff);
        updated.updated_at = chrono::Utc::now().timestamp();
        updated.content_hash = format!("{:x}", sha2::Sha256::digest(updated.content.as_bytes()));
        Ok(updated)
    }

    fn format_context(&self, node: &MemoryNode, _mode: ContextMode) -> String {
        format!(
            "```rust\n// {} ({})\n{}\n```",
            node.title, node.node_type, node.content
        )
    }

    fn compress_nodes(&self, nodes: &[MemoryNode]) -> MemoryNode {
        let title = nodes.first().map(|n| n.title.clone()).unwrap_or_default();
        let content = nodes
            .iter()
            .map(|n| format!("// {}: {}\n{}", n.title, n.node_type, n.summary))
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

    fn rerank_nodes(&self, query: &str, candidates: &mut Vec<ScoredNode>) {
        let q = query.to_lowercase();
        for scored in candidates.iter_mut() {
            let node = &scored.node;
            // 符号名精确匹配加分
            if node.title.to_lowercase() == q {
                scored.score += 0.5;
            }
            // 函数/结构体定义优先
            if node.node_type == "rust_fn" || node.node_type == "rust_struct" {
                scored.score += 0.2;
            }
        }
    }
}

pub struct RustDomainTokenizer;

impl RustDomainTokenizer {
    pub fn new() -> Self {
        Self
    }
}

impl DomainTokenizer for RustDomainTokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> {
        // Rust 标识符风格分词
        let mut tokens = Vec::new();
        // 蛇形命名拆分
        let re = Regex::new(r"[a-z0-9]+(?:_[a-z0-9]+)*|[A-Z][a-z0-9]*(?:[A-Z][a-z0-9]*)*").unwrap();
        for m in re.find_iter(text) {
            let token = m.as_str().to_lowercase();
            // 蛇形再拆分
            for part in token.split('_') {
                if !part.is_empty() {
                    tokens.push(part.to_string());
                }
            }
        }
        tokens
    }

    fn stop_words(&self) -> &[&str] {
        &[
            "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait", "self",
            "super", "crate", "as", "where", "return", "if", "else", "for", "while", "loop",
            "match", "unsafe", "async", "await", "move", "ref", "static", "const", "type", "dyn",
            "in", "and", "or", "not",
        ]
    }

    fn expand_query(&self, query: &str) -> Vec<String> {
        let mut expansions = vec![query.to_string()];
        // 蛇形和驼峰互转
        let snake = query.to_lowercase().replace(' ', "_");
        if snake != query {
            expansions.push(snake);
        }
        // 去掉下划线
        let no_underscore = query.replace('_', "");
        if no_underscore != query {
            expansions.push(no_underscore);
        }
        expansions
    }
}
