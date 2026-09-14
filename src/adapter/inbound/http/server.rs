use anyhow::Result;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::adapter::inbound::http::adapters::HttpAdapterFactory;
use crate::adapter::inbound::http::middleware::{RequestLogLayer, TraceIdLayer};
use crate::adapter::inbound::http::route_adapter::build_router;
use crate::application::observer::{record_fn_log, LogLevel};
use crate::application::CompositionRoot;
use crate::infra::config::AppConfig;

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub mock: bool,
    pub mock_file: Option<String>,
    pub debug: bool,
    pub log_level: Option<String>,
    pub addr: Option<String>,
}

pub async fn start_server(options: ServerOptions) -> Result<()> {
    record_fn_log(
        None,
        "",
        LogLevel::Info,
        "Starting Subhuti HTTP Server...",
        None,
    );
    record_fn_log(
        None,
        "",
        LogLevel::Info,
        "Log files will be written to ./logs/ directory",
        None,
    );

    if options.debug {
        record_fn_log(None, "", LogLevel::Info, "✅ Debug mode enabled", None);
    }
    if options.mock {
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            "✅ Mock mode enabled - using configured mock responses",
            None,
        );
    }

    dotenvy::dotenv().ok();

    let mut app_config = AppConfig::load().unwrap_or_else(|e| {
        eprintln!("⚠️  配置加载失败: {}", e);
        eprintln!("   使用默认配置");
        crate::infra::config::default_config()
    });

    if options.mock {
        app_config.test_mode.enabled = true;
        app_config.test_mode.mock_delay_ms = 0;
        if let Some(mock_file) = &options.mock_file {
            app_config.test_mode.mock_responses_path = mock_file.clone();
        }
    }

    if let Some(log_level) = &options.log_level {
        std::env::set_var("RUST_LOG", log_level);
        app_config.logging.level = log_level.clone();
    } else if options.debug {
        std::env::set_var("RUST_LOG", "debug");
        app_config.logging.level = "debug".to_string();
    }

    if let Some(addr) = &options.addr {
        app_config.http.addr = addr.clone();
    }

    let composition = CompositionRoot::build(&app_config).await?;

    // 端口已装饰 trace（组合根产出 traced 端口），直接注入工厂
    let factory = HttpAdapterFactory::new(
        composition.chat_port,
        composition.expert_port,
        composition.trace_observer.clone(),
        composition.session_observer.clone(),
        composition.pg_storage.clone(),
        composition.sutra_library.clone(),
    );

    // 创建 AppState（依赖注入容器）
    let app_state = factory.create_app_state();

    record_fn_log(None, "", LogLevel::Info, "✅ 六边形架构完成：2 窄端口（trace 装饰器自动记录）+ 2 观察者端口，路由采用 inventory 自动注册", None);

    // ── 方案 C：inventory 自动注册所有路由 ──
    // 所有路由通过 `inventory::submit!` 自注册，build_router 遍历收集后统一构建。
    // trace 由 TraceAppService 装饰器自动处理，不再需要 TraceSessionLayer。
    // 新增路由只需在任意文件添加 handler + inventory::submit!，无需修改 server.rs。
    let app = build_router()
        .nest_service("/subhuti/test", ServeDir::new("static"))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(RequestLogLayer)
        .layer(TraceIdLayer)
        .with_state(app_state);

    let addr = app_config.http.addr.clone();

    record_fn_log(
        None,
        "",
        LogLevel::Info,
        format!("Server listening on {}", addr),
        None,
    );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
