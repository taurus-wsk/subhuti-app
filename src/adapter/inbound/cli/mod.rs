//! # CLI 适配器
//!
//! 命令行界面适配器，将用户命令映射到应用服务。
//!
//! ## 子命令
//!
//! - `serve` - 启动 HTTP 服务
//! - `doctor` - 环境诊断
//! - `api` - HTTP API 客户端（调用 agent）
//! - `log-stream` - 实时日志监控
//! - `mcp` - 启动 MCP server（stdio），供 WorkBuddy / MCP client 调用

pub mod api;
pub mod doctor;
pub mod log_stream;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "subhuti",
    version = "0.1.0",
    about = "Subhuti AI Agent Framework CLI Tool"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Serve {
        #[arg(long)]
        mock: bool,
        #[arg(long)]
        mock_file: Option<String>,
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        log_level: Option<String>,
        #[arg(long)]
        addr: Option<String>,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
    LogStream {
        #[arg(long, short)]
        trace_id: Option<String>,
        #[arg(long, short)]
        user_id: Option<String>,
        #[arg(long, short)]
        level: Option<String>,
        #[arg(long, short)]
        keyword: Option<String>,
        #[arg(long, default_value = "./logs")]
        log_dir: String,
        #[arg(long, default_value = "50")]
        tail: usize,
    },
    Api {
        #[command(subcommand)]
        subcommand: ApiCommands,
    },
    /// 启动 MCP server（stdio），供 WorkBuddy / 其他 MCP client 调用
    Mcp {
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        log_level: Option<String>,
        /// 并发工具调用上限（限流，防止本地 LLM 被同时打爆），默认 2
        #[arg(long)]
        concurrency: Option<usize>,
    },
}

#[derive(Subcommand)]
pub enum ApiCommands {
    Health,
    Experts,
    Trace {
        #[arg(long)]
        id: String,
    },
    Sessions,
    Orchestrate {
        #[arg(long, short)]
        message: String,
        #[arg(long, short)]
        user_id: Option<String>,
        #[arg(long)]
        chain: Option<String>,
    },
}
