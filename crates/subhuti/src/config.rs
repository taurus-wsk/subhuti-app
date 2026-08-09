use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LLMConfig {
    pub model: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub temperature: f32,
    pub max_tokens: usize,
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4".to_string(),
            api_url: "https://api.openai.com/v1".to_string(),
            api_key: None,
            temperature: 0.7,
            max_tokens: 2048,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LLMProvider {
    OpenAI,
    Ollama,
    Doubao,
    /// 智谱 AI (GLM-4 / GLM-4.7-Flash 系列)，OpenAI 兼容协议
    Zhipu,
    Custom,
}

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubhutiConfig {
    pub llm: LLMConfig,
    pub provider: LLMProvider,
    pub runtime: RuntimeConfig,
    pub memory: MemoryConfig,
    pub flow: FlowConfig,
    pub db: Option<DbConfig>,
}

impl Default for SubhutiConfig {
    fn default() -> Self {
        Self {
            llm: LLMConfig::default(),
            provider: LLMProvider::OpenAI,
            runtime: RuntimeConfig::default(),
            memory: MemoryConfig::default(),
            flow: FlowConfig::default(),
            db: None,
        }
    }
}

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
            Self::Calm,
            Self::Witty,
        ]
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackType {
    Like,
    Dislike,
    Neutral,
    Comment,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BigFive {
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InteractionStats {
    pub total_interactions: u32,
    pub skill_usage: std::collections::HashMap<String, u32>,
    pub last_evolve_time: chrono::DateTime<chrono::Utc>,
    pub avg_response_time_ms: u64,
    pub likes: u32,
    pub dislikes: u32,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowTemplate {
    Simple,
    ReAct,
    PlanAct,
    ChainOfThought,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TokenUsage {
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub flow_template: Option<FlowTemplate>,
    pub flow_templates: Vec<FlowTemplate>,
    pub priority: i32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OrchestrationResult {
    pub strategy: String,
    pub expert_chain: Vec<String>,
    pub output: String,
    pub tokens: TokenUsage,
    pub expert_outputs: Vec<String>,
}
