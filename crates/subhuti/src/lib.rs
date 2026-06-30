//! # Subhuti - Rust 极简轻量 AI Agent 框架
//!
//! 设计哲学：薄封装、无魔法、无全局状态、可完全掌控
//!
//! ## 四层架构
//!
//! - **Memory Layer**: 记忆存储与检索 (短期/长期/知识库)
//! - **Runtime Layer**: LLM抽象、工具系统、约束护栏
//! - **Flow Layer**: ReAct 智能闭环 (Plan→Act→Observe→Reflect)
//! - **Extension Layer**: Hook/中间件扩展
//!
//! ## 快速开始
//!
//! ```rust,ignore
//! use subhuti::{Subhuti, Session, memory::MemoryConfig};
//!
//! let subhuti = Subhuti::new();
//! let session = Session::new("user_001");
//! let response = subhuti.run(session, "你好，帮我查一下天气").await;
//! ```

// 四层架构模块
pub mod context; // 统一上下文层
pub mod debug;
pub mod expert; // 专家插件系统 - 领域专家扩展
pub mod flow;
pub mod memory; // 记忆层 - 包含存储实现（数据库作为内部基础设施）
pub mod observe; // 可观测性系统 - Trace 追踪
pub mod orchestrator; // 多Agent协调编排层
pub mod runtime; // 包含 llm 和 tools 子模块
pub mod skill; // Skill 层 - 类似 HTTP 路由的技能系统
pub mod soul; // 心灵层 - 动态角色养成系统

// Re-exports - 从 runtime 导出 LLM 和 Tools，从 memory 导出数据库类型
pub use context::{RunContext, TokenStats};
pub use debug::{
    assert_with_context, debug_print, diagnose_value, measure_time, HealthReport, HealthStatus,
    LockDetector, Profiler, TestTracker,
};
pub use expert::{ExpertInfo, ExpertPersona, ExpertPlugin, KnowledgeEntry};
pub use flow::{Flow, FlowConfig, FlowManager, FlowStep, FlowType, ReactFlow};
pub use memory::{
    Database, DatabaseStore, DbConfig, FeedbackRow, HistoryRow, Memory, MemoryConfig, MemoryRow,
    MemoryStore, PersonaData, PersonaRow, SemanticSearchResult,
};
pub use observe::session::SessionRecordParams;
pub use orchestrator::{
    AgentMeta, CollaborationResult, ContextData, ContextStore, CtxId, DispatchStrategy, Expert,
    ExpertAgent, ExpertMatchResult, ExpertPerformance, OrchestrationResult, Orchestrator,
    OrchestratorConfig, RuleEngine, Step, TaskProfile,
};
pub use runtime::{
    LLMClient, LLMConfig, LLMProvider, LLMResponse, Message, MockLLM, Role, Runtime, RuntimeConfig,
    Session, SessionConfig, Tool, ToolInfo, ToolResult, LLM,
};
pub use skill::{
    CalculatorSkill, CodeExecutionSkill, DefaultChatSkill, FileOperationSkill, FlowTemplate,
    ReminderSkill, SearchLongMemorySkill, Skill, SkillContext, SkillInfo, SkillManager,
    WeatherSkill, WebSearchSkill,
};
pub use soul::{
    BigFive, EmotionalTendency, FeedbackType, InteractionStats, MemoryImportance, MemoryPalace,
    MemoryZone, PalaceConfig, PalaceMemory, PalaceSearchResult, PalaceStats, PersonaProfile,
    SoulConfig, SoulLayer, ToneStyle, UserFeedback,
};

// Core error type
pub use anyhow::Result;
pub use thiserror::Error;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Subhuti 全局配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubhutiConfig {
    /// LLM 配置
    pub llm: LLMConfig,
    /// LLM 提供者
    pub provider: LLMProvider,
    /// Runtime 配置
    pub runtime: RuntimeConfig,
    /// Memory 配置
    pub memory: MemoryConfig,
    /// Flow 配置
    pub flow: FlowConfig,
    /// 数据库配置（可选）
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

/// Subhuti 框架主入口
#[derive(Debug)]
pub struct Subhuti {
    /// 全局配置
    config: SubhutiConfig,
    /// 记忆系统（Arc 共享）- 保留向后兼容
    memory: Arc<Memory>,
    /// 心灵宫殿（记忆与心灵的统一体）- 新的统一入口
    memory_palace: Arc<MemoryPalace>,
    /// 运行时（Arc 共享）
    runtime: Arc<Runtime>,
    /// 流程管理器
    flow: FlowManager,
    /// Skill 管理器（内部可变性，支持动态注册）
    skills: Mutex<SkillManager>,
    /// 心灵层（动态角色养成，内部可变性）
    soul: Mutex<SoulLayer>,
    /// 数据库连接（可选，Arc 共享）
    db: Option<Arc<Database>>,
    /// 专家插件管理（使用增强版 PluginManager）
    experts: Mutex<expert::PluginManager>,
    /// 多Agent协调编排器（内部可变性，支持动态注册专家）
    /// 使用 tokio::sync::Mutex 以支持在 async 上下文中持有锁
    orchestrator: tokio::sync::Mutex<Orchestrator>,
}

impl Subhuti {
    /// 创建新的 Subhuti 实例（使用默认配置）
    pub fn new() -> Self {
        Self::with_config(SubhutiConfig::default())
    }

    /// 使用配置创建
    pub fn with_config(config: SubhutiConfig) -> Self {
        let runtime = Arc::new(Runtime::with_config_and_llm(
            config.runtime.clone(),
            &config.llm,
            config.provider,
        ));
        let flow_config = config.flow.clone();
        let memory_config = config.memory.clone();

        // 创建心灵宫殿（记忆与心灵的统一体）
        let palace_config = PalaceConfig {
            base_config: memory_config.clone(),
            ..Default::default()
        };
        let memory_palace = Arc::new(MemoryPalace::with_config(palace_config));
        let memory = memory_palace.base_memory().clone();

        // 创建心灵层并关联心灵宫殿
        let mut soul = SoulLayer::new("data/persona.json");
        soul.set_memory_palace(memory_palace.clone());

        let mut skill_manager = SkillManager::new();
        // 默认注册基础技能
        skill_manager.register(WeatherSkill);
        skill_manager.register(CalculatorSkill);
        skill_manager.register(SearchLongMemorySkill);
        skill_manager.register(DefaultChatSkill);
        skill_manager.register(FileOperationSkill);
        skill_manager.register(WebSearchSkill);
        skill_manager.register(CodeExecutionSkill);
        skill_manager.register(ReminderSkill);

        // 创建共享 Skill 管理器（tokio RwLock 用于编排器）
        let _shared_skills = Arc::new(tokio::sync::RwLock::new(SkillManager::new()));

        // 创建多Agent协调编排器并注册内置专家
        let mut orchestrator = Orchestrator::with_defaults();

        let coding_expert = orchestrator::DefaultExpert::new(
            "coding".to_string(),
            "编程专家".to_string(),
            vec![
                "编程".into(),
                "代码".into(),
                "rust".into(),
                "开发".into(),
                "bug".into(),
            ],
            10,
            "资深 Rust 开发专家".to_string(),
            "10年 Rust 开发经验，精通系统编程".to_string(),
            "帮助用户解决编程问题".to_string(),
            runtime.clone(),
        );
        orchestrator.register_agent(Arc::new(coding_expert));

        let weather_expert = orchestrator::DefaultExpert::new(
            "weather".to_string(),
            "天气专家".to_string(),
            vec!["天气".into(), "气象".into(), "温度".into(), "下雨".into()],
            5,
            "气象预报专家".to_string(),
            "专业气象分析师".to_string(),
            "提供准确的天气预报".to_string(),
            runtime.clone(),
        );
        orchestrator.register_agent(Arc::new(weather_expert));

        Self {
            config,
            memory,
            memory_palace,
            runtime,
            flow: FlowManager::with_config(flow_config),
            skills: Mutex::new(skill_manager),
            soul: Mutex::new(soul),
            db: None,
            experts: Mutex::new(expert::PluginManager::new()),
            orchestrator: tokio::sync::Mutex::new(orchestrator),
        }
    }

    /// 异步初始化数据库连接
    pub async fn init_database(&mut self, config: &DbConfig) -> Result<()> {
        let db = Database::new(config).await?;
        db.init_tables().await?;
        let db_arc = Arc::new(db);
        self.db = Some(db_arc.clone());

        // 将数据库连接传递给心灵层
        if let Ok(mut soul) = self.soul.lock() {
            soul.set_database(db_arc.clone());
        }

        // 将数据库连接传递给记忆系统（双写策略）
        self.memory.set_database(db_arc.clone());

        // 心灵宫殿也设置数据库
        self.memory_palace.set_database(db_arc.clone());

        // 初始化 embedding 服务（尝试连接 Ollama 的 bge-m3）
        let emb_config = crate::memory::EmbeddingConfig {
            api_url: std::env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            model: std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "bge-m3:latest".to_string()),
            dimensions: 1024,
        };
        let emb_service = Arc::new(crate::memory::EmbeddingService::new(emb_config));
        self.memory.set_embedding(emb_service.clone());
        self.memory_palace.set_embedding(emb_service);

        tracing::info!("Subhuti: Database initialized and connected to SoulLayer & MemoryPalace");
        Ok(())
    }

    /// 设置数据库连接（从外部传入）
    pub fn set_database(&mut self, db: Arc<Database>) {
        self.db = Some(db.clone());

        // 将数据库连接传递给心灵层
        if let Ok(mut soul) = self.soul.lock() {
            soul.set_database(db.clone());
        }

        // 将数据库连接传递给记忆系统
        self.memory.set_database(db.clone());

        // 心灵宫殿也设置数据库
        self.memory_palace.set_database(db);
    }

    /// 获取数据库连接
    pub fn database(&self) -> Option<Arc<Database>> {
        self.db.clone()
    }

    /// 获取配置
    pub fn config(&self) -> &SubhutiConfig {
        &self.config
    }

    /// 注册 Skill
    pub fn register_skill<S: Skill>(&self, skill: S) {
        self.skills.lock().unwrap().register(skill);
    }

    /// 设置 Skill 匹配阈值
    pub fn set_skill_threshold(&self, threshold: f32) {
        self.skills.lock().unwrap().set_match_threshold(threshold);
    }

    // ── 专家插件管理 ──────────────────────────────────────

    /// 安装插件
    pub fn install_plugin<E: ExpertPlugin + 'static>(&self, plugin: E) -> Result<(), String> {
        self.experts.lock().unwrap().install(plugin)
    }

    /// 卸载插件
    pub fn uninstall_plugin(&self, id: &str) -> Result<(), String> {
        self.experts.lock().unwrap().uninstall(id)
    }

    /// 启用插件
    pub fn enable_plugin(&self, id: &str) -> Result<(), String> {
        self.experts.lock().unwrap().enable(id)
    }

    /// 停用插件
    pub fn disable_plugin(&self, id: &str) -> Result<(), String> {
        self.experts.lock().unwrap().disable(id)
    }

    /// 注册并启用专家插件（便捷方法）
    pub fn register_expert<E: ExpertPlugin + 'static>(&self, expert: E) {
        let info = expert.info();
        let id = info.id.clone();

        // 安装插件
        if let Err(e) = self.experts.lock().unwrap().install(expert) {
            tracing::warn!("Failed to install plugin {}: {}", id, e);
            return;
        }

        // 自动启用
        if let Err(e) = self.experts.lock().unwrap().enable(&id) {
            tracing::warn!("Failed to enable plugin {}: {}", id, e);
            return;
        }

        tracing::info!("Subhuti: Registered expert: {}", info.name);
    }

    /// 同步专家到编排器（异步方法）
    pub async fn sync_experts_to_orchestrator(&self) {
        let plugins = self.experts.lock().unwrap().list_plugins();

        let mut orch = self.orchestrator.lock().await;

        for plugin_meta in plugins {
            if let Some(plugin) = self
                .experts
                .lock()
                .unwrap()
                .get_plugin(&plugin_meta.manifest.id)
            {
                let agent = Arc::new(ExpertPluginAdapter::new(plugin, self.runtime.clone()));
                orch.register_agent(agent);
            }
        }

        tracing::info!(
            "Subhuti: Synced {} experts to orchestrator",
            orch.list_agents().len()
        );
    }

    /// 激活指定专家
    pub fn activate_expert(&self, expert_id: &str) -> Result<()> {
        let plugin = self
            .experts
            .lock()
            .unwrap()
            .activate(expert_id)
            .map_err(|e| anyhow::anyhow!(e))?;

        // 1. 更新心灵层 persona
        if let Ok(mut soul) = self.soul.lock() {
            let persona = plugin.persona();
            soul.set_persona_from_expert(persona);
        }

        // 2. 注册专家的 skills
        {
            let mut skills = self.skills.lock().unwrap();
            for skill in plugin.skills() {
                skills.register_boxed(skill);
            }
        }

        // 3. 加载知识库到记忆系统和心灵宫殿
        for entry in plugin.knowledge() {
            let _ = self
                .memory
                .add_knowledge(entry.content.clone(), entry.metadata.clone());
            let _ = self.memory_palace.store_in_zone(
                entry.content,
                MemoryZone::ExpertKnowledge,
                crate::memory::MemoryLayer::Knowledge,
                None,
            );
        }

        tracing::info!("Subhuti: Activated expert: {}", expert_id);
        Ok(())
    }

    /// 停用当前专家，恢复默认状态
    pub fn deactivate_expert(&self) -> Result<()> {
        // 停用当前专家
        self.experts
            .lock()
            .unwrap()
            .deactivate()
            .map_err(|e| anyhow::anyhow!(e))?;
        tracing::info!("Subhuti: Deactivated expert");
        Ok(())
    }

    /// 获取当前激活的专家 ID
    pub fn active_expert_id(&self) -> Option<String> {
        self.experts.lock().unwrap().get_active_expert_id()
    }

    /// 获取专家插件数量
    pub fn expert_plugin_count(&self) -> usize {
        self.experts.lock().unwrap().list_plugins().len()
    }

    /// 获取当前激活的专家信息
    pub fn active_expert_info(&self) -> Option<ExpertInfo> {
        self.experts
            .lock()
            .unwrap()
            .get_active_expert()
            .map(|p| p.info())
    }

    /// 列出所有已注册的插件
    pub fn list_experts(&self) -> Vec<ExpertInfo> {
        self.experts
            .lock()
            .unwrap()
            .list_plugins()
            .into_iter()
            .map(|m| ExpertInfo {
                id: m.manifest.id,
                name: m.manifest.name,
                description: m.manifest.description,
                version: m.manifest.version,
                author: m.manifest.author.map(|a| a.name),
                category: m.manifest.category.to_string(),
                keywords: m.manifest.keywords,
            })
            .collect()
    }

    /// 列出所有插件的详细信息（包括状态）
    pub fn list_plugins(&self) -> Vec<expert::PluginMetadata> {
        self.experts.lock().unwrap().list_plugins()
    }

    /// 自动匹配专家（根据输入内容）
    pub fn match_expert(&self, input: &str) -> Option<ExpertInfo> {
        self.experts
            .lock()
            .unwrap()
            .match_expert(input)
            .map(|p| p.info())
    }

    // ── 多Agent协调编排 ──────────────────────────────────

    /// 注册专家到编排器
    pub async fn register_orchestrator_expert(&self, agent: Arc<dyn ExpertAgent>) {
        self.orchestrator.lock().await.register_agent(agent);
    }

    /// 从插件注册专家到编排器
    pub async fn register_orchestrator_expert_from_plugin(&self, plugin: Arc<dyn ExpertPlugin>) {
        let agent = Arc::new(ExpertPluginAdapter::new(plugin, self.runtime.clone()));
        self.orchestrator.lock().await.register_agent(agent);
    }

    /// 设置编排器通用专家
    pub async fn set_orchestrator_general_expert(&self, expert_id: &str) {
        self.orchestrator.lock().await.set_general_expert(expert_id);
    }

    /// 使用编排器分析任务
    pub async fn analyze_task(&self, input: &str) -> TaskProfile {
        let mut orch = self.orchestrator.lock().await;
        match orch.schedule(input) {
            Ok((_, _, profile)) => profile,
            Err(_) => TaskProfile {
                input: input.to_string(),
                ..Default::default()
            },
        }
    }

    /// 使用编排器执行任务（自动选择策略）
    pub async fn run_orchestrated(
        &self,
        input: &str,
        user_id: &str,
    ) -> Result<OrchestrationResult> {
        let mut orch = self.orchestrator.lock().await;
        orch.run_orchestrated(input, user_id).await
    }

    /// 使用编排器执行任务（指定策略）
    pub async fn run_orchestrated_with_strategy(
        &self,
        input: &str,
        user_id: &str,
        _strategy: DispatchStrategy,
    ) -> Result<OrchestrationResult> {
        let mut orch = self.orchestrator.lock().await;
        orch.run_orchestrated(input, user_id).await
    }

    /// 获取编排器专家列表
    pub async fn list_orchestrator_experts(&self) -> Vec<ExpertInfo> {
        Vec::new()
    }

    /// 获取编排器专家数量
    pub async fn orchestrator_expert_count(&self) -> usize {
        self.orchestrator.lock().await.list_agents().len()
    }

    /// 匹配编排器专家
    pub async fn match_orchestrator_experts(
        &self,
        _input: &str,
    ) -> Vec<orchestrator::ExpertMatchResult> {
        Vec::new()
    }

    /// 执行钩子链
    pub fn execute_hook(
        &self,
        point: expert::HookPoint,
        ctx: expert::HookContext,
    ) -> expert::HookResult {
        self.experts.lock().unwrap().execute_hook(point, ctx)
    }

    /// 获取心灵层（只读，返回当前性格快照克隆）
    pub fn soul_profile(&self) -> PersonaProfile {
        self.soul.lock().unwrap().profile().clone()
    }

    /// 获取距离下次演化还需多少次互动
    pub fn interactions_since_last_evolve(&self) -> u32 {
        self.soul
            .lock()
            .map(|s| s.interactions_since_last_evolve())
            .unwrap_or(0)
    }

    /// 记录互动（统计分析轨道）
    pub fn record_interaction(&self, skill_name: &str, user_input: &str, response_time_ms: u64) {
        if let Ok(mut soul) = self.soul.lock() {
            soul.record_interaction(skill_name, user_input, response_time_ms);
        }
    }

    /// 记录用户反馈（点赞/踩/评论）
    pub fn record_feedback(&self, feedback_type: FeedbackType, content: &str, skill_name: &str) {
        if let Ok(mut soul) = self.soul.lock() {
            soul.record_feedback(feedback_type, content, skill_name);
        }
    }

    /// 获取反馈统计（点赞/踩数量）
    pub fn feedback_stats(&self) -> (u32, u32) {
        self.soul
            .lock()
            .map(|s| s.feedback_stats())
            .unwrap_or((0, 0))
    }

    /// 获取指定用户的性格快照
    pub fn user_profile(&self, user_id: &str) -> PersonaProfile {
        self.soul
            .lock()
            .map(|s| s.get_user_profile(user_id).clone())
            .unwrap_or_default()
    }

    /// 获取所有用户 ID 列表
    pub fn list_users(&self) -> Vec<String> {
        self.soul.lock().map(|s| s.list_users()).unwrap_or_default()
    }

    /// 触发演化（LLM 自反思轨道 + 统计分析双轨融合）
    pub async fn evolve_persona(&self) -> Result<()> {
        // 先获取当前 profile（释放锁后再 await）
        let profile_snapshot = {
            let soul = self.soul.lock().unwrap();
            soul.profile().clone()
        };

        // 准备演化素材（不持有锁）
        let short_term_summary = self.memory.summarize_short_term();
        let recent_archive = self.memory.search_archive("", 15);
        let skill_usage = profile_snapshot.interaction_stats.skill_usage.clone();
        let skill_proficiency = profile_snapshot.skill_proficiency.clone();

        // 构建分析文本
        let mut analysis_text = String::new();
        if !short_term_summary.is_empty() {
            analysis_text.push_str(&format!("【近期对话摘要】\n{}\n\n", short_term_summary));
        }
        if !recent_archive.is_empty() {
            analysis_text.push_str("【近期对话记录】\n");
            for (i, result) in recent_archive.iter().enumerate() {
                analysis_text.push_str(&format!("\n对话 {}:\n{}\n", i + 1, result.item.content));
            }
        }

        let stat_profile = serde_json::to_string_pretty(&profile_snapshot)?;
        analysis_text.push_str(&format!("\n【当前统计性格】\n{}", stat_profile));

        let skill_stats: Vec<String> = skill_usage
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}: {}次 (熟练度: {:.0}%)",
                    k,
                    v,
                    skill_proficiency.get(k).copied().unwrap_or(0.0) * 100.0
                )
            })
            .collect();
        analysis_text.push_str(&format!("\n【技能使用统计】\n{}", skill_stats.join("\n")));

        // 构建提示词
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

        // 调用 LLM（不持有锁）
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
        let response = self.runtime.call_llm_with_stats(messages).await?;
        let response_text = response.content.trim().to_string();

        tracing::info!(
            "SoulLayer: LLM evolution response (first 200): {}",
            &response_text[..response_text.len().min(200)]
        );

        // 解析 LLM 响应
        let json_start = response_text.find('{').unwrap_or(0);
        let json_end = response_text.rfind('}').unwrap_or(response_text.len()) + 1;
        let json_str = &response_text[json_start..json_end];

        let llm_suggestion: crate::soul::EvolutionSuggestion = match serde_json::from_str(json_str)
        {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("SoulLayer: Failed to parse LLM evolution JSON: {}", e);
                // LLM 解析失败，只递增版本号
                let mut soul = self.soul.lock().unwrap();
                soul.increment_version("统计分析更新（LLM 解析失败）".to_string());
                return Ok(());
            }
        };

        // 应用演化（重新获取锁）
        let mut soul = self.soul.lock().unwrap();
        soul.apply_evolution(llm_suggestion);

        Ok(())
    }

    /// 获取运行时引用
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    /// 注入 Mock LLM（测试专用，无需真实 API）
    ///
    /// ```rust,ignore
    /// let subhuti = Subhuti::new();
    /// let mock = MockLLM::with_response("这是模拟响应");
    /// subhuti.set_mock_llm(mock);
    /// ```
    pub fn set_mock_llm(&self, mock: MockLLM) {
        self.runtime.set_mock_llm(mock);
    }

    /// 获取记忆系统引用
    pub fn memory(&self) -> &Arc<Memory> {
        &self.memory
    }

    /// 获取心灵宫殿引用（记忆与心灵的统一体）
    pub fn memory_palace(&self) -> &Arc<MemoryPalace> {
        &self.memory_palace
    }

    /// 获取心灵宫殿统计信息
    pub fn palace_stats(&self) -> PalaceStats {
        self.memory_palace.stats()
    }

    /// 执行记忆遗忘周期
    pub fn run_forget_cycle(&self) -> usize {
        self.memory_palace.run_forget_cycle()
    }

    /// 获取人格影响的记忆分区偏好
    pub fn persona_zone_bias(&self) -> std::collections::HashMap<MemoryZone, f32> {
        if let Ok(soul) = self.soul.lock() {
            soul.get_persona_zone_bias()
        } else {
            std::collections::HashMap::new()
        }
    }

    /// 获取人格版本
    pub fn persona_version(&self) -> u32 {
        if let Ok(soul) = self.soul.lock() {
            soul.profile().version
        } else {
            0
        }
    }

    /// 获取总互动次数
    pub fn total_interactions(&self) -> u32 {
        if let Ok(soul) = self.soul.lock() {
            soul.profile().interaction_stats.total_interactions
        } else {
            0
        }
    }

    /// 健康检查 - 检查系统各组件状态
    pub fn health_check(&self) -> HealthReport {
        let mut report = HealthReport::new();

        // 检查记忆宫殿
        let palace = &self.memory_palace;
        let stats = palace.stats();
        report.add_component(
            HealthStatus::healthy("MemoryPalace")
                .with_detail("total_memories", stats.total_count)
                .with_detail("short_term", stats.base_stats.short_term_count)
                .with_detail("archive", stats.base_stats.archive_count)
                .with_detail("knowledge", stats.base_stats.knowledge_count),
        );

        // 检查数据库连接（可选组件）
        if let Some(_db) = &self.db {
            report.add_component(
                HealthStatus::healthy("Database")
                    .optional_(true)
                    .with_detail("connected", true),
            );
        } else {
            report.add_component(
                HealthStatus::optional("Database", false)
                    .with_detail("reason", "Not configured (optional component)"),
            );
        }

        // 检查心灵层
        if let Ok(soul) = self.soul.lock() {
            let profile = soul.profile();
            report.add_component(
                HealthStatus::healthy("SoulLayer")
                    .with_detail("persona_version", profile.version)
                    .with_detail("persona_name", &profile.name)
                    .with_detail(
                        "total_interactions",
                        profile.interaction_stats.total_interactions,
                    )
                    .with_detail(
                        "interactions_since_evolve",
                        soul.interactions_since_last_evolve(),
                    ),
            );
        } else {
            report.add_component(HealthStatus::unhealthy(
                "SoulLayer",
                "Failed to acquire lock",
            ));
        }

        // 检查专家插件
        if let Ok(experts) = self.experts.lock() {
            let plugin_count = experts.list_plugins().len();
            let active_id = experts.get_active_expert_id();
            report.add_component(
                HealthStatus::healthy("ExpertPlugins")
                    .with_detail("plugin_count", plugin_count)
                    .with_detail(
                        "active_expert",
                        active_id.unwrap_or_else(|| "none".to_string()),
                    ),
            );
        } else {
            report.add_component(HealthStatus::unhealthy(
                "ExpertPlugins",
                "Failed to acquire lock",
            ));
        }

        // 检查 Skills
        if let Ok(skills) = self.skills.lock() {
            let skill_count = skills.get_skills().len();
            report.add_component(
                HealthStatus::healthy("Skills").with_detail("skill_count", skill_count),
            );
        } else {
            report.add_component(HealthStatus::unhealthy("Skills", "Failed to acquire lock"));
        }

        report
    }

    /// 打印健康报告到控制台
    pub fn print_health_report(&self) {
        let report = self.health_check();
        report.print();
    }

    /// 运行 Agent（带 Skill 路由）
    ///
    /// 执行流程：
    /// 1. Extension: before_prompt
    /// 2. Skill 路由匹配
    ///    - 匹配成功：执行 Skill 的纯代码实现
    ///    - 匹配失败：使用默认 Flow（让 AI 自主决策）
    /// 3. Extension: after_complete
    ///
    /// 滑动窗口机制：
    /// - 短期记忆限制在配置容量内
    /// - 超额消息自动归档到长期记忆
    pub async fn run(
        &self,
        session: &mut Session,
        input: &str,
    ) -> Result<(String, Option<String>, TokenStats)> {
        let mut run_ctx = self.create_run_context(std::mem::replace(session, Session::new("temp")));
        let result = self
            .run_with_run_context(&mut run_ctx, input, None, None)
            .await;
        *session = run_ctx.session;
        result
    }

    /// 运行 Agent（显式指定 Skill）
    ///
    /// 如果指定了 skill_name，直接使用该 Skill
    /// 如果没指定，使用智能匹配或默认 Flow
    pub async fn run_with_skill(
        &self,
        session: &mut Session,
        input: &str,
        skill_name: Option<&str>,
    ) -> Result<(String, Option<String>, TokenStats)> {
        let mut run_ctx = self.create_run_context(std::mem::replace(session, Session::new("temp")));
        let result = self
            .run_with_run_context(&mut run_ctx, input, skill_name, None)
            .await;
        *session = run_ctx.session;
        result
    }

    /// 运行 Agent（统一 flow_template 参数）
    ///
    /// 自动判断 flow_template 用于 Skill 还是框架流程：
    /// - 当有 Skill 匹配时：作为 Skill 的流程模板
    /// - 当没有 Skill 匹配时：转换为 FlowType 作为框架流程
    pub async fn run_with_template(
        &self,
        session: &mut Session,
        input: &str,
        skill_name: Option<&str>,
        flow_template: Option<FlowTemplate>,
    ) -> Result<(String, Option<String>, TokenStats)> {
        let mut run_ctx = self.create_run_context(std::mem::replace(session, Session::new("temp")));
        let result = self
            .run_with_run_context(&mut run_ctx, input, skill_name, flow_template)
            .await;
        *session = run_ctx.session;
        result
    }

    /// 简单运行 Agent（HTTP 友好版本）
    ///
    /// 内部创建 Session，适合单次请求场景
    pub async fn run_simple(
        &self,
        user_id: &str,
        input: &str,
    ) -> Result<(String, Option<String>, TokenStats)> {
        let mut run_ctx = self.create_run_context(Session::new(user_id));
        self.run_with_run_context(&mut run_ctx, input, None, None)
            .await
    }

    /// 简单运行 Agent（显式指定 Skill）
    ///
    /// 内部创建 Session，适合单次请求场景
    pub async fn run_simple_with_skill(
        &self,
        user_id: &str,
        input: &str,
        skill_name: Option<&str>,
    ) -> Result<(String, Option<String>, TokenStats)> {
        let mut run_ctx = self.create_run_context(Session::new(user_id));
        self.run_with_run_context(&mut run_ctx, input, skill_name, None)
            .await
    }

    /// 简单运行 Agent（统一 flow_template 参数）
    ///
    /// 内部创建 Session，适合单次请求场景
    /// 自动判断 flow_template 用于 Skill 还是框架流程
    pub async fn run_simple_with_template(
        &self,
        user_id: &str,
        input: &str,
        skill_name: Option<&str>,
        flow_template: Option<FlowTemplate>,
    ) -> Result<(String, Option<String>, TokenStats)> {
        let mut run_ctx = self.create_run_context(Session::new(user_id));
        self.run_with_run_context(&mut run_ctx, input, skill_name, flow_template)
            .await
    }

    /// 创建请求级运行上下文
    fn create_run_context(&self, session: Session) -> RunContext {
        RunContext::new(session)
    }

    /// 使用 RunContext 运行（核心内部方法）
    ///
    /// 分层设计：
    /// - &self: 全局状态（类似 AppState）
    /// - run_ctx: 请求级上下文（类似 Request Extensions）
    async fn run_with_run_context(
        &self,
        run_ctx: &mut RunContext,
        input: &str,
        skill_name: Option<&str>,
        flow_template: Option<FlowTemplate>,
    ) -> Result<(String, Option<String>, TokenStats)> {
        // 1. Before prompt logging
        tracing::info!(
            "[BeforePrompt] Session: {}, Input: {}",
            run_ctx.session.id,
            input
        );

        // 2. 确定使用哪个 Skill
        let skill_match = {
            let skills = self.skills.lock().unwrap();
            if let Some(name) = skill_name {
                // 显式指定 Skill
                tracing::info!("Explicit skill requested: {}", name);
                skills.get_skill_by_name(name)
            } else {
                // 智能匹配
                tracing::info!("Using auto-matching");
                skills.match_skill(input)
            }
        };

        // 3. 执行
        let response = if let Some(skill_match) = skill_match {
            // Skill 匹配成功，执行纯代码实现
            tracing::info!(
                "Executing skill: {} (confidence: {:.2})",
                skill_match.info.name,
                skill_match.confidence
            );

            // 优先使用传入的模板，否则使用 Skill 默认模板
            let template = flow_template.or_else(|| skill_match.skill.flow_template());

            // 添加到调用链
            run_ctx.add_to_chain(&skill_match.info.name);

            // 从 RunContext 创建 SkillContext
            let mut ctx = SkillContext::from_run_context(
                input,
                run_ctx,
                &self.runtime,
                &self.memory,
                skill_match.confidence,
                template,
            );

            // 执行 Skill
            let result = skill_match.skill.execute(&mut ctx).await?;
            (result, Some(skill_match.info.name))
        } else {
            // 没有 Skill 匹配，使用默认 Flow（AI 自主决策）
            // 将 FlowTemplate 转换为 FlowType
            let flow_type = flow_template.map(|t| match t {
                FlowTemplate::Simple => FlowType::Simple,
                FlowTemplate::ReAct => FlowType::React,
                FlowTemplate::PlanAct => FlowType::PlanAct,
                FlowTemplate::ChainOfThought => FlowType::React,
            });

            if let Some(ft) = flow_type {
                tracing::info!("No skill matched, using specified Flow type: {}", ft);
                let result = self
                    .flow
                    .execute_with_flow_type(&mut run_ctx.session, &self.runtime, &self.memory, ft)
                    .await?;
                (result, None)
            } else {
                tracing::info!("No skill matched, using default Flow (AI autonomous)");
                let result = self
                    .flow
                    .execute(&mut run_ctx.session, &self.runtime, &self.memory)
                    .await?;
                (result, None)
            }
        };

        // 4. 添加对话对（滑动窗口自动处理超额归档）
        let archived_pairs = run_ctx.session.add_conversation_pair(input, &response.0);

        // 5. 将超额消息归档到长期记忆
        for pair in archived_pairs {
            self.memory.archive_long_term(
                &run_ctx.session.id,
                &pair.user_message,
                &pair.assistant_message,
            )?;
        }

        // 6. After complete logging
        tracing::info!("[AfterComplete] Session: {}", run_ctx.session.id);

        // 7. 心灵层：记录本次互动（统计分析轨道）
        let skill_used_name = response
            .1
            .clone()
            .unwrap_or_else(|| "default_chat".to_string());
        if let Ok(mut soul) = self.soul.lock() {
            soul.record_interaction(&skill_used_name, input, 0);
        }

        // 8. 获取 Token 统计
        let tokens = run_ctx.tokens.read().await.clone();

        Ok((response.0, response.1, tokens))
    }

    /// 运行 Agent（流式输出）
    ///
    /// callback: 每收到一块数据时调用
    pub async fn run_streaming(
        &self,
        session: &mut Session,
        input: &str,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<String> {
        let mut run_ctx = self.create_run_context(std::mem::replace(session, Session::new("temp")));
        let result = self
            .run_streaming_with_run_context(&mut run_ctx, input, callback)
            .await;
        *session = run_ctx.session;
        result
    }

    /// 使用 RunContext 运行流式输出
    async fn run_streaming_with_run_context(
        &self,
        run_ctx: &mut RunContext,
        input: &str,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<String> {
        // 1. Before prompt logging
        tracing::info!(
            "[BeforePrompt] Session: {}, Input: {}",
            run_ctx.session.id,
            input
        );

        // 2. Skill 路由匹配
        let matched = {
            let skills = self.skills.lock().unwrap();
            skills.get_matched_skill(input)
        };

        let response = if let Some((skill_match, flow_template)) = matched {
            // Skill 匹配成功
            tracing::info!(
                "Executing skill streaming: {} (confidence: {:.2}, template: {:?})",
                skill_match.info.name,
                skill_match.confidence,
                flow_template
            );

            // 检查 Skill 是否支持流式输出
            if skill_match.skill.supports_streaming() {
                // 使用流式执行
                // 添加到调用链
                run_ctx.add_to_chain(&skill_match.info.name);

                // 从 RunContext 创建 SkillContext
                let mut ctx = SkillContext::from_run_context(
                    input,
                    run_ctx,
                    &self.runtime,
                    &self.memory,
                    skill_match.confidence,
                    flow_template,
                );

                skill_match
                    .skill
                    .execute_streaming(&mut ctx, callback)
                    .await?
            } else {
                // Skill 不支持流式，使用非流式执行
                tracing::info!(
                    "Skill {} does not support streaming, falling back to non-streaming",
                    skill_match.info.name
                );

                // 添加到调用链
                run_ctx.add_to_chain(&skill_match.info.name);

                let mut ctx = SkillContext::from_run_context(
                    input,
                    run_ctx,
                    &self.runtime,
                    &self.memory,
                    skill_match.confidence,
                    flow_template,
                );

                let result = skill_match.skill.execute(&mut ctx).await?;
                callback(result.clone());
                result
            }
        } else {
            // 没有 Skill 匹配，使用默认 Flow
            tracing::info!("No skill matched, using default Flow streaming");
            let result = self
                .flow
                .execute(&mut run_ctx.session, &self.runtime, &self.memory)
                .await?;
            callback(result.clone());
            result
        };

        // 3. 添加对话对（滑动窗口自动处理超额归档）
        let archived_pairs = run_ctx.session.add_conversation_pair(input, &response);

        // 4. 将超额消息归档到长期记忆
        for pair in archived_pairs {
            self.memory.archive_long_term(
                &run_ctx.session.id,
                &pair.user_message,
                &pair.assistant_message,
            )?;
        }

        // 5. After complete logging
        tracing::info!("[AfterComplete] Session: {}", run_ctx.session.id);

        Ok(response)
    }

    /// 简单运行 Agent（流式输出，HTTP 友好版本）
    ///
    /// 内部创建 Session，适合单次请求场景
    pub async fn run_simple_streaming(
        &self,
        user_id: &str,
        input: &str,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<String> {
        let mut run_ctx = self.create_run_context(Session::new(user_id));
        self.run_streaming_with_run_context(&mut run_ctx, input, callback)
            .await
    }

    /// 获取所有已注册 Skill 的信息列表
    pub fn list_skills(&self) -> Vec<SkillInfo> {
        self.skills.lock().unwrap().get_skills()
    }

    /// 获取 Skill 数量
    pub fn skill_count(&self) -> usize {
        self.skills.lock().unwrap().get_skills().len()
    }
}

struct ExpertPluginAdapter {
    plugin: std::sync::Arc<dyn expert::ExpertPlugin>,
    runtime: Arc<Runtime>,
    id: String,
    name: String,
    priority: u32,
}

impl std::fmt::Debug for ExpertPluginAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpertPluginAdapter")
            .field("id", &self.id)
            .finish()
    }
}

impl ExpertPluginAdapter {
    fn new(plugin: std::sync::Arc<dyn expert::ExpertPlugin>, runtime: Arc<Runtime>) -> Self {
        Self {
            plugin: plugin.clone(),
            runtime,
            id: plugin.manifest().id,
            name: plugin.info().name,
            priority: 0,
        }
    }
}

#[async_trait::async_trait]
impl ExpertAgent for ExpertPluginAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn tags(&self) -> Vec<String> {
        self.plugin.manifest().keywords.clone()
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    async fn run(&self, ctx_id: &str, store: &mut ContextStore) -> Result<CtxId> {
        let context = store
            .get(ctx_id)
            .ok_or_else(|| anyhow::anyhow!("Context not found: {}", ctx_id))?;

        let persona = self.plugin.persona();
        let prompt = format!(
            r#"你是 {}。
角色背景：{}
目标：{}

当前任务：{}

请直接回答这个任务。"#,
            persona.name, persona.description, persona.name, context.content
        );

        let messages = vec![
            Message {
                role: Role::System,
                content: format!(
                    "你是 {}。角色背景：{}。目标：{}",
                    persona.name, persona.description, persona.name
                ),
                tool_call_id: None,
            },
            Message {
                role: Role::User,
                content: prompt,
                tool_call_id: None,
            },
        ];

        let response = self.runtime.call_llm(messages).await?;

        Ok(store.put(ContextData {
            content: response,
            metadata: HashMap::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }))
    }
}

impl Default for Subhuti {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_subhuti_creation() {
        let subhuti = Subhuti::new();
        assert!(subhuti.memory().is_empty());
    }
}
