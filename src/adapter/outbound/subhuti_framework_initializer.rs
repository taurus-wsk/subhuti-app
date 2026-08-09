//! # Subhuti 框架初始化器
//!
//! 框架初始化器：由组合根（CompositionRoot）直接使用，
//! 将应用层的初始化请求转换为 Subhuti 框架的具体操作。
//!
//! 设计说明：
//! - 初始化逻辑与具体框架（Subhuti）强绑定，无需多态，故不抽象为 trait
//! - 组合根作为唯一组装点，允许依赖具体类型（组合根特权）
//! - 运行时出站端口适配器由 build_adapters() 提供，供组合根构造 AppService

use std::sync::Arc;

use subhuti::{
    CachedLLM, DbConfig, DoubaoClient, LLMConfig, LLMProvider, MemoryConfig, OllamaClient,
    OpenAIClient, RuntimeConfig, Subhuti, SubhutiConfig, ZhipuClient,
};

use crate::adapter::outbound::graphs;
use crate::adapter::outbound::rules;
use crate::adapter::outbound::subhuti_expert_repository::SubhutiExpertRepository;
use crate::adapter::outbound::subhuti_orchestration_engine::SubhutiOrchestrationEngine;
use crate::adapter::outbound::subhuti_skill_executor::SubhutiSkillExecutor;
use crate::domain::ports::{ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort};
use crate::domain::traits::{DomainExpert, DomainRepository};
use crate::infra::config::AppConfig;

/// 出站端口适配器集合（由 SubhutiFrameworkInitializer 构建，供组合根使用）
///
/// 组合根（CompositionRoot）调用 `SubhutiFrameworkInitializer::build_adapters()` 获取此结构体，
/// 用于构造 AppService。初始化与运行时适配器创建都由具体类型直接提供。
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

        let config = SubhutiConfig {
            llm: subhuti_llm_config.clone(),
            provider: if app_config.test_mode.enabled {
                LLMProvider::Custom
            } else {
                provider
            },
            runtime: RuntimeConfig::default(),
            memory: MemoryConfig::default(),
            flow: subhuti::flow::FlowConfig::default(),
            db: None,
        };

        let mut subhuti = Subhuti::with_config(config);

        // 初始化事件总线内置处理器
        let _ = subhuti.event_bus().init_builtin_handlers();
        tracing::info!("✅ EventBus initialized with builtin handlers");

        // 2) 根据 test_mode + provider 构建并注入 LLM client
        if app_config.test_mode.enabled {
            let mock_client = subhuti::runtime::llm::MockLlmClient::new();
            subhuti.set_llm(Arc::new(mock_client));
            tracing::info!("✅ 测试模式已启用，使用 Mock LLM");
        } else {
            // 按 provider 构建真实 LLM client
            let llm_client: Arc<dyn subhuti::runtime::LLM> = match provider {
                LLMProvider::OpenAI => {
                    let cfg = subhuti::OpenAIConfig {
                        api_key: api_key.unwrap_or_default(),
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    tracing::info!("✅ LLM: OpenAI ({})", cfg.model);
                    Arc::new(OpenAIClient::new(cfg))
                }
                LLMProvider::Ollama => {
                    let cfg = subhuti::OllamaConfig {
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    tracing::info!("✅ LLM: Ollama ({})", cfg.model);
                    Arc::new(OllamaClient::new(cfg))
                }
                LLMProvider::Doubao => {
                    let cfg = subhuti::DoubaoConfig {
                        api_key: api_key.unwrap_or_default(),
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    tracing::info!("✅ LLM: Doubao ({})", cfg.model);
                    Arc::new(DoubaoClient::new(cfg))
                }
                LLMProvider::Zhipu => {
                    let cfg = subhuti::ZhipuConfig {
                        api_key: api_key.unwrap_or_default(),
                        api_url: subhuti_llm_config.api_url.clone(),
                        model: subhuti_llm_config.model.clone(),
                        temperature: subhuti_llm_config.temperature,
                        max_tokens: subhuti_llm_config.max_tokens,
                    };
                    tracing::info!("✅ LLM: Zhipu/智谱 ({})", cfg.model);
                    Arc::new(ZhipuClient::new(cfg))
                }
                LLMProvider::Custom => {
                    tracing::warn!("⚠️  LLM provider = custom，未注入任何真实 client");
                    Arc::new(subhuti::runtime::llm::MockLlmClient::new())
                }
            };

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

            let llm_to_inject: Arc<dyn subhuti::runtime::LLM> = if cache_enabled_debug {
                let cache_path = std::env::var("SUBHUTI_LLM_CACHE_PATH")
                    .unwrap_or_else(|_| ".llm-cache.json".to_string());
                tracing::info!(
                    "🧪 LLM 调试缓存已开启：path={}, max_entries=100（相同输入复用上次结果）",
                    cache_path
                );
                CachedLLM::wrap(llm_client, cache_path)
            } else {
                // release 构建但环境变量仍为 1 时，显式 warn 提醒
                if cfg!(not(debug_assertions))
                    && std::env::var("SUBHUTI_LLM_CACHE")
                        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                        .unwrap_or(false)
                {
                    tracing::warn!(
                        "⚠️  检测到 SUBHUTI_LLM_CACHE=1，但当前是 release build，已强制禁用调试缓存"
                    );
                }
                llm_client
            };

            subhuti.set_llm(llm_to_inject);
        }

        Self {
            subhuti: Arc::new(subhuti),
            app_config,
        }
    }

    /// 初始化框架（创建框架实例、配置 LLM、数据库等）
    pub async fn init(&self) -> anyhow::Result<()> {
        let subhuti = self.subhuti.clone();
        let db_config = DbConfig {
            host: self.app_config.database.host.clone(),
            port: self.app_config.database.port,
            database: self.app_config.database.database.clone(),
            username: self.app_config.database.username.clone(),
            password: self.app_config.database.password.clone(),
            max_connections: self.app_config.database.max_connections,
        };
        let is_test_mode = self.app_config.test_mode.enabled;

        // ── 初始化数据库（异步） ──
        match subhuti.init_database(&db_config).await {
            Ok(_) => tracing::info!("Database initialized successfully"),
            Err(e) => tracing::warn!("Database initialization failed (using file storage): {}", e),
        }

        // ── LLM 健康检查（真实 provider 才会打网络） ──
        if !is_test_mode {
            if let Some(llm) = subhuti.current_llm() {
                let provider = subhuti::runtime::LLM::provider(&*llm);
                let cfg = subhuti::runtime::LLM::config(&*llm).clone();
                match subhuti::runtime::LLM::health_check(&*llm).await {
                    Ok(true) => tracing::info!(
                        "✅ LLM 健康检查通过：provider={:?}, model={}",
                        provider,
                        cfg.model
                    ),
                    Ok(false) => tracing::warn!(
                        "⚠️  LLM 健康检查返回 false：provider={:?}, model={}, api_url={}；请检查 API Key / 网络",
                        provider,
                        cfg.model,
                        cfg.api_url
                    ),
                    Err(e) => tracing::warn!(
                        "⚠️  LLM 健康检查调用失败：provider={:?}, model={}, error={}",
                        provider,
                        cfg.model,
                        e
                    ),
                }
            } else {
                tracing::warn!("⚠️  Subhuti 尚未注入 LLM 实例，跳过健康检查");
            }
        }

        // ── 同步插件专家到 Orchestrator ──
        subhuti.sync_experts_to_orchestrator().await;
        Ok(())
    }

    /// 注册事件处理器到框架 EventBus
    ///
    /// 用于 TraceEventBridge 等项目侧 handler。
    pub async fn register_event_handler(&self, handler: Arc<dyn subhuti::event::EventHandler>) {
        self.subhuti.event_bus().subscribe(handler).await;
    }

    /// 注册领域专家到框架
    pub async fn register_expert(
        &self,
        expert: Arc<dyn DomainExpert>,
        repository: Arc<dyn DomainRepository>,
    ) {
        let subhuti = self.subhuti.clone();
        let expert_name = expert.name().to_string();

        // 创建领域专家适配器（领域→框架）
        let adapter: Arc<
            crate::adapter::outbound::domain_expert_adapter::DomainExpertAdapter<dyn DomainExpert>,
        > = Arc::new(
            crate::adapter::outbound::domain_expert_adapter::DomainExpertAdapter::new(
                expert, repository,
            ),
        );

        // 注册到框架的 Orchestrator
        subhuti.register_orchestrator_expert(adapter).await;
        tracing::info!("注册领域专家: {}", expert_name);
    }

    /// 注册图编排流程
    pub async fn register_graph(&self, graph_name: &str) {
        let subhuti = self.subhuti.clone();
        let graph_name = graph_name.to_string();

        // ✅ 获取真实已注册的 Agent trait 对象（不再是 Value 快照，也不再伪造 BlenderExpert）
        //
        // 之前的问题：
        //   list_orchestrator_experts() 返回 Vec<Value> 元数据快照，
        //   无法拿到真实 Arc<dyn ExpertAgent>，于是代码注释说"跳过图注册"
        //   然后给每个节点套一个"新建 BlenderExpert"的假对象。
        //
        // 现在：
        //   新增的 get_orchestrator_agents() 返回真实 Arc<dyn ExpertAgent> 列表，
        //   Graph 节点可以直接调用 .run() 进行真正的编排。
        let framework_agents = subhuti.get_orchestrator_agents().await;

        // 构建专家状态（共享依赖：LLM、Memory、EventBus 等）
        let expert_state = subhuti.build_expert_state();

        // 创建并注册目标图
        let graphs = graphs::create_all_graphs(&framework_agents, &expert_state);
        for graph in graphs {
            if graph.name() == graph_name {
                subhuti.register_graph(graph).await;
                tracing::info!(
                    "注册图编排: {} (挂载真实专家 {} 个)",
                    graph_name,
                    framework_agents.len()
                );
                break;
            }
        }
    }

    /// 设置任务分析规则
    pub async fn set_analysis_rule(&self) {
        let subhuti = self.subhuti.clone();
        let rules = rules::create_all_rules();

        subhuti.set_analysis_rule(rules.analysis_rule).await;
        tracing::info!("注册领域规则: analysis=default");
    }

    /// 设置调度规则
    pub async fn set_dispatch_rule(&self) {
        let subhuti = self.subhuti.clone();
        let rules = rules::create_all_rules();

        subhuti.set_dispatch_rule(rules.dispatch_rule).await;
        tracing::info!("注册领域规则: dispatch=default");
    }

    /// 设置执行规则
    pub async fn set_execution_rule(&self) {
        let subhuti = self.subhuti.clone();
        let rules = rules::create_all_rules();

        subhuti.set_execution_rule(rules.execution_rule).await;
        tracing::info!("注册领域规则: execution=default");
    }

    /// 构建出站端口适配器集合（供组合根调用）
    ///
    /// 返回 3 个运行时出站端口适配器（ExpertRepositoryPort / OrchestrationEnginePort / SkillExecutionPort），
    /// 组合根用它们构造 AppService。
    pub fn build_adapters(&self) -> AppAdapters {
        AppAdapters {
            expert_repository: Arc::new(SubhutiExpertRepository::new(self.subhuti.clone())),
            orchestration_engine: Arc::new(SubhutiOrchestrationEngine::new(self.subhuti.clone())),
            skill_executor: Arc::new(SubhutiSkillExecutor::new(self.subhuti.clone())),
        }
    }
}
