//! # Blender 建模域适配器
//!
//! 解析 Blender Python 脚本 / 场景描述，提取：
//! - 场景、对象、材质、修改器、动画等结构化元素
//! - 资产引用关系（对象→材质、对象→修改器）
//! - 场景层级树

use crate::sutra_library::domain::{DomainParser, DomainTokenizer};
use crate::sutra_library::models::*;
use anyhow::Result;
use regex::Regex;
use sha2::Digest;

/// Blender 场景解析器
pub struct BlenderDomainParser;

impl Default for BlenderDomainParser {
    fn default() -> Self {
        Self::new()
    }
}

impl BlenderDomainParser {
    pub fn new() -> Self {
        Self
    }

    /// 提取 Blender 场景元素
    fn extract_blender_items(&self, text: &str) -> Vec<BlenderItem> {
        let mut items = Vec::new();

        // 场景创建
        let re = Regex::new(
            r#"(?ms)bpy\.context\.scene\s*=\s*(\w+)|bpy\.data\.scenes\.new\(\s*["']([^"']+)["']"#,
        )
        .unwrap();
        for cap in re.captures_iter(text) {
            let name = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|m| m.as_str())
                .unwrap_or("scene");
            items.push(BlenderItem {
                name: name.to_string(),
                item_type: "scene".to_string(),
                content: cap[0].to_string(),
                line: text[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 对象创建（mesh, camera, light, empty, curve, armature 等）
        let re = Regex::new(
            r#"(?ms)bpy\.data\.objects\.new\(\s*["']([^"']+)["']\s*,\s*["']([^"']+)["']"#,
        )
        .unwrap();
        for cap in re.captures_iter(text) {
            items.push(BlenderItem {
                name: cap[1].to_string(),
                item_type: format!("object_{}", cap[2].to_string().to_lowercase()),
                content: cap[0].to_string(),
                line: text[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 材质创建
        let re = Regex::new(r#"(?ms)bpy\.data\.materials\.new\(\s*["']([^"']+)["']"#).unwrap();
        for cap in re.captures_iter(text) {
            items.push(BlenderItem {
                name: cap[1].to_string(),
                item_type: "material".to_string(),
                content: cap[0].to_string(),
                line: text[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 修改器添加
        let re = Regex::new(
            r#"(?ms)(\w+)\.modifiers\.new\(\s*["']([^"']+)["']\s*,\s*["']([^"']+)["']\s*\)"#,
        )
        .unwrap();
        for cap in re.captures_iter(text) {
            items.push(BlenderItem {
                name: cap[2].to_string(),
                item_type: format!("modifier_{}", cap[3].to_string().to_lowercase()),
                content: cap[0].to_string(),
                line: text[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 关键帧动画
        let re = Regex::new(r#"(?ms)(\w+)\.keyframe_insert\(\s*["']([^"']+)["']"#).unwrap();
        for cap in re.captures_iter(text) {
            items.push(BlenderItem {
                name: format!("keyframe_{}", &cap[2]),
                item_type: "animation".to_string(),
                content: cap[0].to_string(),
                line: text[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 节点组（Shader Nodes / Geometry Nodes）
        let re = Regex::new(r#"(?ms)bpy\.data\.node_groups\.new\(\s*["']([^"']+)["']"#).unwrap();
        for cap in re.captures_iter(text) {
            items.push(BlenderItem {
                name: cap[1].to_string(),
                item_type: "node_group".to_string(),
                content: cap[0].to_string(),
                line: text[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        // 集合（Collection）
        let re = Regex::new(r#"(?ms)bpy\.data\.collections\.new\(\s*["']([^"']+)["']"#).unwrap();
        for cap in re.captures_iter(text) {
            items.push(BlenderItem {
                name: cap[1].to_string(),
                item_type: "collection".to_string(),
                content: cap[0].to_string(),
                line: text[..cap.get(0).unwrap().start()].lines().count() + 1,
            });
        }

        items
    }

    /// 内置 Blender 实体关系知识库：概念A → 相关概念B
    fn blender_entity_relations() -> Vec<(&'static str, &'static str, f32)> {
        vec![
            ("阵列修改器", "循环建模", 0.8),
            ("阵列修改器", "Array Modifier", 0.9),
            ("阵列修改器", "重复排列", 0.7),
            ("布尔修改器", "差集", 0.7),
            ("布尔修改器", "交集", 0.7),
            ("布尔修改器", "并集", 0.7),
            ("曲面细分修改器", "细分", 0.8),
            ("实体化修改器", "厚度", 0.7),
            ("几何节点", "程序化建模", 0.8),
            ("几何节点", "节点图", 0.8),
            ("几何节点", "参数化模型", 0.7),
            ("材质节点", "PBR 材质", 0.8),
            ("材质节点", "金属度", 0.7),
            ("材质节点", "粗糙度", 0.7),
            ("材质节点", "法线贴图", 0.7),
            ("PBR 材质", "金属度", 0.8),
            ("PBR 材质", "粗糙度", 0.8),
            ("PBR 材质", "法线贴图", 0.7),
            ("Cycles 渲染", "光线追踪", 0.8),
            ("Cycles 渲染", "GPU 加速", 0.7),
            ("Cycles 渲染", "采样策略", 0.6),
            ("骨骼绑定", "角色动画", 0.8),
            ("骨骼绑定", "IK/FK", 0.8),
            ("骨骼绑定", "权重绘制", 0.7),
            ("雕刻模式", "动态拓扑", 0.8),
            ("雕刻模式", "数字雕刻", 0.7),
            ("UV 展开", "纹理映射", 0.8),
            ("UV 展开", "智能投影", 0.7),
            ("UV 展开", "缝合边", 0.7),
            ("粒子系统", "物理模拟", 0.7),
            ("粒子系统", "碰撞检测", 0.6),
            ("粒子系统", "头发", 0.6),
            ("粒子系统", "火焰", 0.6),
        ]
    }

    /// 从自然语言内容中提取 Blender 实体关系对
    fn extract_blender_entity_pairs(&self, content: &str) -> Vec<(String, String, f32)> {
        let relations = Self::blender_entity_relations();
        let mut pairs = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (a, b, weight) in &relations {
            let has_a = content.contains(*a);
            let has_b = content.contains(*b);
            if has_a && has_b {
                let key = if a < b {
                    format!("{}|{}", a, b)
                } else {
                    format!("{}|{}", b, a)
                };
                if seen.insert(key) {
                    pairs.push((a.to_string(), b.to_string(), *weight));
                }
            }
        }
        pairs
    }
}

struct BlenderItem {
    name: String,
    item_type: String,
    content: String,
    line: usize,
}

impl DomainParser for BlenderDomainParser {
    fn domain_name(&self) -> &str {
        "blender"
    }

    fn split_semantic_chunks(&self, raw: &str, _ctx: &ParseContext) -> Vec<SemanticChunk> {
        let items = self.extract_blender_items(raw);
        if items.is_empty() {
            // 无结构元素，整体作为脚本节点
            return vec![SemanticChunk {
                title: "blender_script".to_string(),
                content: raw.to_string(),
                node_type: "blender_script".to_string(),
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
                node_type: item.item_type,
                parent_path: None,
                sort_order: i as u32,
                metadata: serde_json::json!({"line": item.line}),
            })
            .collect()
    }

    fn enrich_chunk(&self, chunk: &SemanticChunk) -> (String, serde_json::Value) {
        let summary = format!(
            "[Blender {}] {}",
            chunk
                .node_type
                .replace("object_", "对象:")
                .replace("modifier_", "修改器:"),
            chunk.title
        );
        let metadata = serde_json::json!({
            "char_count": chunk.content.len(),
            "blender_type": chunk.node_type,
        });
        (summary, metadata)
    }

    fn extract_edges(&self, nodes: &[MemoryNode]) -> Vec<RefEdge> {
        let mut edges = Vec::new();

        // ─── 已存在的 Python 脚本解析逻辑 ─────────────────────
        for node in nodes {
            // 材质 → 对象关联
            if node.node_type == "material" {
                for other in nodes {
                    if other.node_type.starts_with("object_")
                        && other.node_id != node.node_id
                        && (node.content.contains(&other.title)
                            || other.content.contains(&node.title))
                    {
                        edges.push(RefEdge {
                            target_node_id: other.node_id.clone(),
                            target_collection_id: other.collection_id.clone(),
                            edge_type: RefType::Uses,
                            weight: 0.7,
                        });
                    }
                }
            }
            // 修改器 → 对象关联
            if node.node_type.starts_with("modifier_") {
                for other in nodes {
                    if other.node_type.starts_with("object_")
                        && other.node_id != node.node_id
                        && node.content.contains(&other.title)
                    {
                        edges.push(RefEdge {
                            target_node_id: other.node_id.clone(),
                            target_collection_id: other.collection_id.clone(),
                            edge_type: RefType::DependsOn,
                            weight: 0.6,
                        });
                    }
                }
            }
        }

        // ─── 自然语言文档的实体关系提取 ─────────────────────
        // 当节点是 document_section 类型（自然语言文档），
        // 提取内置关系知识库中的实体对，建立 RefEdge 关联
        let doc_nodes: Vec<&MemoryNode> = nodes
            .iter()
            .filter(|n| n.node_type == "document_section" || n.node_type == "document")
            .collect();

        for node in &doc_nodes {
            let pairs = self.extract_blender_entity_pairs(&node.content);
            // 在同一文档中，相关实体间建立边
            // 在同一集合的其他文档中，如果共享实体，也建立边
            for other in &doc_nodes {
                if node.node_id == other.node_id {
                    continue;
                }
                let other_pairs = self.extract_blender_entity_pairs(&other.content);
                // 检查是否有共享实体
                for (a, b, _) in &pairs {
                    if other_pairs
                        .iter()
                        .any(|(oa, ob, _)| oa == a || ob == a || oa == b || ob == b)
                    {
                        edges.push(RefEdge {
                            target_node_id: other.node_id.clone(),
                            target_collection_id: other.collection_id.clone(),
                            edge_type: RefType::Related,
                            weight: 0.5,
                        });
                        break;
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
            "```python\n# Blender {}: {}\n{}\n```",
            node.node_type, node.title, node.content
        )
    }

    fn compress_nodes(&self, nodes: &[MemoryNode]) -> MemoryNode {
        let title = nodes.first().map(|n| n.title.clone()).unwrap_or_default();
        let content = nodes
            .iter()
            .map(|n| format!("# {}: {}\n{}", n.title, n.node_type, n.summary))
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
            // 对象名精确匹配加分
            if node.title.to_lowercase() == q {
                scored.score += 0.5;
            }
            // 场景和对象优先
            if node.node_type == "scene" || node.node_type.starts_with("object_") {
                scored.score += 0.2;
            }
            // 材质和动画次优先
            if node.node_type == "material" || node.node_type == "animation" {
                scored.score += 0.1;
            }
        }
    }

    /// 提取实体关系对，用于 EntityGraph Manual 边
    ///
    /// 从内容中查找内置知识库的实体关系对，返回 (实体A, 实体B, 权重) 三元组。
    /// 引擎在 write_node 时会自动将这些关系对注册到 EntityGraph 的 adjacency 中。
    fn extract_entity_relations(&self, content: &str) -> Vec<(String, String, f32)> {
        self.extract_blender_entity_pairs(content)
    }
}

/// Blender 领域分词器
pub struct BlenderDomainTokenizer;

impl Default for BlenderDomainTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl BlenderDomainTokenizer {
    pub fn new() -> Self {
        Self
    }
}

impl DomainTokenizer for BlenderDomainTokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> {
        let mut tokens: Vec<String> = text
            .split_whitespace()
            .map(|s| {
                s.trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();

        // 下划线拆分
        let mut expanded = Vec::new();
        for token in &tokens {
            for part in token.split('_') {
                if !part.is_empty() {
                    expanded.push(part.to_lowercase());
                }
            }
        }
        tokens.extend(expanded);
        tokens
    }

    fn stop_words(&self) -> &[&str] {
        &[
            "bpy",
            "context",
            "data",
            "scene",
            "object",
            "mesh",
            "new",
            "create",
            "set",
            "get",
            "import",
            "export",
            "select",
            "active",
            "view",
            "layer",
            "collection",
            "name",
            "type",
            "location",
            "rotation",
            "scale",
            "dimensions",
            "matrix",
            "world",
            "local",
            "filepath",
        ]
    }

    fn expand_query(&self, query: &str) -> Vec<String> {
        let mut expansions = vec![query.to_string()];
        // 蛇形和空格互转
        if query.contains('_') {
            expansions.push(query.replace('_', " "));
        }
        if query.contains(' ') {
            expansions.push(query.replace(' ', "_"));
        }
        // 常见 Blender 术语映射
        let blender_terms = [
            ("mesh", "object"),
            ("material", "shader"),
            ("modifier", "effect"),
            ("armature", "rig"),
            ("keyframe", "animation"),
        ];
        for (from, to) in &blender_terms {
            if query.to_lowercase() == *from {
                expansions.push(to.to_string());
            } else if query.to_lowercase() == *to {
                expansions.push(from.to_string());
            }
        }
        expansions
    }
}
