use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, Column, Row, ValueRef};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use subhuti_app::server;

#[derive(Parser)]
#[command(
    name = "subhuti",
    version = "0.1.0",
    about = "Subhuti AI Agent Framework CLI Tool"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve,
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
    Db {
        #[command(subcommand)]
        subcommand: DbCommands,
    },
    Api {
        #[command(subcommand)]
        subcommand: ApiCommands,
    },
    Flame {
        #[arg(long, default_value = "http")]
        output_format: String,
        #[arg(long)]
        open: bool,
    },
}

#[derive(Subcommand)]
enum DbCommands {
    Query {
        #[arg(long)]
        sql: Option<String>,
        #[arg(long)]
        file: Option<String>,
    },
    ListTables,
    Schema {
        #[arg(long)]
        table: String,
    },
    Stats,
}

#[derive(Subcommand)]
enum ApiCommands {
    Chat {
        #[arg(long, short)]
        message: String,
        #[arg(long, short)]
        user_id: Option<String>,
        #[arg(long, short)]
        session_id: Option<String>,
        #[arg(long, short)]
        skill: Option<String>,
    },
    Health,
    Skills,
    Experts,
    Persona,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => server::start_server().await,
        Commands::Doctor { json } => run_doctor(json),
        Commands::LogStream {
            trace_id,
            user_id,
            level,
            keyword,
            log_dir,
            tail,
        } => run_log_stream(trace_id, user_id, level, keyword, log_dir, tail),
        Commands::Db { subcommand } => run_db(subcommand).await,
        Commands::Api { subcommand } => run_api(subcommand).await,
        Commands::Flame {
            output_format,
            open,
        } => run_flame(output_format, open),
    }
}

fn run_doctor(json: bool) -> anyhow::Result<()> {
    let mut results: Vec<CheckResult> = Vec::new();

    rustc_check(&mut results);
    cargo_check(&mut results);
    rustup_check(&mut results);
    docker_check(&mut results);
    postgres_check(&mut results);
    ollama_check(&mut results);
    bge_m3_check(&mut results);
    llm_provider_check(&mut results);
    cargo_toml_check(&mut results);
    source_code_check(&mut results);

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        print_doctor_output(&results);
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckResult {
    name: String,
    status: CheckStatus,
    details: String,
    fix_hint: Option<String>,
    category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum CheckStatus {
    Pass,
    Fail,
    Warning,
    Optional,
}

impl CheckStatus {
    fn icon(&self) -> &str {
        match self {
            CheckStatus::Pass => "✅",
            CheckStatus::Fail => "❌",
            CheckStatus::Warning => "⚠️",
            CheckStatus::Optional => "🟡",
        }
    }
}

fn print_doctor_output(results: &[CheckResult]) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Subhuti Doctor - 环境诊断工具");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    println!("━━━ 基础工具链 ━━━");
    print_category_results(results, "基础工具链");

    println!();
    println!("━━━ 数据库 ━━━");
    print_category_results(results, "数据库");

    println!();
    println!("━━━ 向量模型 ━━━");
    print_category_results(results, "向量模型");

    println!();
    println!("━━━ LLM 服务 ━━━");
    print_category_results(results, "LLM 服务");

    println!();
    println!("━━━ 项目配置 ━━━");
    print_category_results(results, "项目配置");

    let mut passed = 0;
    let mut failed = 0;
    let mut warnings = 0;
    let mut optional = 0;

    for r in results {
        match r.status {
            CheckStatus::Pass => passed += 1,
            CheckStatus::Fail => failed += 1,
            CheckStatus::Warning => warnings += 1,
            CheckStatus::Optional => optional += 1,
        }
    }

    let total = results.len() as f32;
    let score = (passed as f32 + warnings as f32 * 0.5) / total * 100.0;

    println!();
    println!("══════════════════════════════════════════════════════════════");
    println!();
    println!("📊 诊断结果汇总");
    println!("───────────────────────────────────────────────────────────────");
    println!("  ✅ 通过:  {:>2} 项", passed);
    println!("  ❌ 失败:  {:>2} 项", failed);
    println!("  ⚠️  警告:  {:>2} 项", warnings);
    println!("  🟡 可选:  {:>2} 项", optional);
    println!("───────────────────────────────────────────────────────────────");
    println!("  🎯 环境就绪度: {:.1}%", score);
    println!();

    if failed > 0 {
        println!("❌ 有 {} 项检查失败，需要修复：", failed);
        println!();
        for r in results {
            if r.status == CheckStatus::Fail {
                println!("  ❌ {}", r.name);
                println!("     原因: {}", r.details);
                if let Some(hint) = &r.fix_hint {
                    println!("     💡 修复: {}", hint);
                }
                println!();
            }
        }
    } else if warnings > 0 {
        println!("⚠️  有 {} 项警告，建议优化：", warnings);
        println!();
        for r in results {
            if r.status == CheckStatus::Warning {
                println!("  ⚠️  {}", r.name);
                println!("     说明: {}", r.details);
                if let Some(hint) = &r.fix_hint {
                    println!("     💡 建议: {}", hint);
                }
                println!();
            }
        }
    }

    if failed == 0 {
        println!("✅ 核心环境检查通过！");
        println!();
        println!("🚀 快速开始:");
        println!("   subhuti serve              # 启动 HTTP 服务");
        println!("   subhuti doctor             # 环境诊断");
        println!("   subhuti api chat --message '你好' # 测试 API");
        println!("   subhuti log-stream         # 实时日志监控");
    } else {
        println!("🔧 请先修复失败项，然后重新运行:");
        println!("   subhuti doctor");
    }

    println!();
    println!("══════════════════════════════════════════════════════════════");
    println!();
}

fn print_category_results(results: &[CheckResult], category: &str) {
    for r in results.iter().filter(|r| r.category == category) {
        println!("  {}  {:25} {}", r.status.icon(), r.name, r.details);
    }
}

fn check_command(cmd: &str, args: &[&str]) -> Option<String> {
    match Command::new(cmd).args(args).output() {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Some(stdout)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

fn rustc_check(results: &mut Vec<CheckResult>) {
    match check_command("rustc", &["--version"]) {
        Some(version) => {
            results.push(CheckResult {
                name: "Rust 编译器".to_string(),
                status: CheckStatus::Pass,
                details: version,
                fix_hint: None,
                category: "基础工具链".to_string(),
            });
        }
        None => {
            results.push(CheckResult {
                name: "Rust 编译器".to_string(),
                status: CheckStatus::Fail,
                details: "未安装 rustc".to_string(),
                fix_hint: Some("请访问 https://rustup.rs/ 安装 Rust 工具链".to_string()),
                category: "基础工具链".to_string(),
            });
        }
    }
}

fn cargo_check(results: &mut Vec<CheckResult>) {
    match check_command("cargo", &["--version"]) {
        Some(version) => {
            results.push(CheckResult {
                name: "Cargo 包管理".to_string(),
                status: CheckStatus::Pass,
                details: version,
                fix_hint: None,
                category: "基础工具链".to_string(),
            });
        }
        None => {
            results.push(CheckResult {
                name: "Cargo 包管理".to_string(),
                status: CheckStatus::Fail,
                details: "未安装 cargo".to_string(),
                fix_hint: Some("Cargo 随 Rust 一起安装，请运行 rustup".to_string()),
                category: "基础工具链".to_string(),
            });
        }
    }
}

fn rustup_check(results: &mut Vec<CheckResult>) {
    match check_command("rustup", &["--version"]) {
        Some(version) => {
            results.push(CheckResult {
                name: "Rustup 工具链管理".to_string(),
                status: CheckStatus::Pass,
                details: version,
                fix_hint: None,
                category: "基础工具链".to_string(),
            });
        }
        None => {
            results.push(CheckResult {
                name: "Rustup 工具链管理".to_string(),
                status: CheckStatus::Warning,
                details: "未安装 rustup".to_string(),
                fix_hint: Some("推荐安装 rustup 来管理 Rust 版本: https://rustup.rs/".to_string()),
                category: "基础工具链".to_string(),
            });
        }
    }
}

fn docker_check(results: &mut Vec<CheckResult>) {
    match check_command("docker", &["--version"]) {
        Some(version) => {
            results.push(CheckResult {
                name: "Docker".to_string(),
                status: CheckStatus::Pass,
                details: version,
                fix_hint: None,
                category: "数据库".to_string(),
            });
        }
        None => {
            results.push(CheckResult {
                name: "Docker".to_string(),
                status: CheckStatus::Optional,
                details: "未安装 Docker".to_string(),
                fix_hint: Some("可选，用于运行 PostgreSQL 和 Ollama".to_string()),
                category: "数据库".to_string(),
            });
        }
    }
}

fn postgres_check(results: &mut Vec<CheckResult>) {
    let docker_running = Command::new("docker")
        .args(["ps", "-q", "--filter", "name=pgvector"])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);

    if docker_running {
        results.push(CheckResult {
            name: "PostgreSQL (pgvector)".to_string(),
            status: CheckStatus::Pass,
            details: "Docker 容器运行中".to_string(),
            fix_hint: None,
            category: "数据库".to_string(),
        });
    } else {
        let pg_is_local = Command::new("psql")
            .args(["--version"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if pg_is_local {
            results.push(CheckResult {
                name: "PostgreSQL".to_string(),
                status: CheckStatus::Warning,
                details: "本地安装了 PostgreSQL，但未确认 pgvector 扩展".to_string(),
                fix_hint: Some("请确认已安装 pgvector 扩展: CREATE EXTENSION vector;".to_string()),
                category: "数据库".to_string(),
            });
        } else {
            results.push(CheckResult {
                name: "PostgreSQL (pgvector)".to_string(),
                status: CheckStatus::Optional,
                details: "未检测到 PostgreSQL".to_string(),
                fix_hint: Some("可选，用于持久化记忆存储。使用内存模式可跳过此项。".to_string()),
                category: "数据库".to_string(),
            });
        }
    }
}

fn ollama_check(results: &mut Vec<CheckResult>) {
    match check_command("ollama", &["--version"]) {
        Some(version) => {
            results.push(CheckResult {
                name: "Ollama".to_string(),
                status: CheckStatus::Pass,
                details: version,
                fix_hint: None,
                category: "向量模型".to_string(),
            });
        }
        None => {
            results.push(CheckResult {
                name: "Ollama".to_string(),
                status: CheckStatus::Optional,
                details: "未安装 Ollama".to_string(),
                fix_hint: Some("可选，用于本地 LLM 和向量模型。使用云端 API 可跳过。".to_string()),
                category: "向量模型".to_string(),
            });
        }
    }
}

fn bge_m3_check(results: &mut Vec<CheckResult>) {
    let has_ollama = Command::new("ollama").arg("list").output().is_ok();
    if !has_ollama {
        results.push(CheckResult {
            name: "bge-m3 向量模型".to_string(),
            status: CheckStatus::Optional,
            details: "未检测到 Ollama，无法检查 bge-m3".to_string(),
            fix_hint: Some("先安装 Ollama，再运行: ollama pull bge-m3".to_string()),
            category: "向量模型".to_string(),
        });
        return;
    }

    match Command::new("ollama").arg("list").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("bge-m3") {
                results.push(CheckResult {
                    name: "bge-m3 向量模型".to_string(),
                    status: CheckStatus::Pass,
                    details: "已安装 bge-m3:latest".to_string(),
                    fix_hint: None,
                    category: "向量模型".to_string(),
                });
            } else {
                results.push(CheckResult {
                    name: "bge-m3 向量模型".to_string(),
                    status: CheckStatus::Warning,
                    details: "未安装 bge-m3 模型".to_string(),
                    fix_hint: Some("运行: ollama pull bge-m3".to_string()),
                    category: "向量模型".to_string(),
                });
            }
        }
        Err(_) => {
            results.push(CheckResult {
                name: "bge-m3 向量模型".to_string(),
                status: CheckStatus::Optional,
                details: "无法检查 Ollama 模型列表".to_string(),
                fix_hint: None,
                category: "向量模型".to_string(),
            });
        }
    }
}

fn llm_provider_check(results: &mut Vec<CheckResult>) {
    let doubao_key = std::env::var("DOUBAO_API_KEY").is_ok();
    let openai_key = std::env::var("OPENAI_API_KEY").is_ok();
    let ollama_base = std::env::var("OLLAMA_BASE_URL").is_ok();

    if doubao_key {
        results.push(CheckResult {
            name: "LLM 提供商".to_string(),
            status: CheckStatus::Pass,
            details: "已配置豆包 (Doubao) API Key".to_string(),
            fix_hint: None,
            category: "LLM 服务".to_string(),
        });
    } else if openai_key {
        results.push(CheckResult {
            name: "LLM 提供商".to_string(),
            status: CheckStatus::Pass,
            details: "已配置 OpenAI API Key".to_string(),
            fix_hint: None,
            category: "LLM 服务".to_string(),
        });
    } else if ollama_base {
        results.push(CheckResult {
            name: "LLM 提供商".to_string(),
            status: CheckStatus::Pass,
            details: "已配置 Ollama 本地模型".to_string(),
            fix_hint: None,
            category: "LLM 服务".to_string(),
        });
    } else {
        results.push(CheckResult {
            name: "LLM 提供商".to_string(),
            status: CheckStatus::Warning,
            details: "未配置 LLM API Key".to_string(),
            fix_hint: Some(
                "设置环境变量: \
                \n     豆包: export DOUBAO_API_KEY=your_key \
                \n     OpenAI: export OPENAI_API_KEY=your_key \
                \n     Ollama: export OLLAMA_BASE_URL=http://localhost:11434"
                    .to_string(),
            ),
            category: "LLM 服务".to_string(),
        });
    }
}

fn cargo_toml_check(results: &mut Vec<CheckResult>) {
    if Path::new("Cargo.toml").exists() {
        results.push(CheckResult {
            name: "项目配置".to_string(),
            status: CheckStatus::Pass,
            details: "Cargo.toml 存在".to_string(),
            fix_hint: None,
            category: "项目配置".to_string(),
        });
    } else {
        results.push(CheckResult {
            name: "项目配置".to_string(),
            status: CheckStatus::Fail,
            details: "未找到 Cargo.toml".to_string(),
            fix_hint: Some("请在项目根目录运行 subhuti doctor".to_string()),
            category: "项目配置".to_string(),
        });
    }
}

fn source_code_check(results: &mut Vec<CheckResult>) {
    let src_exists = Path::new("crates/subhuti/src/lib.rs").exists();
    let docs_exist = Path::new("docs").exists();

    if src_exists && docs_exist {
        results.push(CheckResult {
            name: "源代码与文档".to_string(),
            status: CheckStatus::Pass,
            details: "源代码和文档完整".to_string(),
            fix_hint: None,
            category: "项目配置".to_string(),
        });
    } else if src_exists {
        results.push(CheckResult {
            name: "源代码与文档".to_string(),
            status: CheckStatus::Warning,
            details: "源代码存在，文档目录缺失".to_string(),
            fix_hint: Some("建议查看 docs/ 目录获取完整文档".to_string()),
            category: "项目配置".to_string(),
        });
    } else {
        results.push(CheckResult {
            name: "源代码与文档".to_string(),
            status: CheckStatus::Fail,
            details: "未找到源代码".to_string(),
            fix_hint: Some("请在项目根目录运行 subhuti doctor".to_string()),
            category: "项目配置".to_string(),
        });
    }
}

fn run_log_stream(
    trace_id: Option<String>,
    user_id: Option<String>,
    level: Option<String>,
    keyword: Option<String>,
    log_dir: String,
    tail: usize,
) -> anyhow::Result<()> {
    println!("{}", "📡 实时日志流查看器 - 按 Ctrl+C 退出".yellow().bold());
    println!("───────────────────────────────────────────────────────────────");
    if let Some(tid) = &trace_id {
        println!("🔍 过滤 trace_id: {}", tid.cyan());
    }
    if let Some(uid) = &user_id {
        println!("🔍 过滤 user_id: {}", uid.cyan());
    }
    if let Some(lvl) = &level {
        println!("🔍 过滤 level: {}", lvl.cyan());
    }
    if let Some(kw) = &keyword {
        println!("🔍 关键词搜索: {}", kw.cyan());
    }
    println!();

    let log_dir = Path::new(&log_dir);
    if !log_dir.exists() {
        eprintln!("❌ 日志目录不存在: {}", log_dir.display());
        return Ok(());
    }

    let mut last_files: Vec<String> = Vec::new();
    let mut last_lines: HashMap<String, usize> = HashMap::new();

    loop {
        let entries = std::fs::read_dir(log_dir)?;
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("subhuti.log")
            })
            .collect();
        files.sort_by_key(|b| b.path().to_string_lossy().to_string());

        let mut current_files: Vec<String> = Vec::new();
        for entry in &files {
            let path = entry.path();
            let path_str = path.to_string_lossy().to_string();
            current_files.push(path_str.clone());

            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            let lines: Vec<_> = reader.lines().collect::<Result<_, _>>()?;
            let total_lines = lines.len();

            let last_line = *last_lines.get(&path_str).unwrap_or(&0);

            if total_lines > last_line {
                let start = if last_line == 0 && total_lines > tail {
                    total_lines - tail
                } else {
                    last_line
                };

                for line in &lines[start..] {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                        if !filter_log(&value, &trace_id, &user_id, &level, &keyword) {
                            continue;
                        }

                        let ts = value["timestamp"].as_str().unwrap_or("");
                        let lv = value["level"].as_str().unwrap_or("");
                        let tgt = value["target"].as_str().unwrap_or("");
                        let fields = value["fields"].clone();
                        let msg = fields["message"].as_str().unwrap_or("");

                        let level_color = match lv.to_uppercase().as_str() {
                            "ERROR" => "red",
                            "WARN" => "yellow",
                            "INFO" => "green",
                            "DEBUG" => "blue",
                            _ => "white",
                        };

                        println!(
                            "[{}] {} {} - {}",
                            ts.dimmed(),
                            format!("[{}]", lv).color(level_color),
                            tgt.purple(),
                            msg
                        );
                    }
                }
            }

            last_lines.insert(path_str, total_lines);
        }

        for file in &last_files {
            if !current_files.contains(file) {
                last_lines.remove(file);
            }
        }

        last_files = current_files;
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn filter_log(
    value: &serde_json::Value,
    trace_id: &Option<String>,
    user_id: &Option<String>,
    level: &Option<String>,
    keyword: &Option<String>,
) -> bool {
    let fields = value["fields"].clone();
    let lv = value["level"].as_str().unwrap_or("");
    let msg = fields["message"].as_str().unwrap_or("");

    if let Some(tid) = trace_id {
        let field_tid = fields["trace_id"].as_str().unwrap_or("");
        if !field_tid.contains(tid) {
            return false;
        }
    }

    if let Some(uid) = user_id {
        let field_uid = fields["user_id"].as_str().unwrap_or("");
        if !field_uid.contains(uid) {
            return false;
        }
    }

    if let Some(lvl) = level {
        if !lv.eq_ignore_ascii_case(lvl) {
            return false;
        }
    }

    if let Some(kw) = keyword {
        let kw_lower = kw.to_lowercase();
        let haystack = format!("{} {}", msg, fields).to_lowercase();
        if !haystack.contains(&kw_lower) {
            return false;
        }
    }

    true
}

async fn run_db(subcommand: DbCommands) -> anyhow::Result<()> {
    let config = load_db_config()?;
    let conn_str = format!(
        "postgres://{}:{}@{}:{}/{}",
        config.username, config.password, config.host, config.port, config.database
    );

    println!(
        "🔌 连接数据库: {}:{}/{}",
        config.host, config.port, config.database
    );

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&conn_str)
        .await?;

    match subcommand {
        DbCommands::Query { sql, file } => {
            let query = if let Some(sql) = sql {
                sql
            } else if let Some(file) = file {
                std::fs::read_to_string(file)?
            } else {
                eprintln!("❌ 请提供 --sql 或 --file 参数");
                return Ok(());
            };

            println!("───────────────────────────────────────────────────────────────");
            println!("{}", query.blue());
            println!("───────────────────────────────────────────────────────────────");

            let result = sqlx::query(&query).fetch_all(&pool).await?;

            if result.is_empty() {
                println!("{}", "✓ 查询完成，无结果".green());
            } else {
                println!("{}", format!("✓ 查询完成，共 {} 行", result.len()).green());
                println!();

                let first_row = result.first().unwrap();
                let columns = first_row.columns();
                let header: Vec<String> = columns.iter().map(|c| c.name().to_string()).collect();
                let col_count = columns.len();

                print!("| ");
                for col in &header {
                    print!("{} | ", col.bold());
                }
                println!();

                println!("|{}|", "-".repeat(header.len() * 20 + 1));

                for row in result {
                    print!("| ");
                    for i in 0..col_count {
                        let value = match row.try_get_raw(i) {
                            Ok(v) => v,
                            Err(_) => {
                                print!("ERROR | ");
                                continue;
                            }
                        };
                        let str_val = if value.is_null() {
                            "NULL".to_string()
                        } else {
                            String::from_utf8_lossy(value.as_bytes().unwrap_or(&[])).to_string()
                        };
                        let display_val = if str_val.len() > 50 {
                            format!("{}...", &str_val[..50])
                        } else {
                            str_val
                        };
                        print!("{} | ", display_val);
                    }
                    println!();
                }
            }
        }
        DbCommands::ListTables => {
            let tables = sqlx::query(
                "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
            )
            .fetch_all(&pool)
            .await?;

            println!("{}", "📋 数据库表列表".yellow().bold());
            println!("───────────────────────────────────────────────────────────────");
            for table in tables {
                let name: String = table.get(0);
                println!("  {}", name);
            }
        }
        DbCommands::Schema { table } => {
            let columns = sqlx::query(
                "SELECT column_name, data_type, is_nullable FROM information_schema.columns WHERE table_name = $1"
            )
            .bind(&table)
            .fetch_all(&pool)
            .await?;

            println!("{}", format!("📋 表 {} 的结构", table).yellow().bold());
            println!("───────────────────────────────────────────────────────────────");
            println!(
                "| {} | {} | {} |",
                "字段名".bold(),
                "类型".bold(),
                "可空".bold()
            );
            println!("|{}|", "-".repeat(60));

            for col in columns {
                let column_name: String = col.get(0);
                let data_type: String = col.get(1);
                let is_nullable: String = col.get(2);
                let nullable = if is_nullable == "YES" { "✓" } else { "✗" };
                println!("| {} | {} | {} |", column_name, data_type, nullable);
            }
        }
        DbCommands::Stats => {
            let stats = sqlx::query(
                "SELECT table_name, table_rows FROM information_schema.tables WHERE table_schema = 'public'"
            )
            .fetch_all(&pool)
            .await?;

            println!("{}", "📊 数据库统计信息".yellow().bold());
            println!("───────────────────────────────────────────────────────────────");
            println!("| {} | {} |", "表名".bold(), "行数".bold());
            println!("|{}|", "-".repeat(40));

            for stat in stats {
                let table_name: String = stat.get(0);
                let table_rows: i64 = stat.get(1);
                println!("| {} | {} |", table_name, table_rows);
            }
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct DbConfig {
    host: String,
    port: u16,
    database: String,
    username: String,
    password: String,
}

fn load_db_config() -> anyhow::Result<DbConfig> {
    let config_path = Path::new("config/Subhuti.toml");
    if !config_path.exists() {
        return Ok(DbConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            username: "postgres".to_string(),
            password: "123456".to_string(),
        });
    }

    let content = std::fs::read_to_string(config_path)?;
    let config: toml::Value = toml::from_str(&content)?;

    Ok(DbConfig {
        host: config["database"]["host"]
            .as_str()
            .unwrap_or("localhost")
            .to_string(),
        port: config["database"]["port"].as_integer().unwrap_or(5432) as u16,
        database: config["database"]["database"]
            .as_str()
            .unwrap_or("postgres")
            .to_string(),
        username: config["database"]["username"]
            .as_str()
            .unwrap_or("postgres")
            .to_string(),
        password: config["database"]["password"]
            .as_str()
            .unwrap_or("123456")
            .to_string(),
    })
}

async fn run_api(subcommand: ApiCommands) -> anyhow::Result<()> {
    let base_url = "http://localhost:8080/subhuti/api/v1";
    let client = reqwest::Client::new();

    println!("🌐 API 测试客户端 - {}", base_url);
    println!("───────────────────────────────────────────────────────────────");

    match subcommand {
        ApiCommands::Chat {
            message,
            user_id,
            session_id,
            skill,
        } => {
            let mut body: HashMap<String, serde_json::Value> = HashMap::new();
            body.insert("message".to_string(), serde_json::Value::String(message));
            if let Some(uid) = user_id {
                body.insert("user_id".to_string(), serde_json::Value::String(uid));
            }
            if let Some(sid) = session_id {
                body.insert("session_id".to_string(), serde_json::Value::String(sid));
            }
            if let Some(sk) = skill {
                body.insert("skill".to_string(), serde_json::Value::String(sk));
            }

            let response = client
                .post(format!("{}/chat", base_url))
                .json(&body)
                .send()
                .await?;

            print_response(response).await?;
        }
        ApiCommands::Health => {
            let response = client.get(format!("{}/health", base_url)).send().await?;
            print_response(response).await?;
        }
        ApiCommands::Skills => {
            let response = client.get(format!("{}/skills", base_url)).send().await?;
            print_response(response).await?;
        }
        ApiCommands::Experts => {
            let response = client.get(format!("{}/experts", base_url)).send().await?;
            print_response(response).await?;
        }
        ApiCommands::Persona => {
            let response = client.get(format!("{}/persona", base_url)).send().await?;
            print_response(response).await?;
        }
        ApiCommands::Trace { id } => {
            let response = client
                .get(format!("{}/traces/{}", base_url, id))
                .send()
                .await?;
            print_response(response).await?;
        }
        ApiCommands::Sessions => {
            let response = client.get(format!("{}/sessions", base_url)).send().await?;
            print_response(response).await?;
        }
        ApiCommands::Orchestrate {
            message,
            user_id,
            chain,
        } => {
            let mut body: HashMap<String, serde_json::Value> = HashMap::new();
            body.insert("message".to_string(), serde_json::Value::String(message));
            if let Some(uid) = user_id {
                body.insert("user_id".to_string(), serde_json::Value::String(uid));
            }
            if let Some(ch) = chain {
                body.insert("chain".to_string(), serde_json::Value::String(ch));
            }

            let response = client
                .post(format!("{}/orchestrate", base_url))
                .json(&body)
                .send()
                .await?;

            print_response(response).await?;
        }
    }

    Ok(())
}

async fn print_response(response: reqwest::Response) -> anyhow::Result<()> {
    let status = response.status();
    let body = response.text().await?;

    println!("{}", format!("Status: {}", status).bold());

    if status.is_success() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            println!("{}", "Response:".green());
            println!("{}", serde_json::to_string_pretty(&json)?);
        } else {
            println!("{}", body);
        }
    } else {
        eprintln!("{}", format!("Error: {}", body).red());
    }

    Ok(())
}

fn run_flame(output_format: String, open: bool) -> anyhow::Result<()> {
    println!("{}", "🔥 性能火焰图生成器".yellow().bold());
    println!("───────────────────────────────────────────────────────────────");

    let flame_dir = Path::new("./flamegraphs");
    if !flame_dir.exists() {
        std::fs::create_dir_all(flame_dir)?;
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let output_file = flame_dir.join(format!("flame_{}.{}", timestamp, output_format));

    println!("📝 输出文件: {}", output_file.display());

    let mut cmd = Command::new("cargo");
    let result = cmd
        .args([
            "run",
            "--bin",
            "subhuti",
            "--",
            "serve",
            "--tracing-flame",
            output_file.to_str().unwrap(),
        ])
        .spawn();

    match result {
        Ok(mut child) => {
            println!("🚀 服务已启动，开始收集性能数据... (按 Ctrl+C 停止)");
            let _ = child.wait();
            println!("{}", "✓ 数据收集完成".green());
        }
        Err(e) => {
            eprintln!(
                "{}",
                format!("❌ 启动失败: {}。请确保 tracing-flame 已安装。", e).red()
            );
            println!("💡 安装方法: cargo install tracing-flame");
        }
    }

    if open && output_file.exists() {
        println!("🌐 打开火焰图...");
        let _ = Command::new("open").arg(&output_file).status();
    }

    Ok(())
}
