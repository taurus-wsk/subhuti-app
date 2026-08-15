//! # 组合根（Composition Root）
//!
//! 六边形架构的唯一组装点：创建所有依赖、接线、返回 Composition。
//!
//! 本模块是应用层中少数依赖出站适配层具体类型的模块（组合根特权）。
//! 它负责：
//! 1. 创建领域数据仓库（PostgreSQL / 内存降级）
//! 2. 创建框架初始化器（SubhutiFrameworkInitializer）
//! 3. 通过 SubhutiFrameworkInitializer 初始化框架、注册专家、设置规则、注册图
//! 4. 调用 build_adapters() 获取出站端口适配器
//! 5. 构造 OrchestrationService + 横切观察者，打包为 Composition 返回
//!
//! OrchestrationService 本身不再承担组装职责，只负责实现入站端口业务逻辑。
//! 观察者适配器也在此处统一创建，入站适配层不再直接依赖出站适配层具体类型。

use std::sync::Arc;

use crate::adapter::outbound::observer_adapters::{
    SubhutiSessionObserverAdapter, SubhutiTraceObserverAdapter,
};
use crate::adapter::outbound::postgres_repository::{InMemoryRepository, PostgresRepository};
use crate::adapter::outbound::subhuti_framework_initializer::SubhutiFrameworkInitializer;
use crate::application::observer::{
    record_fn_log, LogLevel, SessionObserverPort, TraceObserverPort,
};
use crate::application::orchestration_service::OrchestrationService;
use crate::application::ports::{ChatPort, ExpertQueryPort, SkillPort};
use crate::application::trace_decorator::TraceAppService;
use crate::domain::experts;
use crate::domain::ports::ToolchainPort;
use crate::domain::traits::DomainRepository;
use crate::infra::config::AppConfig;

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
    /// 6. 注册图编排
    /// 7. 调用 build_adapters() 获取出站端口适配器
    /// 8. 构造 OrchestrationService + 观察者，打包为 Composition
    pub async fn build(app_config: &AppConfig) -> anyhow::Result<Composition> {
        // 1. 创建领域数据仓库：
        //    - test_mode=true 用内存仓库
        //    - test_mode=false 优先 PostgreSQL，连接失败自动降级为内存仓库，保证 LLM 初始化等流程能继续
        let repository: Arc<dyn DomainRepository> = if app_config.test_mode.enabled {
            record_fn_log(
                None,
                "",
                LogLevel::Info,
                "✅ 使用内存数据仓库（测试模式）",
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

        // 2. 创建框架初始化器（出站适配层，持有 Subhuti 对象）
        let app_config_arc = Arc::new(app_config.clone());
        let framework_initializer = Arc::new(SubhutiFrameworkInitializer::new(app_config_arc));

        // 3. 通过 SubhutiFrameworkInitializer 初始化框架
        framework_initializer.init().await?;

        // 3.5 创建 Rust 工具链适配器（供 RustExpert 等编译验证专家使用）
        let toolchain: Arc<dyn ToolchainPort> = Arc::new(
            crate::adapter::outbound::rust_toolchain_adapter::RustToolchainAdapter::new(
                std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            ),
        );

        // 4. 通过 SubhutiFrameworkInitializer 注册领域专家
        let domain_experts = experts::create_all_experts();
        for expert in &domain_experts {
            framework_initializer
                .register_expert(expert.clone(), repository.clone(), Some(toolchain.clone()))
                .await;
        }

        // 5. 通过 SubhutiFrameworkInitializer 设置规则
        framework_initializer.set_analysis_rule().await;
        framework_initializer.set_dispatch_rule().await;
        framework_initializer.set_execution_rule().await;
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "注册领域规则: analysis=default, dispatch=default, execution=default",
            None,
        );

        // 6. 通过 SubhutiFrameworkInitializer 注册所有图编排
        framework_initializer.register_all_graphs().await;

        // 7. 创建观察者适配器（必须先于 build_adapters，用于函数调用链路追踪注入引擎）
        let trace_observer: Arc<dyn TraceObserverPort> =
            Arc::new(SubhutiTraceObserverAdapter::new());
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
            .with_trace_observer(trace_observer.clone()),
        );

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
        })
    }
}
