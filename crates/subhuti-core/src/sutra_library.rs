//! # 藏经阁引擎端口（SutraLibraryPort）
//!
//! 专家可访问的记忆系统端口，定义在 core 层以避免依赖 infra 具体实现。
//! 具体实现由 infra 层的 `MemoryEnginePort` + `MemorySkill` 提供。

use async_trait::async_trait;
use std::sync::Arc;

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
    fn record_execution(
        &self,
        query: &str,
        task_success: bool,
        graph: &str,
        domain: &str,
        session_id: Option<String>,
    ) -> String;
}

/// 空实现（未配置藏经阁时使用）
pub struct EmptySutraLibrary;

impl EmptySutraLibrary {
    pub fn new() -> Self {
        Self
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

impl Default for EmptySutraLibrary {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SutraLibraryPort for EmptySutraLibrary {
    fn create_collection(&self, _name: &str, _domain: &str, _description: &str) -> String {
        "⚠️ 藏经阁引擎未配置，无法创建集合".to_string()
    }

    fn list_collections(&self) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn write(
        &self,
        _collection_id: &str,
        _content: &str,
        _domain: &str,
        _parent_id: Option<&str>,
    ) -> String {
        "⚠️ 藏经阁引擎未配置，无法写入记忆".to_string()
    }

    fn read(&self, _node_id: &str) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    async fn search(&self, _text: &str, _collection_id: Option<&str>, _limit: usize) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    async fn library_retrieve(&self, _query: &str, _top_k: usize) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn delete(&self, _node_id: &str) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn add_session(&self, _session_id: &str, _content: &str, _domain: &str) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn precipitate(&self, _session_id: &str, _collection_id: &str, _domain: &str) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn like(&self, _node_id: &str) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn dislike(&self, _node_id: &str) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn stats(&self) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }

    fn record_execution(
        &self,
        _query: &str,
        _task_success: bool,
        _graph: &str,
        _domain: &str,
        _session_id: Option<String>,
    ) -> String {
        "⚠️ 藏经阁引擎未配置".to_string()
    }
}
