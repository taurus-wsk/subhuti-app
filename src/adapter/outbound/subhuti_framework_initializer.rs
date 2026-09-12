//! # Subhuti 框架初始化器
//!
//! 框架初始化器：由组合根（CompositionRoot）直接使用，
//! 将应用层的初始化请求转换为 Subhuti 框架的具体操作。
//!
//! 设计说明：
//! - 初始化逻辑与具体框架（Subhuti）强绑定，无需多态，故不抽象为 trait
//! - 组合根作为唯一组装点，允许依赖具体类型（组合根特权）
//! - 运行时出站端口适配器由 build_adapters() 提供，供组合根构造 OrchestrationService

use std::sync::Arc;
use std::sync::Mutex;

use subhuti_core::engine::Subhuti;
use subhuti_core::event::EventBus;
use subhuti_core::memory::Memory;
use subhuti_core::sutra_library::SutraLibraryPort;
use subhuti_core::vertical::{AssetLibrary, ProjectMemory, ToolRegistry, WorkflowStore};
use subhuti_core::LLMConfig;
use subhuti_core::LLMProvider;
use subhuti_infra::sutra_library::create_sutra_engine;
use subhuti_infra::vertical::{
    MemoryAssetLibrary, MemoryProjectMemory, MemoryToolRegistry, MemoryWorkflowStore,
};
use subhuti_infra::CachedLLM;
use subhuti_infra::{
    ContextLimitLLM, DoubaoClient, DoubaoConfig, LimitConfig, MockLLM, OllamaClient, OllamaConfig,
    OpenAIClient, OpenAIConfig, RetryConfig, RetryLLM, ZhipuClient, ZhipuConfig,
};

use crate::adapter::outbound::rules;
use crate::adapter::outbound::subhuti_expert_repository::SubhutiExpertRepository;
use crate::adapter::outbound::subhuti_orchestration_engine::SubhutiOrchestrationEngine;
use crate::adapter::outbound::subhuti_skill_executor::SubhutiSkillExecutor;
use crate::application::observer::{record_fn_log, LogLevel, TraceObserverPort};
use crate::domain::ports::CommandPort;
use crate::domain::ports::FileSystemPort;
use crate::domain::ports::ToolchainPort;
use crate::domain::ports::{ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort};
use crate::domain::traits::{DomainExpert, DomainRepository};
use crate::infra::config::AppConfig;

/// 出站端口适配器集合（由 SubhutiFrameworkInitializer 构建，供组合根使用）
///
/// 组合根（CompositionRoot）调用 `SubhutiFrameworkInitializer::build_adapters()` 获取此结构体，
/// 用于构造 OrchestrationService。初始化与运行时适配器创建都由具体类型直接提供。
pub struct AppAdapters {
    pub expert_repository: Arc<dyn ExpertRepositoryPort>,
    pub orchestration_engine: Arc<dyn OrchestrationEnginePort>,
    pub skill_executor: Arc<dyn SkillExecutionPort>,
}

/// Subhuti 框架初始化器
///
/// 持有 Subhuti 框架对象，提供初始化与运行时适配器构建能力。
/// 所有框架相关的初始化逻辑都在这里处理。
pub struct SubhutiFrameworkInitializer {
    subhuti: Arc<Subhuti>,
    app_config: Arc<AppConfig>,
    trace_observer: Mutex<Option<Arc<dyn TraceObserverPort>>>,
}

impl SubhutiFrameworkInitializer {
    /// 创建新的框架初始化器
    pub fn new(app_config: Arc<AppConfig>) -> Self {
        // 1) 解析 LLM Provider + 按 provider 读取对应环境变量的 api_key
        let provider_str = app_config.llm.provider.as_str();
        let (provider, api_key) = match provider_str {
            "openai" => (LLMProvider::OpenAI, std::env::var("OPENAI_API_KEY").ok()),
            "ollama" => (LLMProvider::Ollama, None), // Ollama 本地不需要 api_key
            "doubao" => (LLMProvider::Doubao, std::env::var("DOUBAO_API_KEY").ok()),
            "zhipu" => (LLMProvider::Zhipu, std::env::var("ZHIPU_API_KEY").ok()),
            _ => (LLMProvider::Zhipu, std::env::var("ZHIPU_API_KEY").ok()),
        };

        // 构建 Subhuti 配置
        let subhuti_llm_config = LLMConfig {
            model: app_config.llm.model.clone(),
            api_url: app_config.llm.api_url.clone(),
            api_key: api_key.clone(),
            temperature: app_config.llm.temperature as f32,
            max_tokens: app_config.llm.max_tokens,
        };

        let memory: Arc<dyn Memory> = Arc::new(subhuti_infra::memory::Memory::new());
        let event_bus = Arc::new(EventBus::new(1024));
        let asset_library: Arc<dyn AssetLibrary> = MemoryAssetLibrary::arc();
        let project_memory: Arc<dyn ProjectMemory> = MemoryProjectMemory::arc();
        let tool_registry: Arc<dyn ToolRegistry> = MemoryToolRegistry::arc();
        let workflow_store: Arc<dyn WorkflowStore> = MemoryWorkflowStore::arc();

        let mut subhuti = Subhuti::new(
            memory,
            event_bus.clone(),
            asset_library,
            project_memory,
            tool_registry,
            workflow_store,
        );

        // 事件总线内置处理器（日志 / Trace 事件）在 init() 的异步上下文中 await 注册；
        // 此处为同步构造函数，不能 await，故不在此调用（否则 future 被立即丢弃，处理器永不注册）。
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "✅ EventBus initialized with builtin handlers",
            None,
        );

        // 2) 根据 test_mode + provider 构建并注入 LLM client
        if app_config.test_mode.enabled {
            let mock_client = MockLLM::new();
            subhuti.set_llm(Arc::new(mock_client));
            record_fn_log(
                None,
                "",
                LogLevel::Info,
                "✅ 测试模式已启用，使用 Mock LLM",
                None,
            );
        } else {
            // 按 provider 构建真实 LLM client
            let llm_client: Arc<dyn subhuti_core::LLM> = match provider {
                LLMProvider::OpenAI => {
                    let cfg = OpenAIConfig {
                        api_key: api_key.unwrap_or_default(),
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Info,
                        format!("✅ LLM: OpenAI ({})", cfg.model),
                        None,
                    );
                    Arc::new(OpenAIClient::new(cfg))
                }
                LLMProvider::Ollama => {
                    let cfg = OllamaConfig {
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Info,
                        format!("✅ LLM: Ollama ({})", cfg.model),
                        None,
                    );
                    Arc::new(OllamaClient::new(cfg))
                }
                LLMProvider::Doubao => {
                    let cfg = DoubaoConfig {
                        api_key: api_key.unwrap_or_default(),
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Info,
                        format!("✅ LLM: Doubao ({})", cfg.model),
                        None,
                    );
                    Arc::new(DoubaoClient::new(cfg))
                }
                LLMProvider::Zhipu => {
                    let cfg = ZhipuConfig {
                        api_key: api_key.unwrap_or_default(),
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Info,
                        format!("✅ LLM: Zhipu/智谱 ({})", cfg.model),
                        None,
                    );
                    Arc::new(ZhipuClient::new(cfg))
                }
                LLMProvider::Custom => {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Warn,
                        "⚠️  LLM provider = custom，未注入任何真实 client",
                        None,
                    );
                    Arc::new(MockLLM::new())
                }
            };

            // ── P0 韧性：上下文裁剪 → 重试/超时（由内到外，顺序有意义）──
            //   1) ContextLimitLLM（最内）：先把 messages 裁到预算内，再交给重试层，
            //      保证每次重试用的都是同一份已裁剪输入，不会因重试而放大请求体
            //   2) RetryLLM（外一层）：对临时性失败（超时 / 429 / 5xx）指数退避重试，
            //      并给每次调用套整体超时
            //   3) CachedLLM（最外，仅调试）：以「裁剪后」的输入为 cache key，
            //      与真实发给模型的请求保持一致
            let limit_cfg = LimitConfig::from_env();
            let retry_cfg = RetryConfig::from_env();
            let llm_client: Arc<dyn subhuti_core::LLM> =
                RetryLLM::wrap(ContextLimitLLM::wrap(llm_client, limit_cfg), retry_cfg);
            record_fn_log(
                None,
                "",
                LogLevel::Info,
                format!(
                    "🛡️ LLM 韧性：上下文裁剪={}(messages≤{}, chars≤{}) ｜ 重试={}(尝试≤{}次, 超时={}s, 退避={}~{}ms)",
                    if limit_cfg.enabled { "开" } else { "关" },
                    limit_cfg.max_messages,
                    limit_cfg.max_chars,
                    if retry_cfg.enabled { "开" } else { "关" },
                    retry_cfg.max_attempts,
                    retry_cfg.timeout_secs,
                    retry_cfg.base_delay_ms,
                    retry_cfg.max_delay_ms,
                ),
                None,
            );

            // 🧪 调试缓存：env SUBHUTI_LLM_CACHE=1 时用 CachedLLM 包装真实 client，
            //    相同输入直接返回缓存结果，避免反复打智谱 API（默认上限 100 条，LRU）
            //
            //    ⚠️  生产环境强制禁用：
            //    - release build（cfg!(not(debug_assertions))）即使读到 SUBHUTI_LLM_CACHE=1
            //      也会忽略并打印 warn，防止误把缓存带到生产
            //    - 本地开发（debug build）才允许生效，.env 默认 SUBHUTI_LLM_CACHE=1
            let cache_enabled_debug = cfg!(not(debug_assertions)) == false
                && std::env::var("SUBHUTI_LLM_CACHE")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);

            let llm_to_inject: Arc<dyn subhuti_core::LLM> = if cache_enabled_debug {
                let cache_path = std::env::var("SUBHUTI_LLM_CACHE_PATH")
                    .unwrap_or_else(|_| ".llm-cache.json".to_string());
                record_fn_log(
                    None,
                    "",
                    LogLevel::Info,
                    format!(
                        "🧪 LLM 调试缓存已开启：path={}, max_entries=100（相同输入复用上次结果）",
                        cache_path
                    ),
                    None,
                );
                CachedLLM::wrap(llm_client, cache_path)
            } else {
                // release 构建但环境变量仍为 1 时，显式 warn 提醒
                if cfg!(not(debug_assertions))
                    && std::env::var("SUBHUTI_LLM_CACHE")
                        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                        .unwrap_or(false)
                {
                    record_fn_log(None, "", LogLevel::Warn, "⚠️  检测到 SUBHUTI_LLM_CACHE=1，但当前是 release build，已强制禁用调试缓存", None);
                }
                llm_client
            };

            subhuti.set_llm(llm_to_inject);
        }

        // ── 初始化藏经阁引擎（内存版，PG 初始化在 CompositionRoot 中完成） ──
        let (_sutra_engine, _sutra_skill) = create_sutra_engine(None);
        subhuti.set_sutra_library(_sutra_engine as Arc<dyn SutraLibraryPort>);

        Self {
            subhuti: Arc::new(subhuti),
            app_config,
            trace_observer: Mutex::new(None),
        }
    }

    /// 初始化藏经阁引擎的 PG 持久化（在 CompositionRoot 创建 PG Pool 后调用）
    pub async fn init_sutra_library_pg(&self, pg_pool: sqlx::PgPool) {
        let pool = Arc::new(pg_pool);
        let pg = Arc::new(subhuti_infra::sutra_library::storage::PgStorage::new(
            pool.clone(),
        ));
        if let Err(e) = pg.ensure_tables().await {
            tracing::warn!("SutraLibrary: PG table init failed: {}", e);
        } else {
            tracing::info!("✅ 藏经阁引擎 PG 表已就绪");
        }
        // 重建带 PG 的引擎
        let (sutra_engine, _sutra_skill) = create_sutra_engine(Some((*pool).clone()));
        self.subhuti
            .set_sutra_library(sutra_engine as Arc<dyn SutraLibraryPort>);
    }

    /// 设置 trace_observer（用于函数调用链路追踪）
    ///
    /// 由组合根在构建 `AppAdapters` 之前调用，将已创建的 trace_observer 传递给引擎。
    pub fn set_trace_observer(&self, observer: Arc<dyn TraceObserverPort>) {
        *self.trace_observer.lock().unwrap() = Some(observer);
    }

    /// 初始化框架（创建框架实例、配置 LLM、数据库等）
    pub async fn init(&self) -> anyhow::Result<()> {
        let subhuti = self.subhuti.clone();

        // 初始化事件总线内置处理器（日志 / Trace 事件）
        subhuti.event_bus().init_builtin_handlers().await;

        let is_test_mode = self.app_config.test_mode.enabled;

        // ── LLM 健康检查（真实 provider 才会打网络） ──
        if !is_test_mode {
            if let Some(llm) = subhuti.current_llm() {
                let provider = subhuti_core::LLM::provider(&*llm);
                let cfg = subhuti_core::LLM::config(&*llm).clone();
                match subhuti_core::LLM::health_check(&*llm).await {
                    Ok(true) => record_fn_log(None, "", LogLevel::Info, format!("✅ LLM 健康检查通过：provider={:?}, model={}", provider, cfg.model), None),
                    Ok(false) => record_fn_log(None, "", LogLevel::Warn, format!("⚠️  LLM 健康检查返回 false：provider={:?}, model={}, api_url={}；请检查 API Key / 网络", provider, cfg.model, cfg.api_url), None),
                    Err(e) => record_fn_log(None, "", LogLevel::Warn, format!("⚠️  LLM 健康检查调用失败：provider={:?}, model={}, error={}", provider, cfg.model, e), None),
                }
            } else {
                record_fn_log(
                    None,
                    "",
                    LogLevel::Warn,
                    "⚠️  Subhuti 尚未注入 LLM 实例，跳过健康检查",
                    None,
                );
            }
        }

        // ── 专家已通过 register_expert 注册到 Orchestrator（框架不再持有任何 Graph）──
        Ok(())
    }

    /// 注册事件处理器到框架 EventBus
    ///
    /// 用于 TraceEventBridge 等项目侧 handler。
    pub async fn register_event_handler(
        &self,
        handler: Arc<dyn subhuti_core::event::EventHandler>,
    ) {
        self.subhuti.event_bus().subscribe(handler).await;
    }

    /// 注册领域专家到框架
    pub async fn register_expert(
        &self,
        expert: Arc<dyn DomainExpert>,
        repository: Arc<dyn DomainRepository>,
        toolchain: Option<Arc<dyn ToolchainPort>>,
        file_system: Option<Arc<dyn FileSystemPort>>,
        command: Option<Arc<dyn CommandPort>>,
    ) {
        let subhuti = self.subhuti.clone();
        let expert_name = expert.name().to_string();

        // 创建领域专家适配器（领域→框架）
        let event_bus = self.subhuti.event_bus().clone();
        let adapter: Arc<
            crate::adapter::outbound::domain_expert_adapter::DomainExpertAdapter<dyn DomainExpert>,
        > = Arc::new(
            crate::adapter::outbound::domain_expert_adapter::DomainExpertAdapter::new(
                expert,
                repository,
                toolchain,
                file_system,
                command,
                Some(event_bus),
            ),
        );

        // 注册到框架的 Orchestrator（作为 ExpertAgent）
        subhuti.register_orchestrator_expert(adapter.clone()).await;

        // 同时注册为 Actor（全局演员池，用于竞标制）
        let actor = subhuti_core::orchestrator::ExpertAgentActorAdapter::new(adapter);
        subhuti.register_actor(Arc::new(actor)).await;
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            format!("注册领域专家: {}", expert_name),
            None,
        );
    }

    /// 设置任务分析规则
    pub async fn set_analysis_rule(&self) {
        let subhuti = self.subhuti.clone();
        let rules = rules::create_all_rules();

        subhuti.set_analysis_rule(rules.analysis_rule).await;
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "注册领域规则: analysis=default",
            None,
        );
    }

    /// 设置调度规则
    pub async fn set_dispatch_rule(&self) {
        let subhuti = self.subhuti.clone();
        let rules = rules::create_all_rules();

        subhuti.set_dispatch_rule(rules.dispatch_rule).await;
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "注册领域规则: dispatch=default",
            None,
        );
    }

    /// 设置执行规则
    pub async fn set_execution_rule(&self) {
        let subhuti = self.subhuti.clone();
        let rules = rules::create_all_rules();

        subhuti.set_execution_rule(rules.execution_rule).await;
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "注册领域规则: execution=default",
            None,
        );
    }

    /// 构建出站端口适配器集合（供组合根调用）
    ///
    /// 返回 3 个运行时出站端口适配器（ExpertRepositoryPort / OrchestrationEnginePort / SkillExecutionPort），
    /// 组合根用它们构造 OrchestrationService。
    pub fn build_adapters(&self) -> AppAdapters {
        let mut engine = SubhutiOrchestrationEngine::new(self.subhuti.clone());
        if let Some(ref observer) = *self.trace_observer.lock().unwrap() {
            engine = engine.with_trace_observer(observer.clone());
        }
        AppAdapters {
            expert_repository: Arc::new(SubhutiExpertRepository::new(self.subhuti.clone())),
            orchestration_engine: Arc::new(engine),
            skill_executor: Arc::new(SubhutiSkillExecutor::new(self.subhuti.clone())),
        }
    }
}
