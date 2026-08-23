//! # Rust 编程工作流图
//!
//! 模仿 Trace 系统的事件驱动架构，将 Rust 编程任务编排为多步工作流：
//!
//! 每步执行时 Graph 引擎自动 emit 图事件 → 节点内再 emit LLM/工具事件 →
//! TraceEventBridge 捕获所有事件 → span 树呈现完整调用链路。
//!
//! ## 工作流
//!
//! ```text
//! plan → generate → write_files → verify
//!                                  ↓ (conditional)
//!                           success → complete
//!                           failure → fix_errors → generate (loop)
//! ```
//!
//! ## 差分对比：纯 Graph vs 模仿 Trace
//!
//! | 层级 | 纯 Graph（之前） | 模仿 Trace（现在） |
//! |------|-----------------|-------------------|
//! | 节点级 | 引擎 emit NodeExecuteRequested/Completed | 引擎 emit 同上 |
//! | LLM 级 | 节点内 `llm.chat()` 黑盒 | 节点内 emit LLMCalling/Responded |
//! | 工具级 | 节点内 `std::fs` 黑盒 | 节点内 emit ToolCalling/Responded |
//! | 状态 | 状态字段 | 状态字段 + trace_id 传播 |

use std::collections::HashMap;
use std::sync::Arc;

use subhuti_core::event::{AgentEventData, EventBus};
use subhuti_core::graph::state::reducers;
use subhuti_core::graph::{Graph, GraphBuilder, GraphState, NodeResult, Route};
use subhuti_core::{Message, LLM};

// ─── 系统提示词 ──────────────────────────────────────────────────

const PLAN_SYSTEM: &str = r#"你是一个 Rust 项目规划专家。请根据用户需求，生成项目计划，包括：
1. 项目名称
2. 目录结构（需要创建哪些文件）
3. 使用的 crate 依赖
4. 实现步骤

请用如下 JSON 格式输出（不要包含其他内容）：
```json
{
  "project_name": "项目名",
  "files": ["src/main.rs", "Cargo.toml"],
  "dependencies": ["serde", "anyhow"],
  "steps": ["步骤1", "步骤2"]
}
```"#;

const GENERATE_SYSTEM: &str = r#"你是一个 Rust 代码生成专家。根据用户需求和项目计划，生成完整的 Rust 代码。

请按如下格式输出（每个文件用 File: 开头标记）：
File: src/main.rs
```rust
fn main() {
    println!("Hello, world!");
}
```

File: Cargo.toml
```toml
[package]
name = "test-hello"
version = "0.1.0"
edition = "2021"
```

确保代码完整、可编译。输出仅包含文件内容标记，不要添加额外说明。"#;

const FIX_SYSTEM: &str = r#"你是一个 Rust 代码修复专家。根据编译错误修复代码。

请按如下格式输出修复后的代码（每个文件用 File: 开头标记）：
File: src/main.rs
```rust
// 修复后的代码
```

分析错误原因，只修改有问题的部分，不要修改其他代码。"#;

// ─── 事件辅助函数（模仿 Trace 系统的 EventBus emit）───────────────
//
// 这组函数是"模仿 Trace"的核心：每个 LLM 调用和工具操作都 emit 事件，
// 让 span 树呈现完整的嵌套调用链路，不再是黑盒。

/// 带事件 emit 的 LLM 调用
///
/// 模仿 Trace 系统的 LLMCalling → LLMResponded 事件模式。
async fn llm_chat_with_events(
    llm: &dyn LLM,
    bus: &EventBus,
    system: &str,
    user: &str,
    trace_id: &str,
    session_id: &str,
) -> NodeResult {
    // 模仿 Trace：emit LLMCalling 事件
    bus.emit_with_trace(
        AgentEventData::LLMCalling {
            messages_count: 2,
            model: None,
        },
        trace_id,
        Some(session_id.to_string()),
    )
    .await;

    let start = std::time::Instant::now();
    let messages = vec![Message::system(system), Message::user(user)];
    let result = llm.chat(messages).await;
    let elapsed = start.elapsed().as_millis() as u64;

    match result {
        Ok(output) => {
            // 模仿 Trace：emit LLMResponded 事件
            bus.emit_with_trace(
                AgentEventData::LLMResponded {
                    response: output.clone(),
                    tokens_used: 0,
                    duration_ms: elapsed,
                },
                trace_id,
                Some(session_id.to_string()),
            )
            .await;

            let mut updates = HashMap::new();
            updates.insert(
                "llm_output".to_string(),
                serde_json::Value::String(output.clone()),
            );
            NodeResult::ok_with_state(output, updates)
        }
        Err(e) => NodeResult::err(format!("LLM 调用失败: {}", e)),
    }
}

/// 带事件 emit 的文件写入
///
/// 模仿 Trace 系统的 ToolCalling → ToolResponded 事件模式。
async fn write_project_files_with_events(
    project_path: &str,
    code: &str,
    bus: &EventBus,
    trace_id: &str,
    session_id: &str,
) -> Result<String, String> {
    // 模仿 Trace：emit ToolCalling 事件
    bus.emit_with_trace(
        AgentEventData::ToolCalling {
            tool_name: "write_files".to_string(),
            args: serde_json::json!({"project_path": project_path, "files_count": 1}),
        },
        trace_id,
        Some(session_id.to_string()),
    )
    .await;

    let start = std::time::Instant::now();
    let result = write_project_files(project_path, code);
    let elapsed = start.elapsed().as_millis() as u64;

    match &result {
        Ok(msg) => {
            // 模仿 Trace：emit ToolResponded 事件
            bus.emit_with_trace(
                AgentEventData::ToolResponded {
                    tool_name: "write_files".to_string(),
                    result: msg.clone(),
                    success: true,
                    duration_ms: elapsed,
                },
                trace_id,
                Some(session_id.to_string()),
            )
            .await;
        }
        Err(e) => {
            bus.emit_with_trace(
                AgentEventData::ToolResponded {
                    tool_name: "write_files".to_string(),
                    result: e.clone(),
                    success: false,
                    duration_ms: elapsed,
                },
                trace_id,
                Some(session_id.to_string()),
            )
            .await;
        }
    }

    result
}

/// 带事件 emit 的 cargo check
///
/// 模仿 Trace 系统的 ToolCalling → ToolResponded 事件模式。
async fn run_cargo_check_with_events(
    project_path: &str,
    bus: &EventBus,
    trace_id: &str,
    session_id: &str,
) -> (bool, String) {
    // 模仿 Trace：emit ToolCalling 事件
    bus.emit_with_trace(
        AgentEventData::ToolCalling {
            tool_name: "cargo_check".to_string(),
            args: serde_json::json!({"project_path": project_path}),
        },
        trace_id,
        Some(session_id.to_string()),
    )
    .await;

    let start = std::time::Instant::now();
    let (success, output) = run_cargo_check(project_path);
    let elapsed = start.elapsed().as_millis() as u64;

    // 模仿 Trace：emit ToolResponded 事件
    bus.emit_with_trace(
        AgentEventData::ToolResponded {
            tool_name: "cargo_check".to_string(),
            result: output.clone(),
            success,
            duration_ms: elapsed,
        },
        trace_id,
        Some(session_id.to_string()),
    )
    .await;

    (success, output)
}

// ─── 纯函数（无事件，被带事件的包装函数调用）────────────────────────

/// 创建项目目录并写入文件（纯 IO，无事件）
fn write_project_files(project_path: &str, code: &str) -> Result<String, String> {
    let path = std::path::Path::new(project_path);

    std::fs::create_dir_all(path.join("src")).map_err(|e| format!("创建目录失败: {}", e))?;

    let mut written = Vec::new();
    let mut remaining = code;
    let mut file_count = 0;

    while let Some(file_start) = remaining.find("File: ") {
        remaining = &remaining[file_start + 6..];
        let line_end = remaining.find('\n').unwrap_or(remaining.len());
        let file_path = remaining[..line_end].trim();
        remaining = &remaining[line_end..];

        if let Some(block_start) = remaining.find("```") {
            remaining = &remaining[block_start + 3..];
            if let Some(lang_end) = remaining.find('\n') {
                remaining = &remaining[lang_end + 1..];
            }
            if let Some(block_end) = remaining.find("```") {
                let content = &remaining[..block_end];
                remaining = &remaining[block_end + 3..];

                let full_path = path.join(file_path);
                if let Some(parent) = full_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&full_path, content)
                    .map_err(|e| format!("写入文件 {} 失败: {}", file_path, e))?;
                written.push(file_path.to_string());
                file_count += 1;
            }
        }
    }

    if file_count == 0 {
        let main_path = path.join("src").join("main.rs");
        std::fs::write(&main_path, code).map_err(|e| format!("写入 src/main.rs 失败: {}", e))?;
        written.push("src/main.rs".to_string());
    }

    Ok(format!("已写入文件: {}", written.join(", ")))
}

/// 运行 cargo check（纯 IO，无事件）
fn run_cargo_check(project_path: &str) -> (bool, String) {
    let output = std::process::Command::new("cargo")
        .arg("check")
        .current_dir(project_path)
        .output();

    match output {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                (true, "编译检查通过".to_string())
            } else {
                (false, stderr)
            }
        }
        Err(e) => (false, format!("cargo check 执行失败: {}", e)),
    }
}

// ─── 从 GraphState 提取 trace/session ─────────────────────────────

fn extract_trace(state: &GraphState) -> (String, String) {
    let trace_id = state.get("trace_id").unwrap_or_default();
    let session_id = state.get("session_id").unwrap_or_default();
    (trace_id, session_id)
}

// ─── 图节点工厂函数（每个节点都注入 EventBus 用于 emit 事件）───────

/// 创建节点：规划（emit LLMCalling/LLMResponded）
fn make_plan_node(
    llm: Arc<dyn LLM>,
    bus: Arc<EventBus>,
) -> impl Fn(GraphState) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
       + Send
       + Sync
       + 'static {
    move |state: GraphState| {
        let llm = llm.clone();
        let bus = bus.clone();
        Box::pin(async move {
            let (trace_id, session_id) = extract_trace(&state);
            let input = state.get("input").unwrap_or_default();
            let result = llm_chat_with_events(
                llm.as_ref(),
                &bus,
                PLAN_SYSTEM,
                &input,
                &trace_id,
                &session_id,
            )
            .await;

            if result.success {
                let mut updates = HashMap::new();
                updates.insert(
                    "plan".to_string(),
                    serde_json::Value::String(result.output.clone()),
                );

                // 提取 project_name
                let output = &result.output;
                if let Some(json_start) = output.find("```json") {
                    let after = &output[json_start + 7..];
                    if let Some(json_end) = after.find("```") {
                        let json_str = &after[..json_end];
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                            if let Some(name) = parsed.get("project_name").and_then(|v| v.as_str())
                            {
                                updates.insert(
                                    "project_name".to_string(),
                                    serde_json::Value::String(name.to_string()),
                                );
                            }
                        }
                    }
                }

                NodeResult::ok_with_state(result.output, updates)
            } else {
                result
            }
        })
    }
}

/// 创建节点：生成代码（emit LLMCalling/LLMResponded）
fn make_generate_node(
    llm: Arc<dyn LLM>,
    bus: Arc<EventBus>,
) -> impl Fn(GraphState) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
       + Send
       + Sync
       + 'static {
    move |state: GraphState| {
        let llm = llm.clone();
        let bus = bus.clone();
        Box::pin(async move {
            let (trace_id, session_id) = extract_trace(&state);
            let input = state.get("input").unwrap_or_default();
            let plan = state.get("plan").unwrap_or_default();
            let errors = state.get("errors").unwrap_or_default();

            let user_prompt = if errors.is_empty() {
                format!("用户需求：{}\n\n项目计划：{}", input, plan)
            } else {
                format!(
                    "用户需求：{}\n\n项目计划：{}\n\n需要修复以下编译错误：\n{}",
                    input, plan, errors
                )
            };

            let result = llm_chat_with_events(
                llm.as_ref(),
                &bus,
                GENERATE_SYSTEM,
                &user_prompt,
                &trace_id,
                &session_id,
            )
            .await;

            if result.success {
                let mut updates = HashMap::new();
                updates.insert(
                    "code".to_string(),
                    serde_json::Value::String(result.output.clone()),
                );
                NodeResult::ok_with_state(result.output, updates)
            } else {
                result
            }
        })
    }
}

/// 创建节点：写入文件（emit ToolCalling/ToolResponded）
fn make_write_files_node(
    bus: Arc<EventBus>,
) -> impl Fn(GraphState) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
       + Send
       + Sync
       + 'static {
    move |state: GraphState| {
        let bus = bus.clone();
        Box::pin(async move {
            let (trace_id, session_id) = extract_trace(&state);
            let code = state.get("code").unwrap_or_default();
            let project_name = state
                .get("project_name")
                .unwrap_or_else(|| "rust-project".to_string());
            // 优先使用 workspace_folder（用户设置的项目工作目录），否则使用硬编码路径
            let workspace_folder = state.get("workspace_folder").unwrap_or_default();
            let project_path = if workspace_folder.is_empty() {
                format!("/Users/hezenghui/RustroverProjects/{}", project_name)
            } else {
                format!("{}/{}", workspace_folder, project_name)
            };

            match write_project_files_with_events(
                &project_path,
                &code,
                &bus,
                &trace_id,
                &session_id,
            )
            .await
            {
                Ok(msg) => {
                    let mut updates = HashMap::new();
                    updates.insert(
                        "project_path".to_string(),
                        serde_json::Value::String(project_path),
                    );
                    updates.insert(
                        "write_result".to_string(),
                        serde_json::Value::String(msg.clone()),
                    );
                    NodeResult::ok_with_state(msg, updates)
                }
                Err(e) => NodeResult::err(e),
            }
        })
    }
}

/// 创建节点：验证编译（emit ToolCalling/ToolResponded）
fn make_verify_node(
    bus: Arc<EventBus>,
) -> impl Fn(GraphState) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
       + Send
       + Sync
       + 'static {
    move |state: GraphState| {
        let bus = bus.clone();
        Box::pin(async move {
            let (trace_id, session_id) = extract_trace(&state);
            let project_path = state.get("project_path").unwrap_or_default();

            let (success, output) =
                run_cargo_check_with_events(&project_path, &bus, &trace_id, &session_id).await;

            let mut updates = HashMap::new();
            updates.insert(
                "verify_success".to_string(),
                serde_json::Value::String(if success {
                    "true".to_string()
                } else {
                    "false".to_string()
                }),
            );

            if !success {
                updates.insert(
                    "errors".to_string(),
                    serde_json::Value::String(output.clone()),
                );

                let fix_count = state
                    .get("fix_count")
                    .unwrap_or_default()
                    .parse::<i64>()
                    .unwrap_or(0);
                updates.insert(
                    "fix_count".to_string(),
                    serde_json::Value::Number((fix_count + 1).into()),
                );

                if fix_count >= 3 {
                    updates.insert(
                        "verify_success".to_string(),
                        serde_json::Value::String("true".to_string()),
                    );
                    NodeResult::ok_with_state(
                        format!("达到最大修复次数 ({}), 跳过编译验证", fix_count),
                        updates,
                    )
                } else {
                    NodeResult::ok_with_state(output, updates)
                }
            } else {
                NodeResult::ok_with_state("编译检查通过".to_string(), updates)
            }
        })
    }
}

/// 创建节点：修复错误（emit LLMCalling/LLMResponded）
fn make_fix_node(
    llm: Arc<dyn LLM>,
    bus: Arc<EventBus>,
) -> impl Fn(GraphState) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
       + Send
       + Sync
       + 'static {
    move |state: GraphState| {
        let llm = llm.clone();
        let bus = bus.clone();
        Box::pin(async move {
            let (trace_id, session_id) = extract_trace(&state);
            let code = state.get("code").unwrap_or_default();
            let errors = state.get("errors").unwrap_or_default();

            let user_prompt = format!("需要修复的代码：\n{}\n\n编译错误：\n{}", code, errors);

            let result = llm_chat_with_events(
                llm.as_ref(),
                &bus,
                FIX_SYSTEM,
                &user_prompt,
                &trace_id,
                &session_id,
            )
            .await;

            if result.success {
                let mut updates = HashMap::new();
                updates.insert(
                    "code".to_string(),
                    serde_json::Value::String(result.output.clone()),
                );
                updates.insert(
                    "errors".to_string(),
                    serde_json::Value::String(String::new()),
                );
                NodeResult::ok_with_state(result.output, updates)
            } else {
                result
            }
        })
    }
}

/// 创建节点：完成（无事件，纯组装输出）
fn make_complete_node(
) -> impl Fn(GraphState) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
       + Send
       + Sync
       + 'static {
    |state: GraphState| {
        Box::pin(async move {
            let project_path = state.get("project_path").unwrap_or_default();
            let write_result = state.get("write_result").unwrap_or_default();
            let plan = state.get("plan").unwrap_or_default();
            let verify_success = state.get("verify_success").unwrap_or_default();

            let summary = format!(
                "✅ Rust 项目创建完成！\n\n📋 项目路径：{}\n{}\n📐 计划：{}\n🔍 编译验证：{}\n\n项目已创建在 {} 目录下，可直接使用。",
                project_path, write_result, plan,
                if verify_success == "true" { "✅ 通过" } else { "⚠️ 未验证" },
                project_path
            );

            NodeResult::ok(summary)
        })
    }
}

// ─── 公开 API ──────────────────────────────────────────────────

/// 创建 Rust 编程工作流图
///
/// 接收 LLM 实例和 EventBus，返回一个完整的 Graph。
///
/// ## 模仿 Trace 的关键设计
///
/// 1. **节点内 LLM 调用 emit `LLMCalling`/`LLMResponded` 事件**
/// 2. **节点内工具操作 emit `ToolCalling`/`ToolResponded` 事件**
/// 3. **trace_id 从 GraphState 提取，事件携带 trace 上下文**
/// 4. **现有 TraceEventBridge 自动捕获，span 树呈现嵌套结构**
///
/// 效果：span 树从 `graph_started → node_completed` 两级变为
/// `graph_started → node_execute_requested → llm_calling → llm_responded → node_completed` 四级。
pub fn create_rust_programming_graph(llm: Arc<dyn LLM>, bus: Arc<EventBus>) -> Graph {
    let plan_node = make_plan_node(llm.clone(), bus.clone());
    let generate_node = make_generate_node(llm.clone(), bus.clone());
    let write_files_node = make_write_files_node(bus.clone());
    let verify_node = make_verify_node(bus.clone());
    let fix_node = make_fix_node(llm.clone(), bus.clone());
    let complete_node = make_complete_node();

    GraphBuilder::new()
        .name("rust_programming")
        .node("plan", plan_node)
        .node_tag("plan", vec!["planning".to_string(), "rust".to_string()])
        .node("generate", generate_node)
        .node_tag(
            "generate",
            vec![
                "coding".to_string(),
                "rust".to_string(),
                "generation".to_string(),
            ],
        )
        .node("write_files", write_files_node)
        .node_tag("write_files", vec!["file_io".to_string()])
        .node("verify", verify_node)
        .node_tag(
            "verify",
            vec!["verification".to_string(), "rust".to_string()],
        )
        .node("fix_errors", fix_node)
        .node_tag(
            "fix_errors",
            vec!["debugging".to_string(), "rust".to_string()],
        )
        .node("complete", complete_node)
        .node_tag("complete", vec!["reporting".to_string()])
        .edge("plan", "generate")
        .edge("generate", "write_files")
        .edge("write_files", "verify")
        .conditional_edge("verify", |state| {
            let success = state.get("verify_success").unwrap_or_default();
            if success == "true" {
                Route::To("complete".to_string())
            } else {
                Route::To("fix_errors".to_string())
            }
        })
        .edge("fix_errors", "generate")
        .entry("plan")
        .max_iterations(10)
        .reducer("fix_count", reducers::max())
        .build()
        .expect("rust_programming 图构建失败，请检查节点定义")
}
