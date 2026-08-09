use clap::Parser;
use subhuti_app::adapter::inbound::cli::{self, Commands};
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
            server::start_server(server::ServerOptions {
                mock,
                mock_file,
                debug,
                log_level,
                addr,
            })
            .await
        }
        Commands::Doctor { json } => cli::doctor::run(json),
        Commands::LogStream {
            trace_id,
            user_id,
            level,
            keyword,
            log_dir,
            tail,
        } => cli::log_stream::run(trace_id, user_id, level, keyword, log_dir, tail),
        Commands::Api { subcommand } => cli::api::run(subcommand).await,
    }
}
