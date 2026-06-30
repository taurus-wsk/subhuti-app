//! # 心灵层 (Soul Layer)
//!
//! 记忆与心灵的统一体：心灵宫殿。
//!
//! ## 设计理念
//!
//! 记忆不是冰冷的数据存储，而是心灵宫殿的"房间"。
//! 每一段记忆都影响着人格的塑造，而人格又反过来筛选和解释记忆。
//!
//! ## 心灵宫殿结构
//!
//! - **记忆宫殿 (MemoryPalace)**：所有记忆的容器与检索
//!   - 短期记忆 / 长期记忆 / 知识库
//!   - 记忆分区（6 个主题房间）
//!   - 记忆重要性等级与遗忘机制
//!   - 联想网络（记忆之间的关联）
//!
//! - **人格系统 (Persona)**：动态养成的性格
//!   - 大五人格模型
//!   - 语气风格与情感倾向
//!   - 技能熟练度与擅长领域
//!
//! - **演化引擎 (EvolutionEngine)**：基于记忆的人格演化
//!   - 统计分析轨道（实时、轻量）
//!   - LLM 自反思轨道（周期性、深度）
//!
//! ## 双轨驱动架构
//!
//! - **统计分析轨道**：轻量、实时、可解释，每次互动都更新
//! - **LLM 自反思轨道**：深度、周期性，每 N 次互动触发一次

pub mod palace;

pub use palace::{
    MemoryImportance, MemoryPalace, MemoryZone, PalaceConfig, PalaceMemory, PalaceSearchResult,
    PalaceStats,
};

use crate::{memory::storage::Database, memory::Memory, runtime::Runtime, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

// ─── 性格五维模型 ──────────────────────────────────────────────

/// 性格五维分数
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BigFive {
    /// 开放性 (Openness): 愿意尝试新事物、创造力
    pub openness: f32,
    /// 尽责性 (Conscientiousness): 严谨、精确、结构化
    pub conscientiousness: f32,
    /// 外向性 (Extraversion): 活泼、话多、热情
    pub extraversion: f32,
    /// 宜人性 (Agreeableness): 友善、共情、乐于助人
    pub agreeableness: f32,
    /// 情绪稳定性 (Neuroticism): 谨慎、保守、防御性（注意：分数越高越谨慎/保守）
    pub neuroticism: f32,
}

impl Default for BigFive {
    fn default() -> Self {
        Self {
            openness: 0.6,
            conscientiousness: 0.5,
            extraversion: 0.5,
            agreeableness: 0.7,
            neuroticism: 0.4,
        }
    }
}

impl BigFive {
    /// 转为向量（用于余弦相似度计算）
    fn to_vec(&self) -> [f32; 5] {
        [
            self.openness,
            self.conscientiousness,
            self.extraversion,
            self.agreeableness,
            self.neuroticism,
        ]
    }

    /// 夹紧到 [0, 1]
    fn clamp(&mut self) {
        self.openness = self.openness.clamp(0.0, 1.0);
        self.conscientiousness = self.conscientiousness.clamp(0.0, 1.0);
        self.extraversion = self.extraversion.clamp(0.0, 1.0);
        self.agreeableness = self.agreeableness.clamp(0.0, 1.0);
        self.neuroticism = self.neuroticism.clamp(0.0, 1.0);
    }
}

// ─── 语气风格 ──────────────────────────────────────────────────

/// 语气风格
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub enum ToneStyle {
    #[default]
    Friendly, // 友好
    Formal,       // 正式
    Casual,       // 随意
    Enthusiastic, // 热情
    Calm,         // 冷静
    Witty,        // 机智
}

impl ToneStyle {
    /// 每种语气的特征向量（用于余弦相似度匹配）
    fn feature_vector(&self) -> [f32; 5] {
        match self {
            // [开放性, 尽责性, 外向性, 宜人性, 情绪稳定性]
            ToneStyle::Friendly => [0.5, 0.5, 0.6, 0.9, 0.3],
            ToneStyle::Formal => [0.3, 0.9, 0.1, 0.5, 0.7],
            ToneStyle::Casual => [0.7, 0.3, 0.8, 0.7, 0.2],
            ToneStyle::Enthusiastic => [0.9, 0.5, 0.9, 0.8, 0.3],
            ToneStyle::Calm => [0.4, 0.6, 0.2, 0.6, 0.8],
            ToneStyle::Witty => [0.9, 0.4, 0.7, 0.5, 0.4],
        }
    }

    fn all_styles() -> [ToneStyle; 6] {
        [
            ToneStyle::Friendly,
            ToneStyle::Formal,
            ToneStyle::Casual,
            ToneStyle::Enthusiastic,
            ToneStyle::Calm,
            ToneStyle::Witty,
        ]
    }
}

/// 情感倾向
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub enum EmotionalTendency {
    Optimistic, // 乐观
    #[default]
    Neutral, // 中性
    Cautious,   // 谨慎
    Humorous,   // 幽默
    Professional, // 专业
}

// ─── 用户反馈 ──────────────────────────────────────────────────

/// 用户反馈类型
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum FeedbackType {
    Like,
    Dislike,
    Comment,
}

/// 用户反馈
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserFeedback {
    /// 反馈类型
    pub feedback_type: FeedbackType,
    /// 反馈内容（评论时使用）
    pub content: String,
    /// 关联的技能名
    pub skill_name: String,
    /// 反馈时间
    pub created_at: DateTime<Utc>,
}

// ─── 互动统计 ──────────────────────────────────────────────────

/// 互动统计
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct InteractionStats {
    /// 总互动次数
    pub total_interactions: u32,
    /// 最近活跃时间
    pub last_active_at: DateTime<Utc>,
    /// 各技能使用次数
    pub skill_usage: HashMap<String, u32>,
    /// 平均响应时长(毫秒)
    pub avg_response_time_ms: u64,
    /// 点赞次数
    pub likes: u32,
    /// 点踩次数
    pub dislikes: u32,
    /// 用户反馈列表（最近 N 条）
    pub feedbacks: Vec<UserFeedback>,
}

// ─── 性格快照 ──────────────────────────────────────────────────

/// 人物性格快照
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersonaProfile {
    /// 版本号
    pub version: u32,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后更新时间
    pub updated_at: DateTime<Utc>,

    /// 角色名称
    pub name: String,
    /// 角色描述
    pub description: String,

    /// 语气风格（从性格五维映射）
    pub tone: ToneStyle,
    /// 情感倾向
    pub emotional_tendency: EmotionalTendency,

    /// 性格五维
    pub big_five: BigFive,

    /// 技能熟练度（技能名 -> 熟练度 0-1）
    pub skill_proficiency: HashMap<String, f32>,

    /// 擅长领域及其权重 (领域名 -> 权重 0-1)
    pub expertise_areas: HashMap<String, f32>,

    /// 技能偏好权重（用于技能匹配时加权）
    pub skill_affinity: HashMap<String, f32>,

    /// 近期互动统计
    pub interaction_stats: InteractionStats,

    /// 性格特征关键词
    pub traits: Vec<String>,
}

impl Default for PersonaProfile {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            version: 1,
            created_at: now,
            updated_at: now,
            name: "Subhuti".to_string(),
            description: "一个友善的 AI 助手，乐于帮助用户解决问题，善于学习和成长。".to_string(),
            tone: ToneStyle::Friendly,
            emotional_tendency: EmotionalTendency::Neutral,
            big_five: BigFive::default(),
            skill_proficiency: HashMap::new(),
            expertise_areas: HashMap::from([
                ("聊天对话".to_string(), 0.9),
                ("知识问答".to_string(), 0.8),
                ("天气查询".to_string(), 0.7),
                ("数学计算".to_string(), 0.6),
            ]),
            skill_affinity: HashMap::new(),
            interaction_stats: InteractionStats::default(),
            traits: vec![
                "友善".to_string(),
                "乐于助人".to_string(),
                "善于倾听".to_string(),
                "持续学习".to_string(),
            ],
        }
    }
}

// ─── 历史版本 ──────────────────────────────────────────────────

/// 历史版本记录
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersonaVersion {
    version: u32,
    created_at: DateTime<Utc>,
    profile: PersonaProfile,
    reason: String,
}

// ─── 领域关键词库 ──────────────────────────────────────────────

const DOMAIN_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "聊天对话",
        &[
            "你好",
            "在吗",
            "聊聊",
            "哈哈",
            "嗯",
            "哦",
            "哈哈哈哈",
            "emoji",
            "😊",
        ],
    ),
    (
        "天气查询",
        &[
            "天气", "下雨", "温度", "预报", "气温", "多云", "晴", "雪", "风",
        ],
    ),
    (
        "数学计算",
        &[
            "计算",
            "等于",
            "多少",
            "加减乘除",
            "数学",
            "方程",
            "函数",
            "√",
        ],
    ),
    (
        "知识问答",
        &["什么是", "为什么", "怎么", "如何", "原理", "介绍", "解释"],
    ),
    (
        "编程开发",
        &[
            "代码", "编程", "函数", "bug", "报错", "debug", "接口", "API",
        ],
    ),
    (
        "写作创作",
        &["写", "文章", "文案", "故事", "总结", "报告", "邮件", "信"],
    ),
];

// ─── 心灵层 ────────────────────────────────────────────────────

/// 心灵层配置
#[derive(Debug, Clone)]
pub struct SoulConfig {
    /// 演化触发阈值（每 N 次互动触发一次 LLM 演化）
    pub evolve_threshold: u32,
    /// EMA 学习率（技能熟练度更新速率）
    pub proficiency_alpha: f32,
    /// 领域权重学习率
    pub domain_learning_rate: f32,
    /// 性格五维学习率
    pub trait_learning_rate: f32,
    /// 每次演化的最大变化幅度
    pub max_change_per_evolve: f32,
    /// 统计分析权重（双轨融合）
    pub stat_weight: f32,
    /// LLM 反思权重（双轨融合）
    pub llm_weight: f32,
}

impl Default for SoulConfig {
    fn default() -> Self {
        Self {
            evolve_threshold: 20,
            proficiency_alpha: 0.15,
            domain_learning_rate: 0.1,
            trait_learning_rate: 0.03,
            max_change_per_evolve: 0.2,
            stat_weight: 0.7,
            llm_weight: 0.3,
        }
    }
}

/// 心灵层
#[derive(Debug)]
pub struct SoulLayer {
    /// 当前活跃用户的性格快照（简化：默认用户）
    profile: PersonaProfile,
    /// 多用户性格快照（user_id -> PersonaProfile）
    user_profiles: HashMap<String, PersonaProfile>,
    /// 演化历史（默认用户）
    history: Vec<PersonaVersion>,
    /// 配置
    config: SoulConfig,
    /// 存储路径（文件存储备用）
    storage_path: String,
    /// 上次演化后累计的互动次数（默认用户）
    interactions_since_last_evolve: u32,
    /// 数据库连接（优先使用数据库存储）
    db: Option<Arc<Database>>,
    /// 心灵宫殿（记忆与心灵的统一体）
    memory_palace: Option<Arc<MemoryPalace>>,
}

impl SoulLayer {
    /// 创建新的心灵层
    pub fn new(storage_path: &str) -> Self {
        Self::with_config(storage_path, SoulConfig::default())
    }

    /// 带配置创建
    pub fn with_config(storage_path: &str, config: SoulConfig) -> Self {
        let mut sl = Self {
            profile: PersonaProfile::default(),
            user_profiles: HashMap::new(),
            history: Vec::new(),
            config,
            storage_path: storage_path.to_string(),
            interactions_since_last_evolve: 0,
            db: None,
            memory_palace: None,
        };
        sl.load_from_storage();
        sl
    }

    /// 带数据库连接创建（优先使用数据库存储）
    pub fn with_database(storage_path: &str, db: Arc<Database>) -> Self {
        let mut sl = Self {
            profile: PersonaProfile::default(),
            user_profiles: HashMap::new(),
            history: Vec::new(),
            config: SoulConfig::default(),
            storage_path: storage_path.to_string(),
            interactions_since_last_evolve: 0,
            db: Some(db),
            memory_palace: None,
        };
        sl.load_from_storage();
        sl
    }

    /// 设置数据库连接
    pub fn set_database(&mut self, db: Arc<Database>) {
        self.db = Some(db.clone());
        if let Some(palace) = &self.memory_palace {
            palace.set_database(db);
        }
        // 重新从数据库加载
        self.load_from_storage();
    }

    // ── 心灵宫殿管理 ──────────────────────────────────────

    /// 设置心灵宫殿
    pub fn set_memory_palace(&mut self, palace: Arc<MemoryPalace>) {
        if let Some(db) = &self.db {
            palace.set_database(db.clone());
        }
        self.memory_palace = Some(palace);
    }

    /// 获取心灵宫殿引用
    pub fn memory_palace(&self) -> Option<&Arc<MemoryPalace>> {
        self.memory_palace.as_ref()
    }

    /// 检查是否有心灵宫殿
    pub fn has_memory_palace(&self) -> bool {
        self.memory_palace.is_some()
    }

    /// 获取人格偏好的分区权重（用于记忆检索时的人格影响）
    pub fn get_persona_zone_bias(&self) -> HashMap<MemoryZone, f32> {
        let mut bias = HashMap::new();
        let bf = &self.profile.big_five;

        // 开放性高 → 偏好创意想法、专业知识
        bias.insert(MemoryZone::CreativeIdeas, 0.5 + bf.openness);
        bias.insert(MemoryZone::ExpertKnowledge, 0.5 + bf.openness * 0.8);

        // 尽责性高 → 偏好任务进度
        bias.insert(MemoryZone::TaskProgress, 0.5 + bf.conscientiousness);

        // 外向性高 → 偏好日常对话
        bias.insert(MemoryZone::DailyChat, 0.5 + bf.extraversion);

        // 宜人性高 → 偏好情感记忆
        bias.insert(MemoryZone::Emotional, 0.5 + bf.agreeableness);

        // 情绪稳定性高（谨慎）→ 偏好专业知识、任务进度
        bias.insert(MemoryZone::ExpertKnowledge, 0.5 + bf.neuroticism * 0.5);
        bias.insert(MemoryZone::TaskProgress, 0.5 + bf.neuroticism * 0.3);

        // 默认区中性
        bias.insert(MemoryZone::Default, 1.0);

        bias
    }

    // ── 心灵宫殿高级功能（第三阶段） ──────────────────

    /// 执行遗忘清理周期
    ///
    /// 检查所有记忆，强度低于阈值的会被遗忘
    /// 返回被遗忘的记忆数量
    pub fn run_forget_cycle(&self) -> usize {
        if let Some(palace) = &self.memory_palace {
            palace.run_forget_cycle()
        } else {
            0
        }
    }

    /// 添加记忆关联（联想网络）
    pub fn add_memory_association(&self, memory_id: &str, associated_id: &str) {
        if let Some(palace) = &self.memory_palace {
            palace.add_association(memory_id, associated_id);
        }
    }

    /// 获取联想记忆
    pub fn get_associated_memories(&self, memory_id: &str, depth: usize) -> Vec<PalaceMemory> {
        if let Some(palace) = &self.memory_palace {
            palace.get_associated(memory_id, depth)
        } else {
            Vec::new()
        }
    }

    /// 获取心灵宫殿统计信息
    pub fn palace_stats(&self) -> Option<PalaceStats> {
        self.memory_palace.as_ref().map(|p| p.stats())
    }

    /// 记录互动时同步更新记忆（第三阶段：互动影响记忆）
    ///
    /// - 根据互动内容建立记忆关联
    /// - 增强相关记忆的强度
    fn update_memory_from_interaction(&self, user_input: &str, _skill_name: &str) {
        if let Some(palace) = &self.memory_palace {
            // 搜索相关记忆并激活（增强强度）
            let zone_bias = self.get_persona_zone_bias();
            let results = palace.search(user_input, 5, Some(&zone_bias));

            // 被检索到的记忆会自动激活（在 search 方法内部）

            // 如果有多个相关记忆，建立它们之间的关联
            if results.len() >= 2 {
                let first_id = results[0].memory.base.id.clone();
                for result in results.iter().skip(1).take(results.len().min(3) - 1) {
                    palace.add_association(&first_id, &result.memory.base.id);
                }
            }
        }
    }

    /// 从专家插件设置 persona（切换角色）
    pub fn set_persona_from_expert(&mut self, expert_persona: crate::expert::ExpertPersona) {
        let crate::expert::ExpertPersona {
            name,
            description,
            tone,
            emotional_tendency,
            big_five,
            traits,
            expertise_areas,
            system_prompt: _,
        } = expert_persona;

        self.profile.name = name;
        self.profile.description = description;
        self.profile.tone = tone;
        self.profile.emotional_tendency = emotional_tendency;
        self.profile.big_five = big_five;
        self.profile.traits = traits;
        self.profile.expertise_areas = expertise_areas;
        self.profile.updated_at = Utc::now();

        tracing::info!(
            "SoulLayer: Persona updated from expert: {}",
            self.profile.name
        );
        self.save_to_storage();
    }

    // ── 多用户支持 ──────────────────────────────────────────

    /// 获取指定用户的性格快照
    pub fn get_user_profile(&self, user_id: &str) -> &PersonaProfile {
        if let Some(profile) = self.user_profiles.get(user_id) {
            profile
        } else {
            &self.profile
        }
    }

    /// 获取指定用户的性格快照（可变）
    pub fn get_user_profile_mut(&mut self, user_id: &str) -> &mut PersonaProfile {
        if !self.user_profiles.contains_key(user_id) {
            let default = PersonaProfile {
                name: format!("Subhuti-{}", user_id),
                ..Default::default()
            };
            self.user_profiles.insert(user_id.to_string(), default);
        }
        self.user_profiles.get_mut(user_id).unwrap()
    }

    /// 切换活跃用户
    pub fn switch_user(&mut self, user_id: &str) {
        if let Some(profile) = self.user_profiles.get(user_id) {
            self.profile = profile.clone();
        }
    }

    /// 获取所有用户 ID 列表
    pub fn list_users(&self) -> Vec<String> {
        self.user_profiles.keys().cloned().collect()
    }

    // ── 访问器 ──────────────────────────────────────────

    /// 获取当前性格快照
    pub fn profile(&self) -> &PersonaProfile {
        &self.profile
    }

    /// 获取演化历史
    pub fn history(&self) -> &[PersonaVersion] {
        &self.history
    }

    /// 检查是否需要演化
    pub fn should_evolve(&self) -> bool {
        self.interactions_since_last_evolve >= self.config.evolve_threshold
    }

    /// 获取上次演化后累计互动次数
    pub fn interactions_since_last_evolve(&self) -> u32 {
        self.interactions_since_last_evolve
    }

    // ── 系统提示词注入 ──────────────────────────────────

    /// 获取系统提示词风格注入
    pub fn get_system_prompt_injection(&self) -> String {
        let p = &self.profile;

        format!(
            "\n\n【角色设定】\n\
            角色名称：{}\n\
            角色描述：{}\n\
            语气风格：{}\n\
            情感倾向：{}\n\
            性格特征：{}\n\
            擅长领域：{}\n\
            \n\
            请根据以上角色设定调整你的回答风格。",
            p.name,
            p.description,
            tone_to_str(&p.tone),
            emotion_to_str(&p.emotional_tendency),
            p.traits.join(", "),
            p.expertise_areas
                .iter()
                .map(|(k, v)| format!("{}({:.0}%)", k, v * 100.0))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// 获取技能偏好权重（用于技能匹配时加权）
    pub fn get_skill_weight(&self, skill_name: &str) -> f32 {
        // 优先使用 skill_affinity，其次用熟练度，默认 1.0
        self.profile
            .skill_affinity
            .get(skill_name)
            .copied()
            .unwrap_or_else(|| {
                self.profile
                    .skill_proficiency
                    .get(skill_name)
                    .copied()
                    .unwrap_or(0.5)
            })
    }

    /// 记录用户反馈（点赞/踩/评论）
    pub fn record_feedback(
        &mut self,
        feedback_type: FeedbackType,
        content: &str,
        skill_name: &str,
    ) {
        let stats = &mut self.profile.interaction_stats;

        match feedback_type {
            FeedbackType::Like => {
                stats.likes += 1;
                // 点赞 → 宜人性 + 外向性 +
                let bf = &mut self.profile.big_five;
                bf.agreeableness += 0.05 * self.config.trait_learning_rate;
                bf.extraversion += 0.03 * self.config.trait_learning_rate;
                bf.clamp();
            }
            FeedbackType::Dislike => {
                stats.dislikes += 1;
                // 点踩 → 尽责性 +（更加谨慎）、开放性 +（需要改进）
                let bf = &mut self.profile.big_five;
                bf.conscientiousness += 0.05 * self.config.trait_learning_rate;
                bf.openness += 0.03 * self.config.trait_learning_rate;
                bf.clamp();
            }
            FeedbackType::Comment => {}
        }

        // 添加到反馈列表（保留最近 20 条）
        stats.feedbacks.insert(
            0,
            UserFeedback {
                feedback_type: feedback_type.clone(),
                content: content.to_string(),
                skill_name: skill_name.to_string(),
                created_at: Utc::now(),
            },
        );
        if stats.feedbacks.len() > 20 {
            stats.feedbacks.pop();
        }

        // 更新 traits
        self.update_traits();
        self.profile.updated_at = Utc::now();

        // 写入数据库反馈表
        if let Some(db) = &self.db {
            let fb_type = match feedback_type {
                FeedbackType::Like => "like",
                FeedbackType::Dislike => "dislike",
                FeedbackType::Comment => "comment",
            };
            let db_clone = db.clone();
            let content_owned = content.to_string();
            let skill_owned = skill_name.to_string();
            tokio::task::spawn(async move {
                if let Err(e) = db_clone
                    .add_feedback("default", fb_type, &content_owned, &skill_owned)
                    .await
                {
                    tracing::warn!("SoulLayer: Failed to write feedback to DB: {}", e);
                }
            });
        }

        self.save_to_storage();
    }

    /// 获取反馈统计
    pub fn feedback_stats(&self) -> (u32, u32) {
        (
            self.profile.interaction_stats.likes,
            self.profile.interaction_stats.dislikes,
        )
    }

    // ── 统计分析轨道（每次互动调用） ────────────────────

    /// 记录一次互动并触发统计更新
    pub fn record_interaction(
        &mut self,
        skill_name: &str,
        user_input: &str,
        response_time_ms: u64,
    ) {
        let now = Utc::now();
        self.interactions_since_last_evolve += 1;

        // 1. 更新互动统计
        self.profile.interaction_stats.total_interactions += 1;
        self.profile.interaction_stats.last_active_at = now;

        let entry = self
            .profile
            .interaction_stats
            .skill_usage
            .entry(skill_name.to_string())
            .or_insert(0);
        *entry += 1;

        // 更新平均响应时间（EMA）
        let avg = &mut self.profile.interaction_stats.avg_response_time_ms;
        *avg = ((*avg as f32) * 0.9 + response_time_ms as f32 * 0.1) as u64;

        // 2. 更新技能熟练度
        self.update_skill_proficiency(skill_name);

        // 3. 更新领域权重
        self.update_expertise_areas(user_input, skill_name);

        // 4. 更新性格五维
        self.update_big_five(user_input, skill_name);

        // 5. 重新计算语气风格
        self.update_tone_from_big_five();

        // 6. 更新 traits 关键词
        self.update_traits();

        // 7. 记忆宫殿：互动影响记忆（双向影响）
        self.update_memory_from_interaction(user_input, skill_name);

        self.profile.updated_at = now;
        self.save_to_storage();
    }

    /// 更新技能熟练度（S 型曲线 + EMA）
    fn update_skill_proficiency(&mut self, skill_name: &str) {
        let count = self
            .profile
            .interaction_stats
            .skill_usage
            .get(skill_name)
            .copied()
            .unwrap_or(0);

        // S 型曲线：使用 10 次左右达到 50% 熟练度
        let midpoint = 10.0;
        let signal = 1.0 / (1.0 + (-(count as f32 - midpoint) / 3.0).exp());

        let current = self
            .profile
            .skill_proficiency
            .get(skill_name)
            .copied()
            .unwrap_or(0.3);

        // EMA 更新
        let new_val = current * (1.0 - self.config.proficiency_alpha)
            + signal * self.config.proficiency_alpha;

        self.profile
            .skill_proficiency
            .insert(skill_name.to_string(), new_val);

        // 同步更新 skill_affinity（熟练度影响偏好）
        let affinity = self
            .profile
            .skill_affinity
            .entry(skill_name.to_string())
            .or_insert(1.0);
        *affinity = (*affinity * 0.9 + new_val * 0.1).clamp(0.5, 1.5);
    }

    /// 更新领域权重（关键词匹配 + 衰减）
    fn update_expertise_areas(&mut self, user_input: &str, skill_name: &str) {
        let lr = self.config.domain_learning_rate;

        for (domain, keywords) in DOMAIN_KEYWORDS {
            let current = self
                .profile
                .expertise_areas
                .get(*domain)
                .copied()
                .unwrap_or(0.3);

            let mut delta = -0.005; // 自然衰减

            // 命中关键词计数
            let hits = keywords
                .iter()
                .filter(|kw| user_input.contains(*kw))
                .count();
            delta += hits as f32 * 0.02;

            // 技能调用关联
            let skill_domain_match = match skill_name {
                "weather" => *domain == "天气查询",
                "calculator" => *domain == "数学计算",
                "default_chat" => *domain == "聊天对话",
                "search_long_memory" => *domain == "知识问答",
                _ => false,
            };
            if skill_domain_match {
                delta += 0.05;
            }

            let new_val = (current + delta * lr).clamp(0.1, 1.0);
            self.profile
                .expertise_areas
                .insert(domain.to_string(), new_val);
        }
    }

    /// 更新性格五维（信号触发）
    fn update_big_five(&mut self, user_input: &str, skill_name: &str) {
        let lr = self.config.trait_learning_rate;
        let bf = &mut self.profile.big_five;

        let mut signals = BigFive {
            openness: 0.0,
            conscientiousness: 0.0,
            extraversion: 0.0,
            agreeableness: 0.0,
            neuroticism: 0.0,
        };

        // ── 开放性信号 ──
        // 首次使用新技能 → 开放性 +
        let skill_count = self.profile.interaction_stats.skill_usage.len();
        if skill_count > 1 {
            signals.openness += 0.2;
        }
        // 问"为什么"、"怎么" → 开放性 +
        if user_input.contains("为什么")
            || user_input.contains("怎么")
            || user_input.contains("原理")
        {
            signals.openness += 0.3;
        }

        // ── 尽责性信号 ──
        // 使用计算器 → 尽责性 +
        if skill_name == "calculator" {
            signals.conscientiousness += 0.4;
        }
        // 包含数字、精确查询 → 尽责性 +
        let digit_count = user_input.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count >= 3 {
            signals.conscientiousness += 0.2;
        }

        // ── 外向性信号 ──
        // 闲聊类技能 → 外向性 +
        if skill_name == "default_chat" {
            signals.extraversion += 0.3;
        }
        // 包含语气词 → 外向性 +
        let casual_words = ["哈哈", "嘿嘿", "哇", "呀", "呢", "嘛", "啦"];
        if casual_words.iter().any(|w| user_input.contains(w)) {
            signals.extraversion += 0.3;
        }
        // 输入很短 → 外向性 +（闲聊通常短句）
        if user_input.len() < 10 {
            signals.extraversion += 0.2;
        }

        // ── 宜人性信号 ──
        // 用户表达感谢 → 宜人性 +
        let thanks_words = ["谢谢", "感谢", "太好了", "真棒", "厉害"];
        if thanks_words.iter().any(|w| user_input.contains(w)) {
            signals.agreeableness += 0.5;
        }
        // 礼貌用语 → 宜人性 +
        let polite_words = ["请", "麻烦", "不好意思", "请问"];
        if polite_words.iter().any(|w| user_input.contains(w)) {
            signals.agreeableness += 0.2;
        }

        // ── 情绪稳定性信号 ──
        // 用户抱怨/不满 → 谨慎度 +
        let negative_words = ["不对", "错了", "怎么回事", "不行", "没用"];
        if negative_words.iter().any(|w| user_input.contains(w)) {
            signals.neuroticism += 0.4;
        }
        // 用户多次重试 → 谨慎度 +（通过 session 信息，但这里简化处理）

        // 应用更新
        bf.openness += signals.openness * lr;
        bf.conscientiousness += signals.conscientiousness * lr;
        bf.extraversion += signals.extraversion * lr;
        bf.agreeableness += signals.agreeableness * lr;
        bf.neuroticism += signals.neuroticism * lr;

        bf.clamp();
    }

    /// 从性格五维更新语气风格（余弦相似度匹配）
    fn update_tone_from_big_five(&mut self) {
        let current_vec = self.profile.big_five.to_vec();
        let mut best_style = ToneStyle::Friendly;
        let mut best_sim = -1.0;

        for style in ToneStyle::all_styles() {
            let sim = cosine_similarity(&current_vec, &style.feature_vector());
            if sim > best_sim {
                best_sim = sim;
                best_style = style;
            }
        }

        self.profile.tone = best_style;

        // 同步更新情感倾向
        self.profile.emotional_tendency = match self.profile.tone {
            ToneStyle::Witty | ToneStyle::Casual => EmotionalTendency::Humorous,
            ToneStyle::Formal | ToneStyle::Calm => EmotionalTendency::Professional,
            ToneStyle::Enthusiastic => EmotionalTendency::Optimistic,
            ToneStyle::Friendly => EmotionalTendency::Neutral,
        };
    }

    /// 更新性格特征关键词
    fn update_traits(&mut self) {
        let bf = &self.profile.big_five;
        let mut traits = Vec::new();

        if bf.agreeableness > 0.65 {
            traits.push("友善".to_string());
            traits.push("乐于助人".to_string());
        }
        if bf.openness > 0.65 {
            traits.push("好奇心强".to_string());
            traits.push("善于学习".to_string());
        }
        if bf.conscientiousness > 0.6 {
            traits.push("严谨".to_string());
            traits.push("精确".to_string());
        }
        if bf.extraversion > 0.6 {
            traits.push("活泼".to_string());
            traits.push("热情".to_string());
        }
        if bf.neuroticism > 0.6 {
            traits.push("谨慎".to_string());
        }
        if traits.is_empty() {
            traits.push("温和".to_string());
            traits.push("可靠".to_string());
        }

        self.profile.traits = traits;
    }

    // ── LLM 自反思轨道（周期性） ───────────────────────

    /// 手动触发演化（使用 LLM 自反思 + 统计分析双轨融合）
    ///
    /// 优先使用心灵宫殿（带人格影响的记忆检索），如果没有则使用传入的 memory
    pub async fn evolve(&mut self, runtime: &Runtime, memory: &Memory) -> Result<()> {
        tracing::info!(
            "SoulLayer: Starting persona evolution (v{} -> ?)",
            self.profile.version
        );

        // 1. 收集近期记忆作为分析素材
        let mut analysis_text = String::new();

        // 使用心灵宫殿（如果有），否则用普通 memory
        if let Some(palace) = &self.memory_palace.clone() {
            // ── 心灵宫殿模式：人格影响检索 ──
            let zone_bias = self.get_persona_zone_bias();

            // 短期记忆摘要
            let short_term_summary = palace.summarize_short_term();
            if !short_term_summary.is_empty() {
                analysis_text.push_str(&format!("【近期对话摘要】\n{}\n\n", short_term_summary));
            }

            // 使用人格偏好检索记忆（更符合当前性格的记忆优先）
            let personality_results = palace.search("", 20, Some(&zone_bias));
            if !personality_results.is_empty() {
                analysis_text.push_str("【性格相关记忆】\n");
                for (i, result) in personality_results.iter().enumerate() {
                    let zone_name = result.memory.zone.name();
                    let strength = result.memory.strength;
                    analysis_text.push_str(&format!(
                        "\n[{}] 记忆{} (强度: {:.0}%):\n{}\n",
                        zone_name,
                        i + 1,
                        strength * 100.0,
                        result.memory.base.content
                    ));
                }
            }

            // 各分区记忆概览
            let stats = palace.stats();
            analysis_text.push_str("\n【记忆宫殿分区统计】\n");
            for zone in MemoryZone::all() {
                let count = stats.zone_counts.get(&zone).copied().unwrap_or(0);
                analysis_text.push_str(&format!("- {}: {}条\n", zone.name(), count));
            }
        } else {
            // ── 普通模式：直接从 memory 读取 ──
            let short_term_summary = memory.summarize_short_term();
            if !short_term_summary.is_empty() {
                analysis_text.push_str(&format!("【近期对话摘要】\n{}\n\n", short_term_summary));
            }

            let recent_archive = memory.search_archive("", 15);
            if !recent_archive.is_empty() {
                analysis_text.push_str("【近期对话记录】\n");
                for (i, result) in recent_archive.iter().enumerate() {
                    analysis_text.push_str(&format!(
                        "\n对话 {}:\n{}\n",
                        i + 1,
                        result.item.content
                    ));
                }
            }
        }

        // 2. 当前性格快照（统计分析轨道的结果）
        let stat_profile = serde_json::to_string_pretty(&self.profile)?;
        analysis_text.push_str(&format!("\n【当前统计性格】\n{}", stat_profile));

        // 3. 技能使用统计
        let skill_stats: Vec<String> = self
            .profile
            .interaction_stats
            .skill_usage
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}: {}次 (熟练度: {:.0}%)",
                    k,
                    v,
                    self.profile
                        .skill_proficiency
                        .get(k)
                        .copied()
                        .unwrap_or(0.0)
                        * 100.0
                )
            })
            .collect();
        analysis_text.push_str(&format!("\n【技能使用统计】\n{}", skill_stats.join("\n")));

        // 4. 构建 LLM 自反思提示词
        let prompt = format!(
            "你是一个 AI 角色性格分析专家。请分析以下近期互动记录和当前性格数据，\
            给出性格调整建议。\n\n\
            分析素材：\n{}\n\n\
            请输出纯 JSON 格式的调整建议，包含以下字段：\n\
            - tone: 语气风格（Friendly/Formal/Casual/Enthusiastic/Calm/Witty）\n\
            - emotional_tendency: 情感倾向（Optimistic/Neutral/Cautious/Humorous/Professional）\n\
            - traits: 性格特征关键词数组（中文）\n\
            - big_five_adjustments: 五维调整建议对象（openness/conscientiousness/extraversion/agreeableness/neuroticism -> 0-1分数）\n\
            - expertise_areas: 擅长领域权重对象（领域名 -> 0-1权重）\n\
            - skill_affinity: 技能偏好权重对象（技能名 -> 0.5-1.5权重）\n\
            - reason: 调整原因说明（中文）\n\n\
            注意：\n\
            1. 调整应该是渐进的，参考当前统计数据，不要突变\n\
            2. 基于实际互动数据来调整\n\
            3. 输出纯 JSON，不要包含任何其他文本或代码块标记",
            analysis_text
        );

        // 5. 调用 LLM 进行自反思
        let messages = vec![
            crate::runtime::llm::Message {
                role: crate::runtime::llm::Role::System,
                content: "你是一个 AI 角色性格分析专家。".to_string(),
                tool_call_id: None,
            },
            crate::runtime::llm::Message {
                role: crate::runtime::llm::Role::User,
                content: prompt,
                tool_call_id: None,
            },
        ];
        let response = runtime.call_llm_with_stats(messages).await?;

        // 6. 解析 LLM 响应
        let response_text = response.content.trim();
        tracing::info!(
            "SoulLayer: LLM evolution response (first 200): {}",
            &response_text[..response_text.len().min(200)]
        );

        // 提取 JSON 部分
        let json_start = response_text.find('{').unwrap_or(0);
        let json_end = response_text.rfind('}').unwrap_or(response_text.len()) + 1;
        let json_str = &response_text[json_start..json_end];

        let llm_suggestion: EvolutionSuggestion = match serde_json::from_str(json_str) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("SoulLayer: Failed to parse LLM evolution JSON: {}", e);
                // LLM 解析失败，只用统计分析结果
                self.finalize_evolution("统计分析更新（LLM 解析失败）".to_string());
                return Ok(());
            }
        };

        // 7. 双轨融合：统计分析 (70%) + LLM 建议 (30%)
        self.merge_llm_suggestion(&llm_suggestion);

        // 8. 保存新版本
        self.finalize_evolution(llm_suggestion.reason);

        tracing::info!(
            "SoulLayer: Persona evolved to version {}",
            self.profile.version
        );
        Ok(())
    }

    /// 使用心灵宫殿演化（当心灵宫殿已设置时，优先使用此方法）
    pub async fn evolve_with_palace(&mut self, runtime: &Runtime) -> Result<()> {
        if let Some(palace) = &self.memory_palace.clone() {
            // 使用心灵宫殿的底层 memory 调用 evolve（内部会检测到 palace 存在并使用）
            self.evolve(runtime, palace.base_memory()).await
        } else {
            Err(anyhow::anyhow!("Memory palace not set"))
        }
    }

    /// 融合 LLM 建议到当前性格
    fn merge_llm_suggestion(&mut self, suggestion: &EvolutionSuggestion) {
        let w_stat = self.config.stat_weight;
        let w_llm = self.config.llm_weight;
        let max_delta = self.config.max_change_per_evolve;

        // 性格五维融合
        let bf = &mut self.profile.big_five;
        let llm_bf = &suggestion.big_five_adjustments;

        fn merge(current: &mut f32, target: f32, w_stat: f32, w_llm: f32, max_delta: f32) {
            let blended = *current * w_stat + target * w_llm;
            let delta = (blended - *current).clamp(-max_delta, max_delta);
            *current = (*current + delta).clamp(0.0, 1.0);
        }

        merge(&mut bf.openness, llm_bf.openness, w_stat, w_llm, max_delta);
        merge(
            &mut bf.conscientiousness,
            llm_bf.conscientiousness,
            w_stat,
            w_llm,
            max_delta,
        );
        merge(
            &mut bf.extraversion,
            llm_bf.extraversion,
            w_stat,
            w_llm,
            max_delta,
        );
        merge(
            &mut bf.agreeableness,
            llm_bf.agreeableness,
            w_stat,
            w_llm,
            max_delta,
        );
        merge(
            &mut bf.neuroticism,
            llm_bf.neuroticism,
            w_stat,
            w_llm,
            max_delta,
        );

        // 领域权重融合
        for (domain, llm_weight) in &suggestion.expertise_areas {
            if let Some(current) = self.profile.expertise_areas.get_mut(domain) {
                let blended = *current * w_stat + *llm_weight * w_llm;
                let delta = (blended - *current).clamp(-max_delta, max_delta);
                *current = (*current + delta).clamp(0.1, 1.0);
            } else {
                self.profile
                    .expertise_areas
                    .insert(domain.clone(), *llm_weight * w_llm);
            }
        }

        // 技能偏好融合
        for (skill, llm_weight) in &suggestion.skill_affinity {
            let current = self
                .profile
                .skill_affinity
                .entry(skill.clone())
                .or_insert(1.0);
            let blended = *current * w_stat + *llm_weight * w_llm;
            *current = blended.clamp(0.5, 1.5);
        }

        // traits 直接用 LLM 建议
        self.profile.traits = suggestion.traits.clone();

        // 语气风格重新计算
        self.update_tone_from_big_five();
    }

    /// 完成演化：保存历史、更新版本
    fn finalize_evolution(&mut self, reason: String) {
        // 保存旧版本到历史
        self.history.push(PersonaVersion {
            version: self.profile.version,
            created_at: self.profile.created_at,
            profile: self.profile.clone(),
            reason: reason.clone(),
        });

        // 限制历史记录数量
        if self.history.len() > 20 {
            self.history.remove(0);
        }

        // 更新版本
        self.profile.version += 1;
        self.profile.updated_at = Utc::now();
        self.interactions_since_last_evolve = 0;

        // 写入数据库演化历史表
        if let Some(db) = &self.db {
            let db_clone = db.clone();
            let version = self.profile.version as i32;
            let snapshot = match serde_json::to_value(&self.profile) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("SoulLayer: Failed to serialize profile snapshot: {}", e);
                    self.save_to_storage();
                    return;
                }
            };
            let reason_clone = reason.clone();
            tokio::task::spawn(async move {
                if let Err(e) = db_clone
                    .add_history("default", version, &snapshot, &reason_clone)
                    .await
                {
                    tracing::warn!("SoulLayer: Failed to write history to DB: {}", e);
                }
            });
        }

        self.save_to_storage();
    }

    /// 应用演化建议（双轨融合）
    pub fn apply_evolution(&mut self, suggestion: EvolutionSuggestion) {
        let w_stat = self.config.stat_weight;
        let w_llm = self.config.llm_weight;
        let max_delta = self.config.max_change_per_evolve;

        // 性格五维融合
        let bf = &mut self.profile.big_five;
        let llm_bf = &suggestion.big_five_adjustments;

        fn merge(current: &mut f32, target: f32, w_stat: f32, w_llm: f32, max_delta: f32) {
            let blended = *current * w_stat + target * w_llm;
            let delta = (blended - *current).clamp(-max_delta, max_delta);
            *current = (*current + delta).clamp(0.0, 1.0);
        }

        merge(&mut bf.openness, llm_bf.openness, w_stat, w_llm, max_delta);
        merge(
            &mut bf.conscientiousness,
            llm_bf.conscientiousness,
            w_stat,
            w_llm,
            max_delta,
        );
        merge(
            &mut bf.extraversion,
            llm_bf.extraversion,
            w_stat,
            w_llm,
            max_delta,
        );
        merge(
            &mut bf.agreeableness,
            llm_bf.agreeableness,
            w_stat,
            w_llm,
            max_delta,
        );
        merge(
            &mut bf.neuroticism,
            llm_bf.neuroticism,
            w_stat,
            w_llm,
            max_delta,
        );

        // 领域权重融合
        for (domain, llm_weight) in &suggestion.expertise_areas {
            if let Some(current) = self.profile.expertise_areas.get_mut(domain) {
                let blended = *current * w_stat + *llm_weight * w_llm;
                let delta = (blended - *current).clamp(-max_delta, max_delta);
                *current = (*current + delta).clamp(0.1, 1.0);
            } else {
                self.profile
                    .expertise_areas
                    .insert(domain.clone(), *llm_weight * w_llm);
            }
        }

        // 技能偏好融合
        for (skill, llm_weight) in &suggestion.skill_affinity {
            let current = self
                .profile
                .skill_affinity
                .entry(skill.clone())
                .or_insert(1.0);
            let blended = *current * w_stat + *llm_weight * w_llm;
            *current = blended.clamp(0.5, 1.5);
        }

        // traits 直接用 LLM 建议
        self.profile.traits = suggestion.traits;

        // 语气风格重新计算
        self.update_tone_from_big_five();

        // 保存新版本
        self.finalize_evolution(suggestion.reason);
    }

    /// 仅递增版本号（LLM 解析失败时使用）
    pub fn increment_version(&mut self, reason: String) {
        self.finalize_evolution(reason);
    }

    // ── 持久化 ──────────────────────────────────────────

    fn load_from_storage(&mut self) {
        // 优先从数据库加载
        if let Some(_db) = &self.db {
            if let Ok(result) = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(async { self.load_from_database_async("default").await })
            }) {
                if result {
                    tracing::info!(
                        "SoulLayer: Loaded from database (version {}, {} users)",
                        self.profile.version,
                        self.user_profiles.len()
                    );
                    return;
                }
            }
        }

        // 如果数据库加载失败或无数据库,从文件加载
        let path = Path::new(&self.storage_path);
        if path.exists() {
            match fs::read_to_string(path) {
                Ok(content) => {
                    if let Ok(data) = serde_json::from_str::<SoulLayerData>(&content) {
                        self.profile = data.profile;
                        self.user_profiles = data.user_profiles;
                        self.history = data.history;
                        self.interactions_since_last_evolve = data.interactions_since_last_evolve;
                        tracing::info!(
                            "SoulLayer: Loaded from file storage (version {}, {} users)",
                            self.profile.version,
                            self.user_profiles.len()
                        );

                        // 如果有数据库连接,将文件数据同步到数据库
                        if let Some(_db) = &self.db {
                            if let Err(e) = tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current()
                                    .block_on(async { self.sync_to_database_async().await })
                            }) {
                                tracing::warn!("SoulLayer: Failed to sync to database: {}", e);
                            } else {
                                tracing::info!("SoulLayer: Synced file data to database");
                            }
                        }
                        return;
                    }
                }
                Err(e) => tracing::warn!("SoulLayer: Failed to read storage: {}", e),
            }
        }
        tracing::info!("SoulLayer: Initialized default profile");
        self.save_to_storage();
    }

    fn save_to_storage(&self) {
        // 优先保存到数据库
        if let Some(_db) = &self.db {
            if let Err(e) = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(async { self.save_to_database_async("default").await })
            }) {
                tracing::warn!("SoulLayer: Failed to save to database: {}", e);
            } else {
                tracing::debug!("SoulLayer: Saved to database successfully");
                return;
            }
        }

        // 如果数据库保存失败或无数据库,保存到文件
        let data = SoulLayerData {
            profile: self.profile.clone(),
            user_profiles: self.user_profiles.clone(),
            history: self.history.clone(),
            interactions_since_last_evolve: self.interactions_since_last_evolve,
        };

        if let Ok(json) = serde_json::to_string_pretty(&data) {
            if let Some(parent) = Path::new(&self.storage_path).parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut file) = File::create(&self.storage_path) {
                let _ = file.write_all(json.as_bytes());
            }
        }
    }

    /// 从数据库加载指定用户的性格数据(异步版本)
    async fn load_from_database_async(&mut self, user_id: &str) -> Result<bool> {
        if let Some(db) = &self.db {
            // 加载指定用户的 persona
            if let Some(row) = db.get_persona(user_id).await? {
                self.profile = persona_row_to_profile(&row);
                tracing::debug!(
                    "SoulLayer: Loaded persona for user {} (version {})",
                    user_id,
                    self.profile.version
                );
            } else {
                // 如果数据库中没有该用户,使用默认值并保存
                self.profile = PersonaProfile::default();
                self.save_to_database_async(user_id).await?;
                tracing::info!("SoulLayer: Created default persona for user {}", user_id);
            }

            // 加载所有用户列表
            let users = db.list_users().await?;
            for uid in users {
                if uid != user_id && uid != "default" {
                    if let Some(row) = db.get_persona(&uid).await? {
                        self.user_profiles.insert(uid, persona_row_to_profile(&row));
                    }
                }
            }

            return Ok(true);
        }
        Ok(false)
    }

    /// 保存指定用户的性格数据到数据库(异步版本)
    async fn save_to_database_async(&self, user_id: &str) -> Result<()> {
        if let Some(db) = &self.db {
            let persona_data = profile_to_persona_data(&self.profile);
            db.upsert_persona(user_id, &persona_data).await?;

            // 同步保存所有用户的 profile
            for (uid, profile) in &self.user_profiles {
                if uid != user_id {
                    let data = profile_to_persona_data(profile);
                    db.upsert_persona(uid, &data).await?;
                }
            }

            tracing::debug!("SoulLayer: Saved persona to database for user {}", user_id);
        }
        Ok(())
    }

    /// 将文件数据同步到数据库(异步版本)
    async fn sync_to_database_async(&self) -> Result<()> {
        if let Some(db) = &self.db {
            // 保存默认用户
            let default_data = profile_to_persona_data(&self.profile);
            db.upsert_persona("default", &default_data).await?;

            // 保存所有用户
            for (uid, profile) in &self.user_profiles {
                let data = profile_to_persona_data(profile);
                db.upsert_persona(uid, &data).await?;
            }

            tracing::info!(
                "SoulLayer: Synced {} user profiles to database",
                self.user_profiles.len() + 1
            );
        }
        Ok(())
    }
}

// ─── LLM 演化建议结构 ──────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct EvolutionSuggestion {
    #[allow(dead_code)]
    pub tone: ToneStyle,
    #[allow(dead_code)]
    pub emotional_tendency: EmotionalTendency,
    pub traits: Vec<String>,
    pub big_five_adjustments: BigFiveSuggestion,
    pub expertise_areas: HashMap<String, f32>,
    pub skill_affinity: HashMap<String, f32>,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BigFiveSuggestion {
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

// ─── 存储数据结构 ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SoulLayerData {
    profile: PersonaProfile,
    user_profiles: HashMap<String, PersonaProfile>,
    history: Vec<PersonaVersion>,
    interactions_since_last_evolve: u32,
}

// ─── 工具函数 ──────────────────────────────────────────────────

/// 余弦相似度
fn cosine_similarity(a: &[f32; 5], b: &[f32; 5]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

fn tone_to_str(tone: &ToneStyle) -> &str {
    match tone {
        ToneStyle::Friendly => "友好",
        ToneStyle::Formal => "正式",
        ToneStyle::Casual => "随意",
        ToneStyle::Enthusiastic => "热情",
        ToneStyle::Calm => "冷静",
        ToneStyle::Witty => "机智",
    }
}

fn emotion_to_str(emotion: &EmotionalTendency) -> &str {
    match emotion {
        EmotionalTendency::Optimistic => "乐观",
        EmotionalTendency::Neutral => "中性",
        EmotionalTendency::Cautious => "谨慎",
        EmotionalTendency::Humorous => "幽默",
        EmotionalTendency::Professional => "专业",
    }
}

// ─── 数据转换函数 ──────────────────────────────────────────────

/// 将 PersonaRow (数据库行) 转换为 PersonaProfile
fn persona_row_to_profile(row: &crate::memory::storage::PersonaRow) -> PersonaProfile {
    // 解析 JSON 字段
    let traits: Vec<String> = serde_json::from_value(row.traits.clone()).unwrap_or_default();
    let skill_proficiency: HashMap<String, f32> =
        serde_json::from_value(row.skill_proficiency.clone()).unwrap_or_default();
    let expertise_areas: HashMap<String, f32> =
        serde_json::from_value(row.expertise_areas.clone()).unwrap_or_default();
    let skill_affinity: HashMap<String, f32> =
        serde_json::from_value(row.skill_affinity.clone()).unwrap_or_default();
    let skill_usage: HashMap<String, u32> =
        serde_json::from_value(row.skill_usage.clone()).unwrap_or_default();

    // 转换枚举类型
    let tone = match row.tone.as_str() {
        "Friendly" => ToneStyle::Friendly,
        "Formal" => ToneStyle::Formal,
        "Casual" => ToneStyle::Casual,
        "Enthusiastic" => ToneStyle::Enthusiastic,
        "Calm" => ToneStyle::Calm,
        "Witty" => ToneStyle::Witty,
        _ => ToneStyle::Friendly,
    };

    let emotional_tendency = match row.emotional_tendency.as_str() {
        "Optimistic" => EmotionalTendency::Optimistic,
        "Neutral" => EmotionalTendency::Neutral,
        "Cautious" => EmotionalTendency::Cautious,
        "Humorous" => EmotionalTendency::Humorous,
        "Professional" => EmotionalTendency::Professional,
        _ => EmotionalTendency::Neutral,
    };

    PersonaProfile {
        version: row.version as u32,
        created_at: row.created_at,
        updated_at: row.updated_at,
        name: row.name.clone(),
        description: row.description.clone(),
        tone,
        emotional_tendency,
        big_five: BigFive {
            openness: row.openness,
            conscientiousness: row.conscientiousness,
            extraversion: row.extraversion,
            agreeableness: row.agreeableness,
            neuroticism: row.neuroticism,
        },
        skill_proficiency,
        expertise_areas,
        skill_affinity,
        interaction_stats: InteractionStats {
            total_interactions: row.total_interactions as u32,
            last_active_at: row.updated_at,
            skill_usage,
            avg_response_time_ms: row.avg_response_time_ms as u64,
            likes: row.likes as u32,
            dislikes: row.dislikes as u32,
            feedbacks: Vec::new(), // 反馈列表需要单独加载
        },
        traits,
    }
}

/// 将 PersonaProfile 转换为 PersonaData (用于数据库存储)
fn profile_to_persona_data(profile: &PersonaProfile) -> crate::memory::storage::PersonaData {
    // 转换 skill_usage 的类型: u32 -> i32
    let skill_usage: HashMap<String, i32> = profile
        .interaction_stats
        .skill_usage
        .iter()
        .map(|(k, v)| (k.clone(), *v as i32))
        .collect();

    crate::memory::storage::PersonaData {
        version: profile.version as i32,
        name: profile.name.clone(),
        description: profile.description.clone(),
        tone: tone_to_str(&profile.tone).to_string(),
        emotional_tendency: emotion_to_str(&profile.emotional_tendency).to_string(),
        openness: profile.big_five.openness,
        conscientiousness: profile.big_five.conscientiousness,
        extraversion: profile.big_five.extraversion,
        agreeableness: profile.big_five.agreeableness,
        neuroticism: profile.big_five.neuroticism,
        traits: profile.traits.clone(),
        skill_proficiency: profile.skill_proficiency.clone(),
        expertise_areas: profile.expertise_areas.clone(),
        skill_affinity: profile.skill_affinity.clone(),
        total_interactions: profile.interaction_stats.total_interactions as i32,
        likes: profile.interaction_stats.likes as i32,
        dislikes: profile.interaction_stats.dislikes as i32,
        avg_response_time_ms: profile.interaction_stats.avg_response_time_ms as i64,
        skill_usage,
    }
}
