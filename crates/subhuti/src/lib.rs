pub mod config;

pub mod event {
    pub use subhuti_core::event::types::{AgentEventData, ArcEvent, Event, EventId, EventMetadata};
    pub use subhuti_core::{EventBus, EventFilter, EventHandler, EventRecorder, EventSubscription};
}

pub mod graph {
    pub use subhuti_core::graph::state::reducers;
    pub use subhuti_core::{
        ActorAddr, ActorHandle, ActorHealth, ActorLifecycle, ActorStats, Checkpoint,
        CheckpointStore, ConditionalEdge, Edge, EventDrivenActor, EventDrivenScheduler, Graph,
        GraphBuilder, GraphError, GraphNode, GraphOutput, GraphState, GraphStructure,
        MemoryCheckpointStore, NodeActor, NodeFn, NodeMessage, NodeResult, Route, StateReducer,
        SupervisionStrategy, Supervisor,
    };
}

pub mod memory {
    pub use subhuti_core::memory::Memory as MemoryTrait;
    pub use subhuti_infra::memory::*;
}

pub mod runtime {
    pub use subhuti_core::{
        LLMResponse, Message, Role, Session, Tool, ToolCall, ToolCallResult, ToolInfo,
        ToolResponse, ToolResult, LLM,
    };
    pub mod llm {
        pub use subhuti_core::{
            LLMConfig, LLMProvider, LLMResponse, Message, Role, ToolCall, ToolInfo, LLM,
        };
        pub use subhuti_infra::{
            CacheStats, CachedLLM, DoubaoClient, DoubaoConfig, MockLLM as MockLlmClient,
            OllamaClient, OllamaConfig, OpenAIClient, OpenAIConfig, ZhipuClient, ZhipuConfig,
        };
    }
}

pub mod vertical {
    pub use subhuti_core::{
        Asset, AssetLibrary, ProjectInfo, ProjectMemory, ProjectNote, ToolCommand, ToolCommandInfo,
        ToolIntegration, ToolRegistry, Workflow, WorkflowStore,
    };
    pub use subhuti_infra::vertical::{
        MemoryAssetLibrary, MemoryProjectMemory, MemoryToolRegistry, MemoryWorkflowStore,
    };
}

pub mod flow {
    pub use crate::config::FlowConfig;
    pub use crate::config::FlowTemplate;
}

pub mod orchestrator {
    pub use subhuti_core::{
        AgentContext, AgentRegistry, EventBusRef, ExpertAgent, ExpertState, FromState,
        GraphOrchestrator, Llm, MemoryRef, OrchestrationResult, Orchestrator,
    };
    // 规则引擎三层 trait（供应用层实现自定义规则）
    pub use subhuti_core::orchestrator::{
        DefaultDispatchRule, DefaultExecutionRule, DefaultTaskAnalysisRule, DispatchPlan,
        DispatchRule, DispatchStrategy, ExecutionResult, ExecutionRule, FrameworkExpertInfo,
        ResultStrategy, RuleConfig, RuleEngine, SkillInfo, Step, TaskAnalysisRule, TaskProfile,
    };
}

pub use config::{
    DbConfig, EmotionalTendency, FeedbackType, FlowConfig, FlowTemplate, LLMConfig, LLMProvider,
    MemoryConfig, OrchestrationResult, RuntimeConfig, Skill, SoulProfile, SubhutiConfig,
    TokenUsage, ToneStyle,
};
pub use subhuti_infra::tool::{
    CalculatorTool, FileReadTool, FileWriteTool, WeatherTool, WebSearchTool,
};
pub use subhuti_infra::{
    BaseStats, CacheStats, CachedLLM, ConnectionDynamics, ConvoExchange, ConvoMiner, Database,
    DatabaseStore, DedupConfig, DedupResult, Deduplicator, DefaultMemory, DoubaoClient,
    DoubaoConfig, EmbeddingConfig, EmbeddingService, Entity, EntityExtractor, EntityRegistry,
    EntitySource, EntityType, FactChecker, FactIssue, FeedbackRow, Hallway, HistoryRow,
    HybridSearchResult, HybridSearcher, IssueType, KeepStrategy, KnowledgeGraph,
    KnowledgeGraphStats, KnowledgeMemory, LayerOutput, LongTermMemory, Memory, MemoryItem,
    MemoryLayer, MemoryLayerConfig, MemoryRow, MemoryStack, MemoryStats, MemoryStore, MinedMemory,
    MockLLM, OllamaClient, OllamaConfig, OpenAIClient, OpenAIConfig, PalaceGraph, PalaceGraphStats,
    PersonaData, PersonaRow, QueryDirection, Room, SearchResult, SemanticSearchResult,
    ShortTermMemory, SqliteMemoryStore, Triple, Tunnel, Wing, ZhipuClient, ZhipuConfig,
};

pub use subhuti_core::{
    AgentContext, AgentRegistry, Event, ExpertAgent, ExpertState, FrameworkExpertInfo, Graph,
    GraphBuilder, GraphState, LLMResponse, Message, NodeResult, Orchestrator, Role, Route,
    ToolCall, ToolInfo, LLM,
};
pub use subhuti_core::{Error, Result};

use std::sync::Arc;

pub struct Subhuti {
    memory: Arc<memory::Memory>,
    event_bus: Arc<subhuti_core::event::EventBus>,
    orchestrator: tokio::sync::Mutex<subhuti_core::orchestrator::Orchestrator>,
    asset_library: Arc<subhuti_infra::vertical::MemoryAssetLibrary>,
    project_memory: Arc<subhuti_infra::vertical::MemoryProjectMemory>,
    tool_registry: Arc<subhuti_infra::vertical::MemoryToolRegistry>,
    workflow_store: Arc<subhuti_infra::vertical::MemoryWorkflowStore>,
    llm: Option<Arc<dyn LLM>>,
}

impl Subhuti {
    pub fn new() -> Self {
        let memory = Arc::new(memory::Memory::new());
        let event_bus = Arc::new(subhuti_core::event::EventBus::new(1024));
        let asset_library = subhuti_infra::vertical::MemoryAssetLibrary::arc();
        let project_memory = subhuti_infra::vertical::MemoryProjectMemory::arc();
        let tool_registry = subhuti_infra::vertical::MemoryToolRegistry::arc();
        let workflow_store = subhuti_infra::vertical::MemoryWorkflowStore::arc();

        let mut orchestrator = subhuti_core::orchestrator::Orchestrator::new();
        orchestrator = orchestrator.with_event_bus(event_bus.clone());

        Self {
            memory,
            event_bus,
            orchestrator: tokio::sync::Mutex::new(orchestrator),
            asset_library,
            project_memory,
            tool_registry,
            workflow_store,
            llm: None,
        }
    }

    pub fn set_llm(&mut self, llm: Arc<dyn LLM>) {
        self.llm = Some(llm);
    }

    /// 获取当前已注入的 LLM 实例（便于运行时临时切模型）。
    ///
    /// 典型用法：外部拿到后，先 `Arc::downcast_ref::<ZhipuClient>()` 到具体类型，
    /// 调用 `.with_model("glm-4.7-flash")` 生成新实例，再用 `set_llm` 覆盖。
    pub fn current_llm(&self) -> Option<Arc<dyn LLM>> {
        self.llm.clone()
    }

    /// 运行时一键切换模型，复用当前 provider 的 api_key / api_url / temperature / max_tokens 配置。
    ///
    /// 外部只需关心模型名字，无需知道底层是哪种 Client：
    ///
    /// ```ignore
    /// // 默认跑日常任务
    /// subhuti.swap_model("glm-4-flash")?;
    /// // 要深度思考的场景临时切到 4.7
    /// subhuti.swap_model("glm-4.7-flash")?;
    /// // 跑完切回标准版
    /// subhuti.swap_model("glm-4-flash")?;
    /// ```
    ///
    /// 返回切换后的新模型名（空字符串会被自动回退到对应 provider 的默认模型）。
    pub fn swap_model(
        &mut self,
        model_name: impl Into<String>,
    ) -> std::result::Result<String, &'static str> {
        use subhuti_core::runtime::llm::LLM as _LLMTrait;
        use subhuti_core::LLMProvider as _Provider;
        use subhuti_infra::{
            DoubaoClient, DoubaoConfig, OllamaClient, OllamaConfig, OpenAIClient, OpenAIConfig,
            ZhipuClient, ZhipuConfig,
        };

        let current = self
            .llm
            .clone()
            .ok_or("尚未注入 LLM 实例，请先调用 set_llm / 走初始化流程")?;

        let provider = _LLMTrait::provider(&*current);
        let cfg = _LLMTrait::config(&*current).clone();
        let mut new_model: String = model_name.into();

        fn default_for_empty(s: &str, default: &str) -> String {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                default.to_string()
            } else {
                s.to_string()
            }
        }

        let new_llm: std::sync::Arc<dyn _LLMTrait> = match provider {
            _Provider::Zhipu => {
                new_model = default_for_empty(&new_model, "glm-4-flash");
                let concrete = ZhipuConfig {
                    api_key: cfg.api_key.unwrap_or_default(),
                    api_url: cfg.api_url,
                    model: new_model.clone(),
                    temperature: cfg.temperature,
                    max_tokens: cfg.max_tokens,
                };
                std::sync::Arc::new(ZhipuClient::new(concrete))
            }
            _Provider::OpenAI => {
                new_model = default_for_empty(&new_model, "gpt-4o-mini");
                let concrete = OpenAIConfig {
                    api_key: cfg.api_key.unwrap_or_default(),
                    api_url: cfg.api_url,
                    model: new_model.clone(),
                    temperature: cfg.temperature,
                    max_tokens: cfg.max_tokens,
                };
                std::sync::Arc::new(OpenAIClient::new(concrete))
            }
            _Provider::Doubao => {
                new_model = default_for_empty(&new_model, "doubao-pro");
                let concrete = DoubaoConfig {
                    api_key: cfg.api_key.unwrap_or_default(),
                    api_url: cfg.api_url,
                    model: new_model.clone(),
                    temperature: cfg.temperature,
                    max_tokens: cfg.max_tokens,
                };
                std::sync::Arc::new(DoubaoClient::new(concrete))
            }
            _Provider::Ollama => {
                new_model = default_for_empty(&new_model, "llama3");
                let concrete = OllamaConfig {
                    api_url: cfg.api_url,
                    model: new_model.clone(),
                    temperature: cfg.temperature,
                    max_tokens: cfg.max_tokens,
                };
                std::sync::Arc::new(OllamaClient::new(concrete))
            }
            _Provider::Custom => {
                return Err(
                    "当前 LLM 是自定义 provider（Custom，含 MockLLM），无法自动重建；请把原始 Client 保存在应用层，用 with_model + set_llm 手动切换",
                );
            }
        };

        self.llm = Some(new_llm);
        Ok(new_model)
    }

    pub fn with_config(_config: SubhutiConfig) -> Self {
        // TODO: 实际消费配置（LLM / Memory / Orchestrator 设置等）
        Self::new()
    }

    pub fn event_bus(&self) -> &Arc<subhuti_core::event::EventBus> {
        &self.event_bus
    }

    pub async fn register_agent(&self, agent: Arc<dyn subhuti_core::orchestrator::ExpertAgent>) {
        self.orchestrator.lock().await.register_agent(agent);
    }

    pub async fn register_graph(&self, mut graph: subhuti_core::Graph) {
        // 注入框架级 EventBus，使 graph 执行时能 emit Flow*/Node* 事件
        // 没有 event_bus 时 graph.run() 内部的 emit_event 全部跳过，
        // 且 execute_graph 会走 graph.run() 而非 run_event_driven()
        graph.set_event_bus(self.event_bus.clone());
        self.orchestrator.lock().await.register_graph(graph);
    }

    pub async fn dispatch(&self, input: &str) -> subhuti_core::orchestrator::OrchestrationResult {
        let mut ctx = subhuti_core::orchestrator::AgentContext::new(input, "default");
        let state = subhuti_core::orchestrator::ExpertState::builder(
            self.memory.clone() as Arc<dyn subhuti_core::memory::Memory>
        )
        .event_bus(self.event_bus.clone())
        .asset_library(self.asset_library.clone() as Arc<dyn subhuti_core::vertical::AssetLibrary>)
        .project_memory(
            self.project_memory.clone() as Arc<dyn subhuti_core::vertical::ProjectMemory>
        )
        .tool_registry(self.tool_registry.clone() as Arc<dyn subhuti_core::vertical::ToolRegistry>)
        .workflow_store(
            self.workflow_store.clone() as Arc<dyn subhuti_core::vertical::WorkflowStore>
        )
        .build();

        self.orchestrator
            .lock()
            .await
            .dispatch(&mut ctx, &state)
            .await
    }

    /// 使用自定义上下文执行编排
    pub async fn dispatch_with_context(
        &self,
        mut ctx: subhuti_core::orchestrator::AgentContext,
    ) -> subhuti_core::orchestrator::OrchestrationResult {
        let state = subhuti_core::orchestrator::ExpertState::builder(
            self.memory.clone() as Arc<dyn subhuti_core::memory::Memory>
        )
        .event_bus(self.event_bus.clone())
        .asset_library(self.asset_library.clone() as Arc<dyn subhuti_core::vertical::AssetLibrary>)
        .project_memory(
            self.project_memory.clone() as Arc<dyn subhuti_core::vertical::ProjectMemory>
        )
        .tool_registry(self.tool_registry.clone() as Arc<dyn subhuti_core::vertical::ToolRegistry>)
        .workflow_store(
            self.workflow_store.clone() as Arc<dyn subhuti_core::vertical::WorkflowStore>
        )
        .build();

        self.orchestrator
            .lock()
            .await
            .dispatch(&mut ctx, &state)
            .await
    }

    pub async fn init_database(&self, _db_config: &DbConfig) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn register_orchestrator_expert(
        &self,
        agent: Arc<dyn subhuti_core::orchestrator::ExpertAgent>,
    ) {
        self.orchestrator.lock().await.register_agent(agent);
    }

    pub async fn sync_experts_to_orchestrator(&self) {}

    /// 构建共享的 ExpertState（供应用层创建图节点等使用）
    ///
    /// 所有依赖均为 Arc 引用，构建一次即可安全共享。
    pub fn build_expert_state(&self) -> subhuti_core::orchestrator::ExpertState {
        let mut builder = subhuti_core::orchestrator::ExpertState::builder(
            self.memory.clone() as Arc<dyn subhuti_core::memory::Memory>
        )
        .event_bus(self.event_bus.clone())
        .asset_library(self.asset_library.clone() as Arc<dyn subhuti_core::vertical::AssetLibrary>)
        .project_memory(
            self.project_memory.clone() as Arc<dyn subhuti_core::vertical::ProjectMemory>
        )
        .tool_registry(self.tool_registry.clone() as Arc<dyn subhuti_core::vertical::ToolRegistry>)
        .workflow_store(
            self.workflow_store.clone() as Arc<dyn subhuti_core::vertical::WorkflowStore>
        );
        if let Some(ref llm) = self.llm {
            builder = builder.llm(llm.clone());
        }
        builder.build()
    }

    /// 替换任务分析规则（Layer 1）
    pub async fn set_analysis_rule(
        &self,
        rule: Arc<dyn subhuti_core::orchestrator::TaskAnalysisRule>,
    ) {
        self.orchestrator.lock().await.set_analysis_rule(rule);
    }

    /// 替换调度决策规则（Layer 2）
    pub async fn set_dispatch_rule(&self, rule: Arc<dyn subhuti_core::orchestrator::DispatchRule>) {
        self.orchestrator.lock().await.set_dispatch_rule(rule);
    }

    /// 替换执行监控规则（Layer 3）
    pub async fn set_execution_rule(
        &self,
        rule: Arc<dyn subhuti_core::orchestrator::ExecutionRule>,
    ) {
        self.orchestrator.lock().await.set_execution_rule(rule);
    }

    pub fn active_expert_info(&self) -> Option<FrameworkExpertInfo> {
        // TODO: 实现真正的激活专家存储
        None
    }

    /// 任务分析（Layer 1）：透传到 RuleEngine，返回真实 TaskProfile 序列化结果。
    ///
    /// 失败时返回带 `error` 字段的 json，不 panic，保证 HTTP `/orchestrate/analyze` 永远有响应。
    pub async fn analyze_task(&self, message: &str) -> serde_json::Value {
        let orchestrator = self.orchestrator.lock().await;
        match orchestrator.rule_engine().analyze_task(message) {
            Ok(profile) => {
                tracing::info!(
                    "[Subhuti·analyze_task] domain_tags={:?}, task_type={}",
                    profile.domain_tags,
                    profile.task_type
                );
                serde_json::to_value(profile)
                    .unwrap_or_else(|_| serde_json::json!({ "error": "TaskProfile 序列化失败" }))
            }
            Err(e) => {
                tracing::warn!("[Subhuti·analyze_task] 分析失败: {}", e);
                serde_json::json!({
                    "error": e.to_string(),
                    "input": message,
                })
            }
        }
    }

    /// 专家匹配（Layer 1 + Layer 2）：先 analyze 拿 TaskProfile，再 decide_strategy
    /// 拿 DispatchPlan，最后按 plan.steps 里的 agent_id 从快照表查出强类型专家信息。
    ///
    /// 返回顺序按 DispatchPlan.steps 排序（已经过打分 + 过滤 + 限量）。
    pub async fn match_expert(&self, input: &str) -> Vec<FrameworkExpertInfo> {
        let orchestrator = self.orchestrator.lock().await;

        // Layer 1: 任务分析
        let profile = match orchestrator.rule_engine().analyze_task(input) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[Subhuti·match_expert] analyze_task 失败: {}", e);
                return Vec::new();
            }
        };

        // Layer 2: 调度决策（内部已做关键词匹配 + 打分 + 过滤）
        let agents = orchestrator.list_experts();
        let plan = match orchestrator
            .rule_engine()
            .decide_strategy(&profile, &agents)
        {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[Subhuti·match_expert] decide_strategy 失败: {}", e);
                return Vec::new();
            }
        };

        // 拿到所有专家快照，按 plan.steps 顺序挑出匹配到的
        let snapshots = orchestrator.list_expert_snapshots();
        let matched: Vec<FrameworkExpertInfo> = plan
            .steps
            .iter()
            .filter_map(|step| snapshots.iter().find(|s| s.id == step.agent_id).cloned())
            .collect();

        tracing::info!(
            "[Subhuti·match_expert] input={:?}, domain_tags={:?}, strategy={:?}, matched={:?}",
            input,
            profile.domain_tags,
            plan.strategy,
            matched.iter().map(|m| m.id.clone()).collect::<Vec<_>>()
        );
        matched
    }

    /// 获取专家快照列表（强类型 DTO，用于适配器展示/查询）
    pub async fn list_orchestrator_experts(&self) -> Vec<FrameworkExpertInfo> {
        let orchestrator = self.orchestrator.lock().await;
        orchestrator.list_expert_snapshots()
    }

    /// 获取真实 Agent trait 对象列表（用于图注册/内部调度等需要实际 Agent 的场景）
    ///
    /// 与 `list_orchestrator_experts` 的区别：
    /// - 返回真实 `Arc<dyn ExpertAgent>`，可以调用 `.run()`
    /// - 仅内部/基础设施层使用，表现层适配器一律走快照接口
    pub async fn get_orchestrator_agents(
        &self,
    ) -> Vec<Arc<dyn subhuti_core::orchestrator::ExpertAgent>> {
        let orchestrator = self.orchestrator.lock().await;
        orchestrator.list_experts()
    }

    /// 通过技能 ID 反向查找所属的专家（返回专家 ID + 真实 Agent）
    ///
    /// 用于 `SkillExecutionPort::execute_skill`，替代原先 JSON Value 遍历。
    pub async fn find_agent_by_skill(
        &self,
        skill_id: &str,
    ) -> Option<(String, Arc<dyn subhuti_core::orchestrator::ExpertAgent>)> {
        let orchestrator = self.orchestrator.lock().await;
        for agent in orchestrator.list_experts() {
            if agent.skills().iter().any(|s| s.id == skill_id) {
                return Some((agent.id().to_string(), agent));
            }
        }
        None
    }
}
