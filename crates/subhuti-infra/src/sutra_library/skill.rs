//! # 记忆技能接口
//!
//! AI 可自主调用的记忆管理接口，通过此接口可以直接创建表、增删改查数据。

use crate::sutra_library::engine::MemoryEnginePort;
use crate::sutra_library::models::*;
use std::sync::Arc;

/// AI 可操作的记忆技能
///
/// 所有方法返回 String 以便 AI 直接理解和呈现。
pub struct MemorySkill {
    engine: Arc<MemoryEnginePort>,
}

impl MemorySkill {
    pub fn new(engine: Arc<MemoryEnginePort>) -> Self {
        Self { engine }
    }

    // ─── 集合管理 ───────────────────────────────────────────────

    /// 创建记忆集合（类似创建表/数据库）
    pub fn create_collection(&self, name: &str, domain: &str, description: &str) -> String {
        let collection = self.engine.create_collection(name, domain, description);
        format!(
            "✅ 已创建集合\n集合ID: {}\n名称: {}\n领域: {}\n描述: {}",
            collection.collection_id, collection.name, collection.domain, collection.description
        )
    }

    /// 列出所有集合
    pub fn list_collections(&self) -> String {
        let collections = self.engine.list_collections();
        if collections.is_empty() {
            return "暂无记忆集合".to_string();
        }
        let mut output = "📚 记忆集合列表\n".to_string();
        output.push_str(&format!("{:<40} {:<20} {:<15}\n", "集合ID", "名称", "领域"));
        output.push_str(&"-".repeat(80));
        output.push('\n');
        for c in &collections {
            output.push_str(&format!(
                "{:<40} {:<20} {:<15}\n",
                c.collection_id, c.name, c.domain
            ));
        }
        output
    }

    // ─── 节点写入 ───────────────────────────────────────────────

    /// 写入记忆（类似 INSERT）
    ///
    /// 自动完成领域语义切片、摘要生成、树结构挂载。
    pub fn write(
        &self,
        collection_id: &str,
        content: &str,
        domain: &str,
        parent_id: Option<&str>,
    ) -> String {
        match self
            .engine
            .write_node(collection_id, content, domain, parent_id)
        {
            Ok(node) => {
                format!(
                    "✅ 已写入记忆\n节点ID: {}\n标题: {}\n路径: {}\n类型: {}\n摘要: {}",
                    node.node_id, node.title, node.path, node.node_type, node.summary
                )
            }
            Err(e) => format!("❌ 写入失败: {}", e),
        }
    }

    /// 批量写入
    pub fn write_batch(
        &self,
        collection_id: &str,
        items: &[(&str, &str, &str)], // (content, domain, parent_id_or_empty)
    ) -> String {
        let mut results = Vec::new();
        for (content, domain, parent) in items {
            let parent = if parent.is_empty() {
                None
            } else {
                Some(*parent)
            };
            match self
                .engine
                .write_node(collection_id, content, domain, parent)
            {
                Ok(node) => results.push(format!("✅ {}: {}", node.title, node.node_id)),
                Err(e) => results.push(format!(
                    "❌ {}: {}",
                    content.chars().take(30).collect::<String>(),
                    e
                )),
            }
        }
        results.join("\n")
    }

    // ─── 节点查询 ───────────────────────────────────────────────

    /// 按 ID 读取记忆（类似 SELECT WHERE id = ?）
    pub fn read(&self, node_id: &str) -> String {
        match self.engine.read_node(node_id) {
            Some(node) => self.format_node(&node),
            None => format!("❌ 未找到节点: {}", node_id),
        }
    }

    /// 按路径读取
    pub fn read_by_path(&self, path: &str) -> String {
        match self.engine.read_by_path(path) {
            Some(node) => self.format_node(&node),
            None => format!("❌ 未找到路径: {}", path),
        }
    }

    /// 获取子节点
    pub fn children(&self, parent_id: &str) -> String {
        let children = self.engine.get_children(parent_id);
        if children.is_empty() {
            return "无子节点".to_string();
        }
        let mut output = format!("📂 子节点列表 (父节点: {})\n", parent_id);
        for child in &children {
            output.push_str(&format!(
                "  ├─ {} ({}) [{}]\n",
                child.title, child.node_type, child.path
            ));
        }
        output
    }

    // ─── 检索搜索 ───────────────────────────────────────────────

    /// 搜索记忆（类似 SELECT ... WHERE content MATCH ?）
    ///
    /// 三级检索：临时记忆 → 热内存 → PG 冷库
    pub async fn search(&self, text: &str, collection_id: Option<&str>, limit: usize) -> String {
        let query = RetrievalQuery {
            text: text.to_string(),
            collection_id: collection_id.map(|s| s.to_string()),
            domain: None,
            node_type: None,
            limit,
            mode: ContextMode::Standard,
        };
        let result = self.engine.search(&query).await;
        if result.nodes.is_empty() {
            return "未找到匹配的记忆".to_string();
        }
        let mut output = format!(
            "🔍 搜索结果 (共 {} 条, 来源: {:?})\n\n",
            result.nodes.len(),
            result.source
        );
        for (i, scored) in result.nodes.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}** (得分: {:.2})\n   路径: {} | 类型: {} | 领域: {}\n   摘要: {}\n\n",
                i + 1,
                scored.node.title,
                scored.score,
                scored.node.path,
                scored.node.node_type,
                scored.node.domain,
                scored.node.summary.chars().take(100).collect::<String>(),
            ));
        }
        output
    }

    // ─── 删除 ───────────────────────────────────────────────────

    /// 删除节点（类似 DELETE，递归删除子树）
    pub fn delete(&self, node_id: &str) -> String {
        self.engine.delete_node(node_id);
        format!("✅ 已删除节点及其子树: {}", node_id)
    }

    // ─── 反馈 ───────────────────────────────────────────────────

    /// 正反馈
    pub fn like(&self, node_id: &str) -> String {
        self.engine.apply_feedback(node_id, true);
        format!("👍 已记录正反馈: {}", node_id)
    }

    /// 负反馈
    pub fn dislike(&self, node_id: &str) -> String {
        self.engine.apply_feedback(node_id, false);
        format!("👎 已记录负反馈: {}", node_id)
    }

    // ─── 会话记忆 ───────────────────────────────────────────────

    /// 添加会话临时记忆
    pub fn add_session(&self, session_id: &str, content: &str, domain: &str) -> String {
        self.engine.add_session_memory(session_id, content, domain);
        format!("✅ 已添加会话临时记忆: {}", session_id)
    }

    /// 沉淀会话到持久记忆
    pub fn precipitate(&self, session_id: &str, collection_id: &str, domain: &str) -> String {
        self.engine
            .precipitate_session(session_id, collection_id, domain);
        format!("✅ 已沉淀会话 {} 到集合 {}", session_id, collection_id)
    }

    // ─── 统计 ───────────────────────────────────────────────────

    /// 统计信息
    pub fn stats(&self) -> String {
        let s = self.engine.stats();
        format!(
            "📊 藏经阁统计\n\
             - 总节点数: {}\n\
             - 热记忆: {}\n\
             - 冷记忆: {}\n\
             - 集合数: {}\n\
             - 关联边: {}\n\
             - 快照数: {}",
            s.total_nodes, s.hot_nodes, s.cold_nodes, s.collections, s.edges, s.snapshots
        )
    }

    // ─── 格式化 ─────────────────────────────────────────────────

    fn format_node(&self, node: &MemoryNode) -> String {
        format!(
            "📄 记忆节点\n\
             - 节点ID: {}\n\
             - 标题: {}\n\
             - 路径: {}\n\
             - 类型: {}\n\
             - 领域: {}\n\
             - 摘要: {}\n\
             - 内容: {}\n\
             - 激活分: {:.2}\n\
             - 重要性: {}\n\
             - 访问次数: {}\n\
             - 反馈分: {:.2}\n\
             - 父节点: {}\n\
             - 版本: {}",
            node.node_id,
            node.title,
            node.path,
            node.node_type,
            node.domain,
            node.summary,
            node.content.chars().take(200).collect::<String>(),
            node.base_activation,
            node.importance,
            node.access_count,
            node.feedback_score,
            node.parent_id.as_deref().unwrap_or("(根节点)"),
            node.version_tag,
        )
    }
}
