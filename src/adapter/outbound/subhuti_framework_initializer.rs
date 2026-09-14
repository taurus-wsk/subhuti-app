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
use subhuti_core::sutra_library::SutraLibraryPort;
use subhuti_core::LLMConfig;
use subhuti_core::LLMProvider;
use subhuti_infra::sutra_library::create_sutra_engine;
use subhuti_infra::CachedLLM;
use subhuti_infra::{
    ContextLimitLLM, DoubaoClient, DoubaoConfig, LimitConfig, MockLLM, OllamaClient, OllamaConfig,
    OpenAIClient, OpenAIConfig, RetryConfig, RetryLLM, ZhipuClient, ZhipuConfig,
};

use crate::adapter::outbound::subhuti_expert_repository::SubhutiExpertRepository;
use crate::adapter::outbound::subhuti_orchestration_engine::SubhutiOrchestrationEngine;
use crate::adapter::outbound::subhuti_skill_executor::SubhutiSkillExecutor;
use crate::application::observer::{record_fn_log, LogLevel, TraceObserverPort};
use crate::application::session_manager::SessionManager;
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
    /// 框架事件总线（供 OrchestrationService 在每次请求时 per-request 订阅进度桥）
    pub event_bus: Option<Arc<EventBus>>,
    /// 藏经阁记忆引擎（供 OrchestrationService 在会话结束时沉淀记忆）
    pub sutra_library: Option<Arc<dyn SutraLibraryPort>>,
    /// LLM（供记忆沉淀时提炼事实；与注入框架的同一实例）
    pub llm: Option<Arc<dyn subhuti_core::LLM>>,
    /// 藏经阁引擎具体类型（供取持久化后端）
    pub sutra_engine: Option<Arc<subhuti_infra::sutra_library::engine::MemoryEnginePort>>,
}

/// Subhuti 框架初始化器
///
/// 持有 Subhuti 框架对象，提供初始化与运行时适配器构建能力。
/// 所有框架相关的初始化逻辑都在这里处理。
pub struct SubhutiFrameworkInitializer {
    subhuti: Arc<Subhuti>,
    app_config: Arc<AppConfig>,
    trace_observer: Mutex<Option<Arc<dyn TraceObserverPort>>>,
    /// 框架级会话上下文管理者（可选）：装配后领域专家读写的是框架共享上下文
    /// （历史注入 + 专家记忆回流）
    session_manager: Mutex<Option<Arc<SessionManager>>>,
    /// 藏经阁引擎句柄：编排层会话结束沉淀要用，这里留一份引用
    /// （框架内部也持有同一 Arc，两处是同一个实例）
    sutra_library: Mutex<Option<Arc<dyn SutraLibraryPort>>>,
    /// LLM 句柄：沉淀前用 LLM 从对话里提炼值得长期记住的事实
    llm: Mutex<Option<Arc<dyn subhuti_core::LLM>>>,
    /// 藏经阁引擎的**具体类型**句柄：知识库 CRUD 等场景要直接拿它的持久化后端，
    /// trait 对象（`dyn SutraLibraryPort`）拿不到。
    sutra_engine: Mutex<Option<Arc<subhuti_infra::sutra_library::engine::MemoryEnginePort>>>,
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

        // 沉淀链路需要复用同一批句柄，先建好槽位，装配完成后回填
        let llm_slot: Mutex<Option<Arc<dyn subhuti_core::LLM>>> = Mutex::new(None);
        let sutra_slot: Mutex<Option<Arc<dyn SutraLibraryPort>>> = Mutex::new(None);
        let engine_slot: Mutex<
            Option<Arc<subhuti_infra::sutra_library::engine::MemoryEnginePort>>,
        > = Mutex::new(None);

        // 构建 Subhuti 配置
        let subhuti_llm_config = LLMConfig {
            model: app_config.llm.model.clone(),
            api_url: app_config.llm.api_url.clone(),
            api_key: api_key.clone(),
            temperature: app_config.llm.temperature as f32,
            max_tokens: app_config.llm.max_tokens,
        };

        let event_bus = Arc::new(EventBus::new(1024));

        let mut subhuti = Subhuti::new(event_bus.clone());

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

            subhuti.set_llm(llm_to_inject.clone());
            // 留一份给编排层（记忆沉淀要复用同一个 LLM 实例）
            *llm_slot.lock().unwrap() = Some(llm_to_inject);
        }

        // ── 初始化藏经阁引擎（SQLite 降级版；有 PG 时由 CompositionRoot 重建） ──
        let (sutra_engine, _sutra_skill) = create_sutra_engine(None);
        *engine_slot.lock().unwrap() = Some(sutra_engine.clone());
        let sutra_engine = sutra_engine as Arc<dyn SutraLibraryPort>;
        subhuti.set_sutra_library(sutra_engine.clone());
        *sutra_slot.lock().unwrap() = Some(sutra_engine);

        Self {
            subhuti: Arc::new(subhuti),
            app_config,
            trace_observer: Mutex::new(None),
            session_manager: Mutex::new(None),
            sutra_library: sutra_slot,
            llm: llm_slot,
            sutra_engine: engine_slot,
        }
    }

    /// 装配框架级会话上下文管理者（**必须在注册专家前调用**，否则适配器拿不到）
    pub fn set_session_manager(&self, manager: Arc<SessionManager>) {
        if let Ok(mut slot) = self.session_manager.lock() {
            *slot = Some(manager);
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
        *self.sutra_engine.lock().unwrap() = Some(sutra_engine.clone());
        let sutra_engine = sutra_engine as Arc<dyn SutraLibraryPort>;
        self.subhuti.set_sutra_library(sutra_engine.clone());
        // 同步更新编排层持有的句柄，否则沉淀仍打到无 PG 的旧引擎上
        *self.sutra_library.lock().unwrap() = Some(sutra_engine);
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
        > = {
            let base = crate::adapter::outbound::domain_expert_adapter::DomainExpertAdapter::new(
                expert,
                repository,
                toolchain,
                file_system,
                command,
                Some(event_bus),
            );
            // 注入框架级会话上下文管理者（若已装配）：专家据此读历史 + 回流记忆
            match self.session_manager.lock() {
                Ok(slot) => match slot.as_ref() {
                    Some(manager) => Arc::new(base.with_session_manager(manager.clone())),
                    None => Arc::new(base),
                },
                Err(_) => Arc::new(base),
            }
        };

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
            event_bus: Some(self.subhuti.event_bus().clone()),
            sutra_library: self.sutra_library.lock().unwrap().clone(),
            llm: self.llm.lock().unwrap().clone(),
            sutra_engine: self.sutra_engine.lock().unwrap().clone(),
        }
    }
}
