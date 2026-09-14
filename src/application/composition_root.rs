//! # 组合根（Composition Root）
//!
//! 六边形架构的唯一组装点：创建所有依赖、接线、返回 Composition。
//!
//! 本模块是应用层中少数依赖出站适配层具体类型的模块（组合根特权）。
//! 它负责：
//! 1. 创建领域数据仓库（PostgreSQL / 内存降级）
//! 2. 创建框架初始化器（SubhutiFrameworkInitializer）
//! 3. 通过 SubhutiFrameworkInitializer 初始化框架、注册专家、设置规则
//! 4. 调用 build_adapters() 获取出站端口适配器
//! 5. 构造 OrchestrationService + 横切观察者，打包为 Composition 返回
//!
//! OrchestrationService 本身不再承担组装职责，只负责实现入站端口业务逻辑。
//! 观察者适配器也在此处统一创建，入站适配层不再直接依赖出站适配层具体类型。

use std::sync::Arc;

use crate::adapter::outbound::observer_adapters::{
    InMemoryTraceObserverAdapter, SqliteTraceObserverAdapter, SubhutiSessionObserverAdapter,
};
use crate::adapter::outbound::postgres_repository::{InMemoryRepository, PostgresRepository};
use crate::adapter::outbound::session_context_adapter::SqliteSessionContextAdapter;
use crate::adapter::outbound::subhuti_framework_initializer::SubhutiFrameworkInitializer;
use crate::application::memory_consolidation::MemoryConsolidator;
use crate::application::observer::{
    record_fn_log, LogLevel, SessionObserverPort, TraceObserverPort,
};
use crate::application::orchestration_service::OrchestrationService;
use crate::application::ports::{ChatPort, ExpertQueryPort, SkillPort};
use crate::application::session_manager::SessionManager;
use crate::application::trace_decorator::TraceAppService;
use crate::domain::experts;
use crate::domain::ports::{CommandPort, FileSystemPort, SessionContextPort, ToolchainPort};
use crate::domain::traits::DomainRepository;
use crate::infra::config::AppConfig;
use subhuti_infra::trace_store::{resolve_trace_db_path, SqliteTraceStore};

/// 组合根产物：应用服务 + 横切观察者
///
/// 入站适配层从此结构体取出所需依赖，不直接创建出站适配器。
pub struct Composition {
    /// 已装饰 trace 的入站端口（HTTP/测试/CLI 直接用，trace 自动生效）
    pub chat_port: Arc<dyn ChatPort>,
    pub expert_port: Arc<dyn ExpertQueryPort>,
    pub skill_port: Arc<dyn SkillPort>,
    /// 观察者（给 traces/sessions 查询路由用）
    pub trace_observer: Arc<dyn TraceObserverPort>,
    pub session_observer: Arc<dyn SessionObserverPort>,
    /// 知识库 PG 存储（可选，降级模式下为 None）
    /// 知识库后端：有 PG 走 PG，无 PG 走藏经阁的 SQLite 降级后端
    pub pg_storage: Option<Arc<dyn subhuti_infra::sutra_library::PersistencePort>>,
    /// 藏经阁记忆引擎（供 MCP 记忆工具 / HTTP 统计接口使用）
    pub sutra_library: Option<Arc<dyn subhuti_core::SutraLibraryPort>>,
}

/// 组合根：组装应用实例的唯一入口
pub struct CompositionRoot;

impl CompositionRoot {
    /// 从配置组装应用实例
    ///
    /// 执行流程：
    /// 1. 创建领域数据仓库（PG/内存降级）
    /// 2. 创建框架初始化器
    /// 3. 通过 SubhutiFrameworkInitializer 初始化框架
    /// 4. 注册领域专家
    /// 5. 设置规则（analysis / dispatch / execution）
    /// 7. 调用 build_adapters() 获取出站端口适配器
    /// 8. 构造 OrchestrationService + 观察者，打包为 Composition
    pub async fn build(app_config: &AppConfig) -> anyhow::Result<Composition> {
        // 0. 先创建框架初始化器（用于后续藏经阁引擎初始化）
        let app_config_arc = Arc::new(app_config.clone());
        let framework_initializer = Arc::new(SubhutiFrameworkInitializer::new(app_config_arc));

        // 0.05 框架级会话上下文：必须在注册专家**之前**装配，
        //      否则专家适配器拿不到管理者，历史注入与专家记忆回流都会失效。
        //      HTTP 进程与 MCP 进程共享同一个 SQLite 库，上下文因此跨进程、跨重启生效。
        let session_manager = Arc::new(SessionManager::new(
            match subhuti_infra::session_store::SqliteSessionStore::open(
                &subhuti_infra::session_store::default_session_db_path(),
            ) {
                Ok(s) => {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Info,
                        format!(
                            "✅ 框架级会话上下文已启用（持久化: {}）",
                            subhuti_infra::session_store::default_session_db_path()
                        ),
                        None,
                    );
                    // infra 具体类型在此被包成领域端口实现，上层只见 SessionContextPort
                    Some(Arc::new(SqliteSessionContextAdapter::new(Arc::new(s)))
                        as Arc<dyn SessionContextPort>)
                }
                Err(e) => {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Warn,
                        format!("会话上下文持久化不可用（{}），退化为进程内单轮", e),
                        None,
                    );
                    None
                }
            },
        ));
        framework_initializer.set_session_manager(session_manager.clone());

        // 0.1 统一数据目录：所有运行时落盘数据都在它下面，启动时打印绝对路径
        //     （可用 SUBHUTI_DATA_DIR 覆盖，默认 ~/.subhuti/data）
        match subhuti_infra::data_dir::ensure_data_dir() {
            Ok(dir) => record_fn_log(
                None,
                "",
                LogLevel::Info,
                format!("数据目录: {}（SUBHUTI_DATA_DIR 可覆盖）", dir.display()),
                None,
            ),
            Err(e) => record_fn_log(
                None,
                "",
                LogLevel::Warn,
                format!(
                    "数据目录创建失败: {}（{}）",
                    e,
                    subhuti_infra::data_dir::data_dir().display()
                ),
                None,
            ),
        }

        // 1. 创建领域数据仓库：
        //    - test_mode=true 用内存仓库
        //    - test_mode=false 优先 PostgreSQL，连接失败自动降级为内存仓库，保证 LLM 初始化等流程能继续
        let mut pg_pool_opt: Option<Arc<sqlx::PgPool>> = None;
        // PG 探活默认关闭：本机无 PG 时 `PgPool::connect` 会白等 5 秒才超时降级，
        // 这对 stdio MCP（每次被客户端拉起都要付）是不可接受的冷启动成本。
        // 需要 PG 时显式设置 SUBHUTI_PG_ENABLED=true 即可恢复原行为。
        let pg_enabled = std::env::var("SUBHUTI_PG_ENABLED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let repository: Arc<dyn DomainRepository> = if app_config.test_mode.enabled {
            record_fn_log(
                None,
                "",
                LogLevel::Info,
                "✅ 使用内存数据仓库（测试模式）",
                None,
            );
            Arc::new(InMemoryRepository::new())
        } else if !pg_enabled {
            record_fn_log(
                None,
                "",
                LogLevel::Info,
                "⏭️ 跳过 PostgreSQL 探活（SUBHUTI_PG_ENABLED 未开启），直接使用内存数据仓库",
                None,
            );
            Arc::new(InMemoryRepository::new())
        } else {
            let dsn = format!(
                "postgres://{}:{}@{}:{}/{}",
                app_config.database.username,
                app_config.database.password,
                app_config.database.host,
                app_config.database.port,
                app_config.database.database,
            );
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                sqlx::PgPool::connect(&dsn),
            )
            .await
            {
                Ok(Ok(pool)) => {
                    let pool = Arc::new(pool);
                    let postgres_repo = Arc::new(PostgresRepository::new(pool.clone()));
                    if let Err(e) = postgres_repo.init().await {
                        record_fn_log(
                            None,
                            "",
                            LogLevel::Warn,
                            format!("数据库表初始化失败: {}", e),
                            None,
                        );
                    }
                    pg_pool_opt = Some(pool.clone());
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Info,
                        "✅ 使用 PostgreSQL 数据仓库",
                        None,
                    );
                    postgres_repo
                }
                Ok(Err(e)) => {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Warn,
                        format!("PostgreSQL 连接失败（{}），降级为内存数据仓库", e),
                        None,
                    );
                    Arc::new(InMemoryRepository::new())
                }
                Err(_elapsed) => {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Warn,
                        "PostgreSQL 连接超时（5秒），降级为内存数据仓库",
                        None,
                    );
                    Arc::new(InMemoryRepository::new())
                }
            }
        };

        // 1.5 初始化藏经阁引擎 PG 持久化（如果 PG 连接成功）
        //
        // 注意：这里不再自己持有 PgStorage 作为知识库后端。
        // 有 PG 时 `init_sutra_library_pg` 会重建引擎（其 persistence 即 PG）；
        // 无 PG 时引擎降级为 SQLite。**两种情况下都从引擎取 persistence**，
        // 知识库 CRUD 因此不再强制依赖 PG。
        if let Some(pool) = pg_pool_opt {
            framework_initializer
                .init_sutra_library_pg((*pool).clone())
                .await;
        } else {
            record_fn_log(
                None,
                "",
                LogLevel::Info,
                "未检测到 PostgreSQL，藏经阁引擎将降级使用 SQLite 持久化（SUBHUTI_SUTRA_SQLITE 可自定义路径）",
                None,
            );
        }

        // 2. 通过 SubhutiFrameworkInitializer 初始化框架
        framework_initializer.init().await?;

        // 3.5 创建 Rust 工具链适配器
        let toolchain: Arc<dyn ToolchainPort> = Arc::new(
            crate::adapter::outbound::rust_toolchain_adapter::RustToolchainAdapter::new(
                std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            ),
        );

        // 3.6 创建文件系统和命令执行适配器
        let file_system: Arc<dyn FileSystemPort> =
            Arc::new(crate::adapter::outbound::file_system_adapter::LocalFileSystemAdapter::new());
        let command: Arc<dyn CommandPort> =
            Arc::new(crate::adapter::outbound::command_adapter::LocalCommandAdapter::new());

        // 4. 通过 SubhutiFrameworkInitializer 注册领域专家
        let domain_experts = experts::create_all_experts();
        for expert in &domain_experts {
            framework_initializer
                .register_expert(
                    expert.clone(),
                    repository.clone(),
                    Some(toolchain.clone()),
                    Some(file_system.clone()),
                    Some(command.clone()),
                )
                .await;
        }

        // 5. 规则层已精简：Dispatch/Execution 规则从未被 dispatch 主链路读取，已删除；
        //    任务分析不装默认关键词表实现（Orchestrator 内置 tags 打分派生，与路由同源零漂移）。
        //    如未来需要自定义分析规则：framework_initializer.subhuti().set_analysis_rule(...)

        // 6. 框架不再注册任何 Graph 编排（Workflow 已下沉为专家内部，由专家自选执行路径）

        // 7. 创建观察者适配器（必须先于 build_adapters，用于函数调用链路追踪注入引擎）
        //    优先使用共享 SQLite：HTTP 与 MCP 两进程写入同一文件，trace 自然汇聚，
        //    可在任一进程的 /traces 查到全部链路。SQLite 不可用（如只读文件系统）
        //    则降级为进程内内存存储（仅本进程可见）。
        let trace_db_path = resolve_trace_db_path();
        let trace_observer: Arc<dyn TraceObserverPort> =
            match SqliteTraceStore::open(&trace_db_path) {
                Ok(store) => {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Info,
                        format!("Trace 持久化已启用 (共享 SQLite): {}", trace_db_path),
                        None,
                    );
                    Arc::new(SqliteTraceObserverAdapter::new(Arc::new(store)))
                }
                Err(e) => {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Warn,
                        format!("Trace SQLite 打开失败，降级为内存存储: {}", e),
                        None,
                    );
                    Arc::new(InMemoryTraceObserverAdapter::new())
                }
            };
        let session_observer: Arc<dyn SessionObserverPort> =
            Arc::new(SubhutiSessionObserverAdapter::new());

        // 7a. 注入 trace_observer 到 SubhutiOrchestrationEngine（函数调用链路追踪）
        framework_initializer.set_trace_observer(trace_observer.clone());

        // 7b. 调用 build_adapters() 获取出站端口适配器
        let adapters = framework_initializer.build_adapters();

        // 8. 构造 OrchestrationService → 注入 trace_observer
        let app = Arc::new(
            OrchestrationService::new(
                adapters.expert_repository,
                adapters.orchestration_engine,
                adapters.skill_executor,
            )
            .with_trace_observer(trace_observer.clone())
            // 编排层与专家共用同一个框架级上下文管理者
            .with_session_manager(session_manager.clone())
            // 用于每次请求 per-request 订阅 ProgressEventBridge（SSE 阶段流）
            .with_event_bus(adapters.event_bus.clone())
            // 记忆闭环：会话结束后提炼事实 → 藏经阁沉淀 → 落库
            .with_memory_consolidator(Arc::new(MemoryConsolidator::new(
                adapters.sutra_library.clone(),
                adapters.llm.clone(),
            ))),
        );

        // 8c. 冷启动：把专家的静态知识灌进藏经阁（幂等，重复启动不会堆积）
        //
        // 此前这些知识只硬编码在提示词里，藏经阁检索不到，
        // 于是新装好的系统"记忆库是空的"，召回与命中率统计都无从谈起。
        //
        // ⚠️ 但对**只把藏经阁当记忆引擎**的客户端（如 WorkBuddy 仅需保存/搜索
        // 用户自己的记忆），这 4 条 Rust 静态知识属于污染：实测它们会混进召回结果
        // （虽然分数低，但白占 top_k 名额），且注入是「spawn + 固定 sleep 800ms」
        // 异步进行的，会在启动后约 1~2 秒内让 stats 与检索结果抖动。
        // 因此提供 `SUBHUTI_DISABLE_SEED=1` 关闭；**默认仍灌，行为不变**。
        tracing::info!(
            "🌱 冷启动检查: sutra_library={}",
            if adapters.sutra_library.is_some() {
                "已装配"
            } else {
                "None"
            }
        );
        let seed_disabled = std::env::var("SUBHUTI_DISABLE_SEED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if let Some(sutra) = adapters.sutra_library.clone() {
            if seed_disabled {
                tracing::info!("🌱 冷启动已关闭（SUBHUTI_DISABLE_SEED=1）：仅使用客户端写入的记忆");
            } else {
                tokio::task::spawn(async move {
                    // 等建表 + 回灌的后台任务先跑完，避免与 hydrate 抢写
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    let expert = crate::domain::experts::rust_expert::RustExpert::new();
                    let entries = expert.static_knowledge_entries();
                    let want = entries.len();
                    tracing::info!("🌱 冷启动开始: 待灌 {} 条", want);
                    let n = sutra.seed_knowledge("rust", &entries).await;
                    tracing::info!("🌱 冷启动完成: 新写入 {} 条", n);
                    if n > 0 {
                        record_fn_log(
                            None,
                            "",
                            LogLevel::Info,
                            format!("🌱 藏经阁冷启动：灌入 {} 条 Rust 静态知识", n),
                            None,
                        );
                    }
                });
            }
        }

        // 8b. 装饰器包装：一个 TraceAppService 实现 3 入站端口，trace 自动记录
        let traced = Arc::new(TraceAppService::new(
            app,
            trace_observer.clone(),
            session_observer.clone(),
        ));

        // 8d. 注册 TraceEventBridge：把框架 EventBus → TraceObserverPort.spans
        let bridge = Arc::new(
            crate::adapter::outbound::event_bridge::TraceEventBridge::new(trace_observer.clone()),
        );
        framework_initializer.register_event_handler(bridge).await;
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "✅ TraceEventBridge 已注册到框架 EventBus（细粒度 span 写入观察者 HashMap）",
            None,
        );

        // 8e. ProgressEventBridge 改为「per-request 订阅」：不再全局注册，
        //     而是每次 /orchestrate 请求开始时构造实例、订阅到 EventBus，
        //     请求结束 unsubscribe（见 orchestration_service.rs）。
        //     因此这里不再 register_event_handler，避免全局可变注册表与跨请求误投。

        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "CompositionRoot 构建完成: 规则已注册, 出站端口已装配, trace 装饰器已挂载",
            None,
        );

        Ok(Composition {
            chat_port: traced.clone(),
            expert_port: traced.clone(),
            skill_port: traced,
            trace_observer,
            session_observer,
            // 知识库后端从藏经阁引擎取：有 PG 即 PG，无 PG 即 SQLite 降级
            pg_storage: adapters.sutra_engine.as_ref().and_then(|e| e.persistence()),
            sutra_library: adapters.sutra_library.clone(),
        })
    }
}
