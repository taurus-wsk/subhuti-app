//! Subhuti CLI 工具
//!
//! 使用方式:
//!   subhuti doctor     - 环境诊断
//!   subhuti help     - 帮助信息
//!   subhuti version  - 版本信息

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match command {
        "doctor" => run_doctor(),
        "version" => print_version(),
        "help" | "--help" | "-h" => print_help(),
        _ => {
            println!("未知命令: {}", command);
            println!();
            print_help();
            std::process::exit(1);
        }
    }
}

fn print_version() {
    println!("subhuti-cli v{}", env!("CARGO_PKG_VERSION"));
    println!("Subhuti AI Agent Framework CLI Tool");
}

fn print_help() {
    println!("Subhuti CLI - AI Agent 框架命令行工具");
    println!();
    println!("用法:");
    println!("  subhuti <command>");
    println!();
    println!("命令:");
    println!("  doctor     环境诊断 - 检查运行环境是否完备");
    println!("  version  显示版本信息");
    println!("  help     显示帮助信息");
    println!();
    println!("示例:");
    println!("  subhuti doctor");
    println!("  subhuti version");
}

fn run_doctor() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Subhuti Doctor - 环境诊断工具");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let mut results: Vec<CheckResult> = Vec::new();

    // 1. Rust 工具链
    println!("━━━ 基础工具链 ━━━");
    rustc_check(&mut results);
    cargo_check(&mut results);
    rustup_check(&mut results);
    print_category_results(&results, "基础工具链");

    println!();
    println!("━━━ 数据库 ━━━");
    docker_check(&mut results);
    postgres_check(&mut results);
    print_category_results(&results, "数据库");

    println!();
    println!("━━━ 向量模型 ━━━");
    ollama_check(&mut results);
    bge_m3_check(&mut results);
    print_category_results(&results, "向量模型");

    println!();
    println!("━━━ LLM 服务 ━━━");
    llm_provider_check(&mut results);
    print_category_results(&results, "LLM 服务");

    println!();
    println!("━━━ 项目配置 ━━━");
    cargo_toml_check(&mut results);
    source_code_check(&mut results);
    print_category_results(&results, "项目配置");

    // 统计
    let mut passed = 0;
    let mut failed = 0;
    let mut warnings = 0;
    let mut optional = 0;

    for r in &results {
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
        for r in &results {
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
        for r in &results {
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
        println!("   cargo run --bin http_server    # 启动 HTTP 服务");
        println!("   cargo test                   # 运行测试");
        println!("   cargo run --bin subhuti -- help # 查看 CLI 帮助");
    } else {
        println!("🔧 请先修复失败项，然后重新运行:");
        println!("   cargo run --bin subhuti -- doctor");
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

fn check_command(_name: &str, cmd: &str, args: &[&str]) -> Option<String> {
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
    match check_command("rustc", "rustc", &["--version"]) {
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
    match check_command("cargo", "cargo", &["--version"]) {
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
    match check_command("rustup", "rustup", &["--version"]) {
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
    match check_command("docker", "docker", &["--version"]) {
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
    match check_command("ollama", "ollama", &["--version"]) {
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
