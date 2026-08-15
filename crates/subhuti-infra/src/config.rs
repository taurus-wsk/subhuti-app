//! # 应用配置类型
//!
//! 基础层配置结构体，供应用层加载和解析 Subhuti.toml 使用。

use serde::{Deserialize, Serialize};

/// 大五人格特质
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BigFive {
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

/// 交互统计
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InteractionStats {
    pub total_interactions: u32,
    pub skill_usage: std::collections::HashMap<String, u32>,
    pub last_evolve_time: chrono::DateTime<chrono::Utc>,
    pub avg_response_time_ms: u64,
    pub likes: u32,
    pub dislikes: u32,
}

/// 语气风格
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToneStyle {
    #[default]
    Friendly,
    Formal,
    Casual,
    Enthusiastic,
    Professional,
    Calm,
    Witty,
}

impl ToneStyle {
    pub fn all_styles() -> [Self; 6] {
        [
            Self::Friendly,
            Self::Formal,
            Self::Casual,
            Self::Enthusiastic,
            Self::Professional,
            Self::Calm,
        ]
    }
}

/// 情感倾向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EmotionalTendency {
    Optimistic,
    #[default]
    Neutral,
    Cautious,
    Humorous,
    Professional,
}

/// 反馈类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackType {
    Like,
    Dislike,
    Neutral,
    Comment,
}

/// 灵魂配置
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SoulProfile {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub tone: ToneStyle,
    pub emotional_tendency: EmotionalTendency,
    pub traits: Vec<String>,
    pub big_five: BigFive,
    pub skill_proficiency: std::collections::HashMap<String, f32>,
    pub expertise_areas: std::collections::HashMap<String, f32>,
    pub skill_affinity: std::collections::HashMap<String, f32>,
    pub interaction_stats: InteractionStats,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// 流程模板
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowTemplate {
    Simple,
    ReAct,
    PlanAct,
    ChainOfThought,
}

/// token 用量
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TokenUsage {
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// 运行时配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeConfig {
    pub max_concurrent_requests: usize,
    pub timeout_ms: u64,
    pub retry_count: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 10,
            timeout_ms: 30000,
            retry_count: 3,
        }
    }
}

/// 流程配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlowConfig {
    pub max_iterations: usize,
    pub auto_retry: bool,
    pub max_retries: usize,
    pub convergence_threshold: f32,
    pub enable_reflection: bool,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            auto_retry: true,
            max_retries: 3,
            convergence_threshold: 0.9,
            enable_reflection: true,
        }
    }
}

/// 记忆配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryConfig {
    pub short_term_max_items: usize,
    pub long_term_enabled: bool,
    pub knowledge_enabled: bool,
    pub embedding_enabled: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            short_term_max_items: 100,
            long_term_enabled: true,
            knowledge_enabled: true,
            embedding_enabled: false,
        }
    }
}

/// 数据库配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: u32,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "subhuti".to_string(),
            username: "postgres".to_string(),
            password: "password".to_string(),
            max_connections: 10,
        }
    }
}

/// Subhuti 框架配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubhutiConfig {
    pub llm: subhuti_core::runtime::LLMConfig,
    pub provider: subhuti_core::runtime::LLMProvider,
    pub runtime: RuntimeConfig,
    pub memory: MemoryConfig,
    pub flow: FlowConfig,
    pub db: Option<DbConfig>,
}

impl Default for SubhutiConfig {
    fn default() -> Self {
        Self {
            llm: subhuti_core::runtime::LLMConfig::default(),
            provider: subhuti_core::runtime::LLMProvider::OpenAI,
            runtime: RuntimeConfig::default(),
            memory: MemoryConfig::default(),
            flow: FlowConfig::default(),
            db: None,
        }
    }
}
