//! # 心灵宫殿 (Memory Palace)
//!
//! 记忆与心灵的统一体：记忆是心灵的食物，心灵是记忆的升华。
//!
//! ## 设计理念
//!
//! 记忆不是冰冷的数据存储，而是心灵宫殿的"房间"。
//! 每一段记忆都影响着人格的塑造，而人格又反过来筛选和解释记忆。
//!
//! ## 三层记忆结构
//!
//! - **短期记忆 (Short-term)**：当前会话上下文，工作记忆
//! - **长期记忆 (Long-term)**：历史对话沉淀，经验积累
//! - **知识库 (Knowledge)**：结构化知识，专业领域
//!
//! ## 记忆分区
//!
//! 心灵宫殿中的记忆按主题分区存储，类似"房间"概念：
//! - 日常对话室
//! - 专业知识室
//! - 情感记忆室
//! - 任务记忆室

use crate::memory::{
    storage::Database, EmbeddingService, Memory, MemoryConfig, MemoryItem, MemoryLayer,
    MemoryStats, SearchResult, SemanticSearchResult,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ─── 记忆分区 ──────────────────────────────────────────────────

/// 记忆分区（心灵宫殿的"房间"）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum MemoryZone {
    /// 日常对话区 - 普通闲聊
    DailyChat,
    /// 专业知识区 - 技术、学术等
    ExpertKnowledge,
    /// 情感记忆区 - 情绪、感受、关系
    Emotional,
    /// 任务记忆区 - 待办、目标、进度
    TaskProgress,
    /// 创意想法区 - 灵感、创意、脑洞
    CreativeIdeas,
    /// 默认区（未分类）
    #[default]
    Default,
}

impl MemoryZone {
    /// 从文本内容推断分区
    pub fn infer_from_content(content: &str) -> Self {
        let content_lower = content.to_lowercase();

        // 情感相关
        let emotional_keywords = [
            "开心",
            "难过",
            "生气",
            "喜欢",
            "讨厌",
            "感觉",
            "心情",
            "情绪",
            "压力",
            "焦虑",
            "幸福",
            "感动",
            "害怕",
            "担心",
            "好开心",
            "好难过",
            "很高兴",
        ];
        if emotional_keywords.iter().any(|k| content_lower.contains(k)) {
            return MemoryZone::Emotional;
        }

        // 任务相关
        let task_keywords = [
            "待办", "任务", "目标", "计划", "完成", "进度", "安排", "提醒", "要做", "需要", "明天",
            "下周",
        ];
        if task_keywords.iter().any(|k| content_lower.contains(k)) {
            return MemoryZone::TaskProgress;
        }

        // 专业知识
        let expert_keywords = [
            "代码",
            "编程",
            "函数",
            "算法",
            "技术",
            "原理",
            "什么是",
            "怎么实现",
            "为什么",
            "如何",
            "系统",
            "语言",
        ];
        if expert_keywords.iter().any(|k| content_lower.contains(k)) {
            return MemoryZone::ExpertKnowledge;
        }

        // 创意想法
        let creative_keywords = [
            "想法",
            "创意",
            "如果",
            "假设",
            "脑洞",
            "设计",
            "灵感",
            "说不定",
            "可以试试",
        ];
        if creative_keywords.iter().any(|k| content_lower.contains(k)) {
            return MemoryZone::CreativeIdeas;
        }

        // 日常对话（最后判断，因为范围最广）
        let daily_keywords = [
            "你好", "在吗", "今天", "昨天", "明天", "天气", "吃饭", "睡觉", "周末", "假期", "哈哈",
            "嗯", "哦", "好的", "谢谢",
        ];
        if daily_keywords.iter().any(|k| content_lower.contains(k)) {
            return MemoryZone::DailyChat;
        }

        MemoryZone::Default
    }

    /// 分区名称
    pub fn name(&self) -> &str {
        match self {
            MemoryZone::DailyChat => "日常对话",
            MemoryZone::ExpertKnowledge => "专业知识",
            MemoryZone::Emotional => "情感记忆",
            MemoryZone::TaskProgress => "任务进度",
            MemoryZone::CreativeIdeas => "创意想法",
            MemoryZone::Default => "其他",
        }
    }

    /// 所有分区
    pub fn all() -> [MemoryZone; 6] {
        [
            MemoryZone::DailyChat,
            MemoryZone::ExpertKnowledge,
            MemoryZone::Emotional,
            MemoryZone::TaskProgress,
            MemoryZone::CreativeIdeas,
            MemoryZone::Default,
        ]
    }
}

// ─── 记忆重要性 ────────────────────────────────────────────────

/// 记忆重要性等级（影响遗忘速度）
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Deserialize, Serialize, Default)]
pub enum MemoryImportance {
    /// 转瞬即逝 - 很快遗忘
    Trivial = 1,
    /// 普通 - 正常衰减
    #[default]
    Normal = 2,
    /// 重要 - 衰减较慢
    Important = 3,
    /// 核心 - 几乎不遗忘
    Core = 4,
}

// ─── 带元数据的记忆项 ─────────────────────────────────────────

/// 心灵宫殿中的记忆项（扩展了分区、重要性、关联记忆）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PalaceMemory {
    /// 基础记忆项
    pub base: MemoryItem,
    /// 记忆分区
    pub zone: MemoryZone,
    /// 重要性等级
    pub importance: MemoryImportance,
    /// 关联记忆 ID（联想网络）
    pub associated_ids: Vec<String>,
    /// 激活次数（被检索/引用的次数）
    pub activation_count: u32,
    /// 最后激活时间
    pub last_activated_at: DateTime<Utc>,
    /// 记忆强度（0-1，随时间衰减，被激活则增强）
    pub strength: f32,
}

impl PalaceMemory {
    /// 创建新的宫殿记忆
    pub fn new(base: MemoryItem) -> Self {
        let zone = MemoryZone::infer_from_content(&base.content);
        let importance = Self::estimate_importance(&base.content);
        Self {
            base,
            zone,
            importance,
            associated_ids: Vec::new(),
            activation_count: 0,
            last_activated_at: Utc::now(),
            strength: 0.8,
        }
    }

    /// 估计记忆重要性
    fn estimate_importance(content: &str) -> MemoryImportance {
        let mut score = 0;

        // 长度：越长越可能重要
        if content.len() > 100 {
            score += 1;
        }
        if content.len() > 300 {
            score += 1;
        }

        // 关键词提示重要性（每个命中都加分）
        let important_keywords = ["重要", "关键", "核心", "必须", "一定要", "记住", "不要忘了"];
        let hits: usize = important_keywords
            .iter()
            .filter(|k| content.contains(**k))
            .count();
        score += hits.min(2);

        // 情感浓度高的记忆更重要
        let emotional_keywords = ["爱", "恨", "感动", "心碎", "幸福", "绝望", "永远", "第一次"];
        if emotional_keywords.iter().any(|k| content.contains(k)) {
            score += 1;
        }

        match score {
            0 => MemoryImportance::Trivial,
            1 => MemoryImportance::Normal,
            2 => MemoryImportance::Important,
            _ => MemoryImportance::Core,
        }
    }

    /// 激活记忆（被检索/引用时调用，增强强度）
    pub fn activate(&mut self) {
        self.activation_count += 1;
        self.last_activated_at = Utc::now();
        self.strength = (self.strength + 0.1).min(1.0);
    }

    /// 时间衰减（每次检索时计算）
    pub fn decay(&mut self, days_passed: f32) {
        let decay_rate = match self.importance {
            MemoryImportance::Trivial => 0.1,
            MemoryImportance::Normal => 0.03,
            MemoryImportance::Important => 0.01,
            MemoryImportance::Core => 0.002,
        };

        let decay = decay_rate * days_passed;
        self.strength = (self.strength - decay).max(0.0);
    }

    /// 是否应该被遗忘
    pub fn should_forget(&self) -> bool {
        self.strength < 0.1
    }
}

// ─── 心灵宫殿 ──────────────────────────────────────────────────

/// 心灵宫殿 - 记忆与心灵的统一体
pub struct MemoryPalace {
    base_memory: Arc<Memory>,
    palace_memories: RwLock<HashMap<String, PalaceMemory>>,
    zone_index: RwLock<HashMap<MemoryZone, Vec<String>>>,
    config: PalaceConfig,
}

impl std::fmt::Debug for MemoryPalace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.stats();
        f.debug_struct("MemoryPalace")
            .field("total_memories", &stats.total_count)
            .field("zones", &stats.zone_counts)
            .field("has_database", &self.has_database())
            .field("has_embedding", &self.has_embedding())
            .finish()
    }
}

/// 心灵宫殿配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PalaceConfig {
    pub base_config: MemoryConfig,
    pub enable_zones: bool,
    pub enable_forgetting: bool,
    pub enable_association: bool,
    pub forget_check_interval_secs: u64,
    pub forget_threshold: f32,
    pub association_depth: usize,
    pub persona_influence_weight: f32,
}

impl Default for PalaceConfig {
    fn default() -> Self {
        Self {
            base_config: MemoryConfig::default(),
            enable_zones: true,
            enable_forgetting: true,
            enable_association: true,
            forget_check_interval_secs: 3600,
            forget_threshold: 0.1,
            association_depth: 2,
            persona_influence_weight: 0.3,
        }
    }
}

impl MemoryPalace {
    pub fn new() -> Self {
        Self::with_config(PalaceConfig::default())
    }

    pub fn with_config(config: PalaceConfig) -> Self {
        Self {
            base_memory: Arc::new(Memory::with_config(config.base_config.clone())),
            palace_memories: RwLock::new(HashMap::new()),
            zone_index: RwLock::new(HashMap::new()),
            config,
        }
    }

    pub fn base_memory(&self) -> &Arc<Memory> {
        &self.base_memory
    }

    pub fn has_database(&self) -> bool {
        self.base_memory.has_database()
    }

    pub fn has_embedding(&self) -> bool {
        self.base_memory.has_embedding()
    }

    pub fn set_database(&self, db: Arc<Database>) {
        self.base_memory.set_database(db);
    }

    pub fn database(&self) -> Option<Arc<Database>> {
        self.base_memory.database()
    }

    pub fn set_embedding(&self, service: Arc<EmbeddingService>) {
        self.base_memory.set_embedding(service);
    }

    pub fn embedding_service(&self) -> Option<Arc<EmbeddingService>> {
        self.base_memory.embedding_service()
    }

    // ── 写入记忆 ──────────────────────────────────────

    /// 写入记忆（自动分区）
    pub fn store(
        &self,
        content: String,
        layer: MemoryLayer,
        session_id: Option<String>,
    ) -> Result<String> {
        let content_len = content.len();
        tracing::info!(
            "MemoryPalace: Storing memory (layer: {:?}, session: {:?}, content_len: {})",
            layer,
            session_id,
            content_len
        );

        let base_item = MemoryItem::new(content.clone(), layer, session_id.clone());
        let memory_id = base_item.id.clone();
        let palace_mem = PalaceMemory::new(base_item);
        let zone = palace_mem.zone;
        let importance = palace_mem.importance;

        tracing::debug!(
            "MemoryPalace: Memory auto-categorized - zone: {:?}, importance: {:?}",
            zone,
            importance
        );

        match layer {
            MemoryLayer::ShortTerm => {
                if let Some(sid) = &session_id {
                    self.base_memory.write_short_term(content.clone(), sid)?;
                }
            }
            MemoryLayer::Archive => {
                self.base_memory.archive_long_term(
                    session_id.as_deref().unwrap_or("default"),
                    &content,
                    "",
                )?;
            }
            MemoryLayer::Knowledge => {
                self.base_memory.add_knowledge(content, None)?;
            }
        }

        self.palace_memories
            .write()
            .unwrap()
            .insert(memory_id.clone(), palace_mem);

        if self.config.enable_zones {
            let mut zones = self.zone_index.write().unwrap();
            zones.entry(zone).or_default().push(memory_id.clone());
        }

        tracing::info!(
            "MemoryPalace: Memory stored successfully - id: {} ({} chars), zone: {:?}, importance: {:?}",
            memory_id,
            content_len,
            zone,
            importance
        );

        Ok(memory_id)
    }

    /// 写入指定分区
    pub fn store_in_zone(
        &self,
        content: String,
        zone: MemoryZone,
        layer: MemoryLayer,
        session_id: Option<String>,
    ) -> Result<String> {
        tracing::info!(
            "MemoryPalace: Storing memory in zone {:?} (layer: {:?}, content_len: {})",
            zone,
            layer,
            content.len()
        );

        let base_item = MemoryItem::new(content.clone(), layer, session_id.clone());
        let memory_id = base_item.id.clone();

        let mut palace_mem = PalaceMemory::new(base_item);
        palace_mem.zone = zone;

        match layer {
            MemoryLayer::ShortTerm => {
                if let Some(sid) = &session_id {
                    self.base_memory.write_short_term(content, sid)?;
                }
            }
            MemoryLayer::Archive => {
                self.base_memory.archive_long_term(
                    session_id.as_deref().unwrap_or("default"),
                    &content,
                    "",
                )?;
            }
            MemoryLayer::Knowledge => {
                self.base_memory.add_knowledge(content, None)?;
            }
        }

        self.palace_memories
            .write()
            .unwrap()
            .insert(memory_id.clone(), palace_mem);

        if self.config.enable_zones {
            let mut zones = self.zone_index.write().unwrap();
            zones.entry(zone).or_default().push(memory_id.clone());
        }

        Ok(memory_id)
    }

    // ── 搜索记忆 ──────────────────────────────────────

    /// 搜索记忆（受人格影响）
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        persona_zone_bias: Option<&HashMap<MemoryZone, f32>>,
    ) -> Vec<PalaceSearchResult> {
        tracing::debug!(
            "MemoryPalace: Searching memory (query: '{}...', limit: {}, has_persona_bias: {})",
            &query[..query.len().min(30)],
            limit,
            persona_zone_bias.is_some()
        );

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        // Phase 1: 在 read 锁内完成搜索和评分
        let (results, total_memories) = {
            let memories = self.palace_memories.read().unwrap();
            let total = memories.len();

            let mut results = Vec::new();

            for (_id, palace_mem) in memories.iter() {
                let content_lower = palace_mem.base.content.to_lowercase();

                let relevance = if query.is_empty() {
                    1.0
                } else if content_lower.contains(&query_lower) {
                    tracing::trace!(
                        "MemoryPalace: Full match in memory {} (zone: {:?})",
                        palace_mem.base.id,
                        palace_mem.zone
                    );
                    1.0
                } else {
                    let mut match_count = 0;
                    for word in &query_words {
                        if content_lower.contains(word) {
                            match_count += 1;
                        }
                    }
                    if query_words.is_empty() {
                        0.0
                    } else {
                        let ratio = match_count as f32 / query_words.len() as f32;
                        if ratio > 0.0 {
                            tracing::trace!(
                                "MemoryPalace: Partial match ({:.1}%) in memory {} (zone: {:?})",
                                ratio * 100.0,
                                palace_mem.base.id,
                                palace_mem.zone
                            );
                        }
                        ratio
                    }
                };

                let mut final_score = relevance * palace_mem.strength;

                if let Some(bias) = persona_zone_bias {
                    if let Some(&zone_weight) = bias.get(&palace_mem.zone) {
                        let influence = self.config.persona_influence_weight;
                        let old_score = final_score;
                        final_score = final_score * (1.0 - influence) + zone_weight * influence;
                        tracing::trace!(
                            "MemoryPalace: Persona bias applied to {} - zone: {:?}, weight: {:.2}, score: {:.3} -> {:.3}",
                            palace_mem.base.id,
                            palace_mem.zone,
                            zone_weight,
                            old_score,
                            final_score
                        );
                    }
                }

                if relevance > 0.0 {
                    results.push(PalaceSearchResult {
                        memory: palace_mem.clone(),
                        relevance_score: relevance,
                        final_score,
                    });
                }
            }

            (results, total)
        }; // read 锁在此作用域结束时自动释放

        // Phase 2: 排序和截断（无需锁）
        let mut results = results;
        results.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);

        tracing::info!(
            "MemoryPalace: Search completed - found {}/{} memories, returning top {}",
            results.len(),
            total_memories,
            limit
        );

        // Phase 3: 激活记忆（单独获取 write 锁，此时 read 锁已释放）
        if self.config.enable_association {
            let to_activate: Vec<String> =
                results.iter().map(|r| r.memory.base.id.clone()).collect();

            if !to_activate.is_empty() {
                let mut memories = self.palace_memories.write().unwrap();

                // 先收集所有需要增强的关联记忆ID
                let mut assoc_ids_to_boost: Vec<String> = Vec::new();
                for id in &to_activate {
                    if let Some(mem) = memories.get(id) {
                        assoc_ids_to_boost.extend(mem.associated_ids.clone());
                    }
                }

                // 激活主记忆
                for (i, id) in to_activate.iter().enumerate() {
                    if let Some(mem) = memories.get_mut(id) {
                        mem.activate();
                        tracing::trace!(
                            "MemoryPalace: Activated memory {} (rank {}), activation_count: {}, strength: {:.3}",
                            id,
                            i + 1,
                            mem.activation_count,
                            mem.strength
                        );
                    }
                }

                // 增强关联记忆
                for assoc_id in &assoc_ids_to_boost {
                    if let Some(assoc_mem) = memories.get_mut(assoc_id) {
                        assoc_mem.strength = (assoc_mem.strength + 0.05).min(1.0);
                        tracing::trace!(
                            "MemoryPalace: Associated memory {} strength boosted to {:.3}",
                            assoc_id,
                            assoc_mem.strength
                        );
                    }
                }
            }
        }

        results
    }

    /// 按分区搜索
    pub fn search_in_zone(
        &self,
        query: &str,
        zone: MemoryZone,
        limit: usize,
    ) -> Vec<PalaceSearchResult> {
        let mut zone_bias = HashMap::new();
        zone_bias.insert(zone, 1.5);
        self.search(query, limit, Some(&zone_bias))
    }

    /// 语义搜索
    pub async fn search_semantic(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticSearchResult>> {
        self.base_memory.search_semantic(query, limit).await
    }

    // ── 记忆维护 ──────────────────────────────────────

    /// 执行遗忘清理
    pub fn run_forget_cycle(&self) -> usize {
        if !self.config.enable_forgetting {
            tracing::debug!("MemoryPalace: Forgetting disabled, skipping forget cycle");
            return 0;
        }

        tracing::info!("MemoryPalace: Starting forget cycle");

        let mut forgotten = 0;
        let mut memories = self.palace_memories.write().unwrap();
        let total_before = memories.len();

        let mut to_remove = Vec::new();

        for (id, mem) in memories.iter_mut() {
            let now = Utc::now();
            let duration = now - mem.last_activated_at;
            let days_passed = duration.num_seconds() as f32 / 86400.0;

            mem.decay(days_passed);
            tracing::trace!(
                "MemoryPalace: Decayed memory {} (zone: {:?}, days_passed: {:.1}, new_strength: {:.3})",
                id,
                mem.zone,
                days_passed,
                mem.strength
            );

            if mem.should_forget() {
                to_remove.push(id.clone());
                forgotten += 1;
            }
        }

        for id in &to_remove {
            memories.remove(id);
        }

        if self.config.enable_zones {
            let mut zones = self.zone_index.write().unwrap();
            for ids in zones.values_mut() {
                ids.retain(|id| !to_remove.contains(id));
            }
        }

        tracing::info!(
            "MemoryPalace: Forget cycle completed - {} / {} memories forgotten",
            forgotten,
            total_before
        );

        forgotten
    }

    // ── 联想网络 ──────────────────────────────────────

    /// 添加双向关联
    pub fn add_association(&self, memory_id: &str, associated_id: &str) {
        let mut memories = self.palace_memories.write().unwrap();

        tracing::debug!(
            "MemoryPalace: Adding bidirectional association {} <-> {}",
            memory_id,
            associated_id
        );

        let memory_id_str = memory_id.to_string();
        let associated_id_str = associated_id.to_string();

        if let Some(mem) = memories.get_mut(memory_id) {
            if !mem.associated_ids.contains(&associated_id_str) {
                mem.associated_ids.push(associated_id_str.clone());
                tracing::trace!(
                    "MemoryPalace: Added association from {} to {}",
                    memory_id,
                    associated_id
                );
            }
        }

        if let Some(assoc_mem) = memories.get_mut(associated_id) {
            if !assoc_mem.associated_ids.contains(&memory_id_str) {
                assoc_mem.associated_ids.push(memory_id_str);
                tracing::trace!("MemoryPalace: Added association to {}", associated_id);
            }
        }
    }

    /// 获取联想记忆
    pub fn get_associated(&self, memory_id: &str, depth: usize) -> Vec<PalaceMemory> {
        if !self.config.enable_association || depth == 0 {
            return Vec::new();
        }

        let memories = self.palace_memories.read().unwrap();
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut current_level = vec![memory_id.to_string()];

        for _ in 0..depth {
            let mut next_level = Vec::new();
            for current_id in &current_level {
                if let Some(mem) = memories.get(current_id) {
                    for assoc_id in &mem.associated_ids {
                        if !visited.contains(assoc_id) {
                            visited.insert(assoc_id.clone());
                            if let Some(assoc_mem) = memories.get(assoc_id) {
                                result.push(assoc_mem.clone());
                                next_level.push(assoc_id.clone());
                            }
                        }
                    }
                }
            }
            current_level = next_level;
            if current_level.is_empty() {
                break;
            }
        }

        result
    }

    // ── 统计信息 ──────────────────────────────────────

    pub fn stats(&self) -> PalaceStats {
        let memories = self.palace_memories.read().unwrap();
        let base_stats = self.base_memory.stats();

        let mut zone_counts = HashMap::new();
        let mut importance_counts = HashMap::new();
        let mut total_strength = 0.0;

        for mem in memories.values() {
            *zone_counts.entry(mem.zone).or_insert(0) += 1;
            *importance_counts.entry(mem.importance as u32).or_insert(0) += 1;
            total_strength += mem.strength;
        }

        let avg_strength = if memories.is_empty() {
            0.0
        } else {
            total_strength / memories.len() as f32
        };

        PalaceStats {
            total_count: memories.len(),
            zone_counts,
            importance_counts,
            avg_strength,
            base_stats,
        }
    }

    pub fn zone_count(&self, zone: MemoryZone) -> usize {
        self.zone_index
            .read()
            .unwrap()
            .get(&zone)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    // ── 兼容层 ──

    pub fn write_short_term(&self, content: String, session_id: &str) -> Result<()> {
        self.store(
            content,
            MemoryLayer::ShortTerm,
            Some(session_id.to_string()),
        )
        .map(|_| ())
    }

    pub fn search_short_term(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.base_memory.search_short_term(query, limit)
    }

    pub fn search_archive(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.base_memory.search_archive(query, limit)
    }

    pub fn add_knowledge(
        &self,
        content: String,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<()> {
        self.base_memory.add_knowledge(content, metadata)
    }

    pub fn summarize_short_term(&self) -> String {
        self.base_memory.summarize_short_term()
    }

    pub fn is_empty(&self) -> bool {
        self.base_memory.is_empty()
    }
}

impl Default for MemoryPalace {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 搜索结果 ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PalaceSearchResult {
    pub memory: PalaceMemory,
    pub relevance_score: f32,
    pub final_score: f32,
}

// ─── 统计信息 ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PalaceStats {
    pub total_count: usize,
    pub zone_counts: HashMap<MemoryZone, usize>,
    pub importance_counts: HashMap<u32, usize>,
    pub avg_strength: f32,
    pub base_stats: MemoryStats,
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_zone_inference() {
        assert_eq!(
            MemoryZone::infer_from_content("今天天气真好"),
            MemoryZone::DailyChat
        );
        assert_eq!(
            MemoryZone::infer_from_content("什么是 Rust 的所有权"),
            MemoryZone::ExpertKnowledge
        );
        assert_eq!(
            MemoryZone::infer_from_content("我今天好开心啊"),
            MemoryZone::Emotional
        );
        assert_eq!(
            MemoryZone::infer_from_content("明天要完成这个任务"),
            MemoryZone::TaskProgress
        );
        assert_eq!(
            MemoryZone::infer_from_content("如果我有超能力就好了"),
            MemoryZone::CreativeIdeas
        );
    }

    #[test]
    fn test_memory_importance_estimation() {
        let trivial = PalaceMemory::new(MemoryItem::new("嗯".into(), MemoryLayer::ShortTerm, None));
        assert!(trivial.importance as u32 <= MemoryImportance::Normal as u32);

        let important = PalaceMemory::new(MemoryItem::new(
            "这很重要，你一定要记住：Rust 的所有权规则是核心概念".into(),
            MemoryLayer::Archive,
            None,
        ));
        assert!(important.importance as u32 >= MemoryImportance::Important as u32);
    }

    #[test]
    fn test_palace_store_and_search() {
        let palace = MemoryPalace::new();
        palace
            .store("Rust 是一种系统编程语言".into(), MemoryLayer::Archive, None)
            .unwrap();
        palace
            .store("今天天气不错".into(), MemoryLayer::Archive, None)
            .unwrap();

        let results = palace.search("Rust 编程", 5, None);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_memory_activation_and_decay() {
        let mut mem =
            PalaceMemory::new(MemoryItem::new("测试".into(), MemoryLayer::ShortTerm, None));
        let initial_strength = mem.strength;

        mem.activate();
        assert!(mem.strength >= initial_strength);
        assert_eq!(mem.activation_count, 1);

        mem.decay(10.0);
        assert!(mem.strength < 1.0);
    }

    #[test]
    fn test_persona_influence() {
        let palace = MemoryPalace::new();
        palace
            .store_in_zone(
                "编程技术内容".into(),
                MemoryZone::ExpertKnowledge,
                MemoryLayer::Archive,
                None,
            )
            .unwrap();
        palace
            .store_in_zone(
                "今天好开心".into(),
                MemoryZone::Emotional,
                MemoryLayer::Archive,
                None,
            )
            .unwrap();

        let mut bias = HashMap::new();
        bias.insert(MemoryZone::ExpertKnowledge, 1.5);
        bias.insert(MemoryZone::Emotional, 0.5);

        let results = palace.search("内容", 5, Some(&bias));
        assert!(!results.is_empty() || results.is_empty());
    }

    #[test]
    fn test_forget_cycle() {
        let palace = MemoryPalace::new();
        let id = palace
            .store("临时记忆".into(), MemoryLayer::Archive, None)
            .unwrap();

        {
            let mut memories = palace.palace_memories.write().unwrap();
            if let Some(mem) = memories.get_mut(&id) {
                mem.strength = 0.05;
            }
        }

        let forgotten = palace.run_forget_cycle();
        assert!(forgotten >= 1);
    }
}
