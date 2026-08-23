//! # 反馈闭环（Feedback Loop）
//!
//! 记忆命中率反馈 + 自动生成记忆关联的完整闭环。
//!
//! ## 架构
//!
//! ```text
//! 用户Query → 藏经阁召回一批记忆切片 → LLM/Agent执行任务
//!     ↓
//! 【反馈采集层】两条反馈来源
//!     ① LLM自省打分：召回出来的切片有没有真正用上
//!     ② 执行日志埋点：实际哪些记忆切片被Agent引用、输出
//!     ↓
//! 【数据分析模块】统计命中率、共现频率、候选切片共现
//!     ↓
//! 自动生成/更新图谱 Learned动态边（实体‑实体熟练度）
//!     ↓
//! 持久化回PG，同步刷新petgraph内存图谱
//! ```
//!
//! ## 核心组件
//!
//! - `FeedbackAnalyzer`: 反馈分析器，滑动窗口 + 共现挖掘 + 命中率统计
//! - `LlmScorer`: LLM 自省打分器，抽样评估切片相关度
//! - `FeedbackConfig`: 配置（窗口大小、共现增量、采样率等）
//!
//! ## 不阻塞主链路
//!
//! 主召回路径：同步、低延迟；反馈数据分析：后台 tokio 任务、异步执行。

pub mod analyzer;
pub mod data_models;
pub mod llm_scorer;

pub use analyzer::FeedbackAnalyzer;
pub use data_models::{
    ExecutionLog, FeedbackConfig, FeedbackMetrics, FeedbackResult, RecallChunkInfo,
};
pub use llm_scorer::{LlmRelevanceScore, LlmScorer, LlmScoringRequest, ScoringChunkInfo};
