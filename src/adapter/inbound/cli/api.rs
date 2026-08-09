use colored::Colorize;
use reqwest::Client;
use serde_json::Value;

use super::ApiCommands;

const DEFAULT_BASE_URL: &str = "http://localhost:8080";

fn handle_connection_error(e: &reqwest::Error) -> bool {
    if e.is_connect() {
        eprintln!("{}", "❌ 连接失败：服务器未启动或端口不可达".red());
        eprintln!("   💡 请先在另一个终端运行:");
        eprintln!("      {}", "subhuti serve".yellow());
        eprintln!("   或设置环境变量指定服务器地址:");
        eprintln!(
            "      {}",
            "export SUBHUTI_API_URL=http://your-server:8080".yellow()
        );
        true
    } else {
        false
    }
}

pub async fn run(subcommand: ApiCommands) -> anyhow::Result<()> {
    let base_url =
        std::env::var("SUBHUTI_API_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let client = Client::new();

    match subcommand {
        ApiCommands::Health => {
            let resp = match client
                .get(format!("{}/subhuti/api/v1/health", base_url))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if handle_connection_error(&e) {
                        return Ok(());
                    }
                    return Err(e.into());
                }
            };
            let result: Value = resp.json().await?;
            println!("{}", "📊 健康检查".yellow().bold());
            println!("───────────────────────────────────────────────────────────────");
            println!(
                "状态: {}",
                if result["status"].as_str() == Some("ok") {
                    "✅ 正常".green()
                } else {
                    "❌ 异常".red()
                }
            );
            println!("版本: {}", result["version"].as_str().unwrap_or("unknown"));
        }
        ApiCommands::Skills => {
            let resp = match client
                .get(format!("{}/subhuti/api/v1/skills", base_url))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if handle_connection_error(&e) {
                        return Ok(());
                    }
                    return Err(e.into());
                }
            };
            let result: Value = resp.json().await?;
            println!("{}", "📚 技能列表".yellow().bold());
            println!("───────────────────────────────────────────────────────────────");
            for skill in result["skills"].as_array().unwrap_or(&vec![]) {
                println!(
                    "  {} - {}",
                    skill["id"].as_str().unwrap_or(""),
                    skill["description"].as_str().unwrap_or("")
                );
            }
        }
        ApiCommands::Experts => {
            let resp = match client
                .get(format!("{}/subhuti/api/v1/experts", base_url))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if handle_connection_error(&e) {
                        return Ok(());
                    }
                    return Err(e.into());
                }
            };
            let result: Value = resp.json().await?;
            println!("{}", "👥 专家列表".yellow().bold());
            println!("───────────────────────────────────────────────────────────────");
            for expert in result["experts"].as_array().unwrap_or(&vec![]) {
                println!(
                    "  {} - {}",
                    expert["id"].as_str().unwrap_or(""),
                    expert["description"].as_str().unwrap_or("")
                );
            }
        }
        ApiCommands::Trace { id } => {
            let resp = match client
                .get(format!("{}/subhuti/api/v1/trace/{}", base_url, id))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if handle_connection_error(&e) {
                        return Ok(());
                    }
                    return Err(e.into());
                }
            };
            let result: Value = resp.json().await?;
            println!("{}", format!("🔍 追踪 ID: {}", id).yellow().bold());
            println!("───────────────────────────────────────────────────────────────");
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        ApiCommands::Sessions => {
            let resp = match client
                .get(format!("{}/subhuti/api/v1/sessions", base_url))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if handle_connection_error(&e) {
                        return Ok(());
                    }
                    return Err(e.into());
                }
            };
            let result: Value = resp.json().await?;
            println!("{}", "📝 会话列表".yellow().bold());
            println!("───────────────────────────────────────────────────────");
            for session in result["sessions"].as_array().unwrap_or(&vec![]) {
                println!(
                    "  {} - {}",
                    session["id"].as_str().unwrap_or(""),
                    session["user_id"].as_str().unwrap_or("")
                );
            }
        }
        ApiCommands::Orchestrate {
            message,
            user_id,
            chain,
        } => {
            let mut body = serde_json::json!({
                "message": message,
                "user_id": user_id.unwrap_or("default_user".to_string()),
            });

            if let Some(chain) = chain {
                body["chain"] = chain.into();
            }

            println!("🔗 编排执行: {}", message);
            let resp = match client
                .post(format!("{}/subhuti/api/v1/orchestrate", base_url))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if handle_connection_error(&e) {
                        return Ok(());
                    }
                    return Err(e.into());
                }
            };

            if resp.status().is_success() {
                let result: Value = resp.json().await?;
                println!(
                    "{}",
                    "───────────────────────────────────────────────────────────────".yellow()
                );
                println!("{}", result["output"].as_str().unwrap_or(""));
                if let Some(chain) = result["chain"].as_str() {
                    println!("{}", format!("策略: {}", chain).dimmed());
                }
                if let Some(experts) = result["expert_chain"].as_array() {
                    if !experts.is_empty() {
                        println!(
                            "{}",
                            format!(
                                "专家链: {}",
                                experts
                                    .iter()
                                    .filter_map(|e| e.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" → ")
                            )
                            .dimmed()
                        );
                    }
                }
            } else {
                let err: Value = resp.json().await?;
                println!("❌ 错误: {}", err);
            }
        }
    }

    Ok(())
}
