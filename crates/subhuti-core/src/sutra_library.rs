//! # 藏经阁引擎端口（SutraLibraryPort）
//!
//! 专家可访问的记忆系统端口，定义在 core 层以避免依赖 infra 具体实现。
//! 具体实现由 infra 层的 `MemoryEnginePort` + `MemorySkill` 提供。

use async_trait::async_trait;

/// 藏经阁记忆引擎端口
///
/// 专家通过此接口访问结构化记忆系统：
/// - 创建/管理集合（类似表/数据库）
/// - 写入/读取记忆节点
/// - 搜索记忆（三级检索：临时→热内存→PG冷库）
/// - 会话记忆沉淀
/// - 反馈训练
#[async_trait]
pub trait SutraLibraryPort: Send + Sync {
    /// 创建记忆集合（类似创建表）
    fn create_collection(&self, name: &str, domain: &str, description: &str) -> String;

    /// 列出所有集合
    fn list_collections(&self) -> String;

    /// 写入记忆节点（自动领域切片、幂等去重、树挂载）
    fn write(
        &self,
        collection_id: &str,
        content: &str,
        domain: &str,
        parent_id: Option<&str>,
    ) -> String;

    /// 按 ID 读取记忆节点
    fn read(&self, node_id: &str) -> String;

    /// 搜索记忆（三级检索）
    async fn search(&self, text: &str, collection_id: Option<&str>, limit: usize) -> String;

    /// 新版召回检索（五阶段流水线：BaseSearch → Space → Graph → 合并 → 排序）
    ///
    /// 使用四大召回数据源（基础检索、空间通路、图谱一阶、图谱二阶）和归一化排序。
    /// 返回格式化的检索结果字符串，与 `search` 的返回格式一致。
    async fn library_retrieve(&self, query: &str, top_k: usize) -> String;

    /// 删除节点（递归删除子树）
    fn delete(&self, node_id: &str) -> String;

    /// 添加会话临时记忆
    fn add_session(&self, session_id: &str, content: &str, domain: &str) -> String;

    /// 沉淀会话到持久记忆
    fn precipitate(&self, session_id: &str, collection_id: &str, domain: &str) -> String;

    /// 一键自动沉淀（P0 主链路）
    ///
    /// 自动建/复用领域集合，把提炼好的事实写入会话记忆并**等待落库完成**。
    /// 返回实际落库的节点数（0 表示无有效事实或不可持久化）。
    ///
    /// 与 `precipitate` 的区别：后者是同步 fire-and-forget，MCP stdio 这类
    /// "请求结束即退出进程"的场景下可能写不完；本方法 await 到底。
    async fn auto_precipitate(&self, session_id: &str, domain: &str, facts: &[String]) -> usize;

    /// 冷启动灌入领域静态知识（P1）
    ///
    /// `entries` 为 (标题, 正文) 列表；已存在内容自动跳过。返回新写入节点数。
    async fn seed_knowledge(&self, domain: &str, entries: &[(String, String)]) -> usize;

    /// 结构化统计（P2 可观测），返回 JSON：
    /// `{total_nodes, hot_nodes, cold_nodes, collections, edges, snapshots, tantivy_docs}`
    fn stats_json(&self) -> serde_json::Value;

    /// 正反馈
    fn like(&self, node_id: &str) -> String;

    /// 负反馈
    fn dislike(&self, node_id: &str) -> String;

    /// 统计信息
    fn stats(&self) -> String;

    /// 记录执行日志到反馈分析器
    ///
    /// 专家在执行完成后调用，用于反馈闭环：
    /// - 命中率统计
    /// - 实体共现挖掘（自动生成 Learned 边）
    ///
    /// # 参数
    /// - `query`: 用户原始 Query
    /// - `task_success`: 任务执行是否成功
    /// - `graph`: 对话图谱 ID
    /// - `domain`: 领域
    /// - `session_id`: 会话 ID（可选）
    /// - `final_answer`: 本轮最终回答。用于判定哪些召回切片**真的被用上了**——
    ///   没有它 `used_chunk_ids` 只能恒为空，命中率永远 0%，反馈信号毫无信息量。
    fn record_execution(
        &self,
        query: &str,
        task_success: bool,
        graph: &str,
        domain: &str,
        session_id: Option<String>,
        final_answer: &str,
    ) -> String;

    // ─── 知识库 CRUD ────────────────────────────────────────────

    /// 获取所有知识库列表
    ///
    /// 返回 JSON 格式的知识库列表，包含 id、name、description、tags、expert_id 等字段
    async fn list_knowledge_bases(&self) -> String;

    /// 获取指定知识库的所有切片内容
    ///
    /// # 参数
    /// - `kb_id`: 知识库 ID
    ///
    /// 返回 JSON 格式的切片列表，包含 id、title、content、metadata 等字段
    async fn list_chunks(&self, kb_id: &str) -> String;

    /// 根据 expert_id 获取关联的知识库
    ///
    /// # 参数
    /// - `expert_id`: 专家 ID（如 "rust-expert"）
    ///
    /// 返回 JSON 格式的知识库列表
    async fn get_knowledge_base_by_expert(&self, expert_id: &str) -> String;
}
