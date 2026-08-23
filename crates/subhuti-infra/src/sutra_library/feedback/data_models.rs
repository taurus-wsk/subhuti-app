//! # 反馈闭环数据模型
//!
//! 执行日志、召回切片信息、反馈指标、分析结果等。

use crate::sutra_library::recall::RetrieveSource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── 执行日志 ───────────────────────────────────────────────

/// 执行日志（用户Query → 藏经阁召回 → Agent执行 → 反馈）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLog {
    /// Query 的 Sha256 哈希（用于去重/分组）
    pub query_hash: String,
    /// 原始 Query 文本
    pub query: String,
    /// 召回的全部切片信息
    pub recalled_chunks: Vec<RecallChunkInfo>,
    /// Agent 实际使用的切片 ID 列表
    pub used_chunk_ids: Vec<String>,
    /// 任务执行是否成功
    pub task_success: bool,
    /// 时间戳（Unix 毫秒）
    pub timestamp: i64,
    /// 对话图谱 ID
    pub graph: String,
    /// 领域
    pub domain: String,
    /// 会话 ID（可选，用于关联会话记忆）
    pub session_id: Option<String>,
}

/// 单条召回切片的信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallChunkInfo {
    /// 切片 ID
    pub chunk_id: String,
    /// 来源通路
    pub source: RetrieveSource,
    /// 召回得分
    pub score: f32,
    /// LLM 自省相关度（可选，抽样评估）
    pub llm_relevance: Option<f32>,
    /// LLM 自省理由（可选）
    pub llm_reason: Option<String>,
}

// ─── 反馈指标 ───────────────────────────────────────────────

/// 反馈指标快照（实时统计）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackMetrics {
    /// 总查询次数
    pub total_queries: usize,
    /// 命中率（used_chunks / recalled_chunks 的比例）
    pub hit_rate: f64,
    /// 各通路贡献占比 {source_name: ratio}
    pub source_contribution: HashMap<String, f64>,
    /// 平均实际使用切片数
    pub avg_used_chunks: f64,
    /// 窗口大小
    pub window_size: usize,
    /// 成功任务占比
    pub success_rate: f64,
    /// 时间戳
    pub timestamp: i64,
}

/// 分析结果（批量分析后输出）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackResult {
    /// 指标
    pub metrics: FeedbackMetrics,
    /// 需要更新的 Learned 边 { (from, to): 新权重 }
    pub learned_edge_updates: Vec<(String, String, f32)>,
    /// 分析窗口内的日志条数
    pub analyzed_logs: usize,
    /// 分析时间戳
    pub timestamp: i64,
}

// ─── 配置 ─────────────────────────────────────────────────────

/// 反馈分析器配置
#[derive(Debug, Clone)]
pub struct FeedbackConfig {
    /// 滑动窗口大小（最多保留多少条执行日志）
    pub window_size: usize,
    /// 实体共现挖掘：正任务增量
    pub cooccurrence_positive_delta: f32,
    /// 实体共现挖掘：失败任务增量（减半）
    pub cooccurrence_failure_delta: f32,
    /// 后台分析间隔（秒）
    pub background_interval_secs: u64,
    /// LLM 自省采样率（0.0 ~ 1.0，0 表示关闭）
    pub llm_sample_rate: f64,
    /// 是否启用 PG 持久化执行日志
    pub enable_pg_persistence: bool,
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            window_size: 1000,
            cooccurrence_positive_delta: 0.15,
            cooccurrence_failure_delta: 0.075,
            background_interval_secs: 300, // 5 分钟
            llm_sample_rate: 0.0,          // 默认关闭 LLM 自省
            enable_pg_persistence: true,
        }
    }
}

// ─── PG 表结构 ───────────────────────────────────────────────

/// 执行日志 PG 表名
pub const EXECUTION_LOGS_TABLE: &str = "sutra_execution_logs";

/// 反馈指标 PG 表名
pub const FEEDBACK_METRICS_TABLE: &str = "sutra_feedback_metrics";

/// 创建执行日志表的 SQL（仅建表，不含索引）
pub const CREATE_EXECUTION_LOGS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS sutra_execution_logs (
    id SERIAL PRIMARY KEY,
    query_hash VARCHAR(64) NOT NULL,
    query TEXT NOT NULL,
    recalled_chunks JSONB NOT NULL DEFAULT '[]',
    used_chunk_ids TEXT[] NOT NULL DEFAULT '{}',
    task_success BOOLEAN NOT NULL DEFAULT true,
    timestamp BIGINT NOT NULL,
    graph VARCHAR(64) NOT NULL DEFAULT 'default',
    domain VARCHAR(64) NOT NULL DEFAULT 'general',
    session_id VARCHAR(64) DEFAULT NULL
)"#;

/// 创建执行日志表时间戳索引的 SQL
pub const CREATE_EXECUTION_LOGS_INDEX_TS: &str =
    "CREATE INDEX IF NOT EXISTS idx_sutra_exec_logs_ts ON sutra_execution_logs(timestamp)";

/// 创建执行日志表领域索引的 SQL
pub const CREATE_EXECUTION_LOGS_INDEX_DOMAIN: &str =
    "CREATE INDEX IF NOT EXISTS idx_sutra_exec_logs_domain ON sutra_execution_logs(domain)";

/// 创建反馈指标表的 SQL（仅建表，不含索引）
pub const CREATE_FEEDBACK_METRICS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS sutra_feedback_metrics (
    id SERIAL PRIMARY KEY,
    graph VARCHAR(64) NOT NULL,
    domain VARCHAR(64) NOT NULL,
    total_queries BIGINT NOT NULL DEFAULT 0,
    hit_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    source_contribution JSONB NOT NULL DEFAULT '{}',
    avg_used_chunks DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    success_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    analyzed_at BIGINT NOT NULL
)"#;

/// 创建反馈指标表图谱索引的 SQL
pub const CREATE_FEEDBACK_METRICS_INDEX_GRAPH: &str =
    "CREATE INDEX IF NOT EXISTS idx_sutra_fb_metrics_graph ON sutra_feedback_metrics(graph, domain)";
