use clap::Parser;
use subhuti_app::adapter::inbound::cli::{self, Commands};
use subhuti_app::adapter::inbound::http::middleware;
use subhuti_app::adapter::inbound::http::server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        Commands::Serve {
            mock,
            mock_file,
            debug,
            log_level,
            addr,
        } => {
            // serve 的 --debug / --log-level 直接决定日志级别
            let level = if debug {
                Some("debug")
            } else {
                log_level.as_deref()
            };
            let _log_guard = middleware::init_logging(level);
            server::start_server(server::ServerOptions {
                mock,
                mock_file,
                debug,
                log_level,
                addr,
            })
            .await
        }
        Commands::Doctor { json } => {
            let _log_guard = middleware::init_logging(None);
            cli::doctor::run(json)
        }
        Commands::LogStream {
            trace_id,
            user_id,
            level,
            keyword,
            log_dir,
            tail,
        } => {
            let _log_guard = middleware::init_logging(None);
            cli::log_stream::run(trace_id, user_id, level, keyword, log_dir, tail)
        }
        Commands::Api { subcommand } => {
            let _log_guard = middleware::init_logging(None);
            cli::api::run(subcommand).await
        }
        Commands::Mcp {
            debug,
            log_level,
            concurrency,
        } => {
            // 不调用 init_logging：MCP 的 stdout 必须是纯净的 JSON-RPC 通道，
            // 日志由 mcp::run 内部配置到 stderr / 文件。
            subhuti_app::adapter::inbound::mcp::run(debug, log_level, concurrency).await
        }
    }
}
