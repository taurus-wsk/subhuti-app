//! # Rust 项目编辑工作流图
//!
//! 增量编辑现有 Rust 项目：
//!
//! ```text
//! analyze → diff_plan → edit_files → verify
//!                                    ↓ success → complete
//!                                    ↓ failure → fix_errors → verify (loop, max 3次)
//! ```
//!
//! ## 事件架构
//!
//! 每个节点内 LLM 调用 emit LLMCalling/LLMResponded，工具操作 emit ToolCalling/ToolResponded，
//! TraceEventBridge 捕获后 span 树呈现完整嵌套调用链路。

use std::collections::HashMap;
use std::sync::Arc;

use crate::application::observer::{record_fn_log, LogLevel};
use serde::Deserialize;
use subhuti_core::event::{AgentEventData, EventBus};
use subhuti_core::graph::state::reducers;
use subhuti_core::graph::{Graph, GraphBuilder, GraphState, NodeResult};
use subhuti_core::{Message, LLM};

// ─── 系统提示词 ──────────────────────────────────────────────────

const ANALYZE_SYSTEM: &str = r#"你是一个 Rust 项目分析专家。分析以下项目结构，给出：

1. 项目类型（binary/library/workspace）
2. 主要模块和文件列表（每个文件的功能摘要）
3. 关键类型和函数接口
4. 现有依赖
5. 建议的修改点

请用如下 JSON 格式输出（不要包含其他内容）：
```json
{
  "project_type": "binary",
  "files": [
    {"path": "src/main.rs", "summary": "主入口，包含 main 函数"},
    {"path": "src/lib.rs", "summary": "库模块，包含核心类型"}
  ],
  "key_types": ["App", "Config"],
  "key_functions": ["main", "setup"],
  "dependencies": ["serde", "anyhow"],
  "suggestions": ["可以添加 HTTP 路由处理"]
}
```"#;

const DIFF_PLAN_SYSTEM: &str = r#"你是一个 Rust 代码修改规划专家。根据用户需求和项目结构分析，制定精确的修改计划。

修改计划必须使用以下 JSON 格式输出（不要包含其他内容）：

```json
{
  "summary": "修改概述，用一句话描述要做什么",
  "edit_files": [
    {
      "path": "src/main.rs",
      "operation": "insert_after",
      "anchor": "fn main() {",
      "content": "    println!(\"新功能!\");\n"
    },
    {
      "path": "src/lib.rs",
      "operation": "replace_block",
      "start_marker": "fn old_function() {",
      "end_marker": "}",
      "content": "fn new_function() -> String {\n    \"hello\".to_string()\n}\n"
    },
    {
      "path": "src/main.rs",
      "operation": "append",
      "content": "fn new_helper() {\n    println!(\"helper\");\n}\n"
    }
  ],
  "new_files": [
    {
      "path": "src/handler.rs",
      "content": "pub fn handle() {}\n"
    }
  ],
  "dependencies": {
    "add": ["serde"],
    "remove": []
  }
}
```

支持的 operation 类型：
- insert_after: 在 anchor 行之后插入 content
- replace_block: 用 content 替换从 start_marker 到 end_marker 之间的内容（包含标记行）
- append: 将 content 追加到文件末尾

确保修改计划精确、最小化，只修改必要的部分。不要修改无关代码。"#;

const FIX_SYSTEM: &str = r#"你是一个 Rust 代码修复专家。根据编译错误和当前代码，制定修复方案。

编译错误通常包含文件路径、行号和错误描述。请仔细分析每个错误，定位到具体的代码行。

修复方案必须使用以下 JSON 格式输出（不要包含其他内容）：

```json
{
  "summary": "修复概述，用一句话描述修复了什么",
  "edit_files": [
    {
      "path": "src/main.rs",
      "operation": "replace_block",
      "start_marker": "fn broken_function() {",
      "end_marker": "}",
      "content": "fn fixed_function() -> String {\n    \"fixed\".to_string()\n}\n"
    }
  ],
  "new_files": [],
  "dependencies": {
    "add": [],
    "remove": []
  }
}
```

支持的 operation 类型：
- insert_after: 在 anchor 行之后插入 content
- replace_block: 用 content 替换从 start_marker 到 end_marker 之间的内容（包含标记行）
- append: 将 content 追加到文件末尾

规则：
1. 只修复有编译错误的代码，不要修改无关代码
2. 如果错误是缺少 use 语句，使用 insert_after 在文件顶部添加
3. 如果错误是类型不匹配，使用 replace_block 替换有问题的代码块
4. 确保修复后的代码能编译通过
5. 不要添加新功能，只修复错误"#;

// ─── 辅助函数 ──────────────────────────────────────────────────

/// 从 GraphState 提取 trace_id 和 session_id
fn extract_trace(state: &GraphState) -> (String, String) {
    let trace_id = state.get("trace_id").unwrap_or_default();
    let session_id = state.get("session_id").unwrap_or_default();
    (trace_id, session_id)
}

/// 从用户输入中提取项目路径
///
/// 支持两种格式：
/// 1. "修改 /Users/xxx/project，添加功能" → 提取 /Users/xxx/project
/// 2. 未指定路径时返回默认路径
fn extract_project_path(input: &str) -> String {
    // 尝试匹配绝对路径
    for word in input.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| c == '，' || c == ',' || c == '。' || c == '.');
        if trimmed.starts_with('/') || trimmed.starts_with("~/") {
            return trimmed.to_string();
        }
    }
    // 尝试匹配 "项目路径：" 或 "path:"
    if let Some(pos) = input.find("项目路径：") {
        let rest = &input[pos + 5..];
        let path = rest.split_whitespace().next().unwrap_or("").trim();
        if !path.is_empty() {
            return path.to_string();
        }
    }
    if let Some(pos) = input.to_lowercase().find("path:") {
        let rest = &input[pos + 5..];
        let path = rest.split_whitespace().next().unwrap_or("").trim();
        if !path.is_empty() {
            return path.to_string();
        }
    }
    // 默认路径
    "/Users/hezenghui/RustroverProjects".to_string()
}

// ─── 带事件 emit 的 LLM 调用 ──────────────────────────────────

/// 带事件 emit 的 LLM 调用（模仿 Trace 的 LLMCalling → LLMResponded 模式）
async fn llm_chat_with_events(
    llm: &dyn LLM,
    bus: &EventBus,
    system: &str,
    user: &str,
    trace_id: &str,
    session_id: &str,
) -> NodeResult {
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

// ─── 带事件 emit 的项目结构读取 ──────────────────────────────

/// 读取项目结构（纯 IO，emit ToolCalling/ToolResponded）
async fn read_project_structure_with_events(
    project_path: &str,
    bus: &EventBus,
    trace_id: &str,
    session_id: &str,
) -> Result<String, String> {
    bus.emit_with_trace(
        AgentEventData::ToolCalling {
            tool_name: "read_project".to_string(),
            args: serde_json::json!({"project_path": project_path}),
        },
        trace_id,
        Some(session_id.to_string()),
    )
    .await;

    let start = std::time::Instant::now();
    let result = read_project_structure(project_path);
    let elapsed = start.elapsed().as_millis() as u64;

    match &result {
        Ok(msg) => {
            bus.emit_with_trace(
                AgentEventData::ToolResponded {
                    tool_name: "read_project".to_string(),
                    result: format!("项目结构读取成功（{} 字符）", msg.len()),
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
                    tool_name: "read_project".to_string(),
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

/// 读取项目结构（纯函数，无事件）
///
/// 读取 Cargo.toml 和 src/ 下所有 .rs 文件的前 30 行，
/// 返回格式化的项目结构描述。
fn read_project_structure(project_path: &str) -> Result<String, String> {
    let path = std::path::Path::new(project_path);
    if !path.exists() {
        return Err(format!("项目路径不存在: {}", project_path));
    }

    let mut result = String::new();

    // 1. 读取 Cargo.toml
    let cargo_path = path.join("Cargo.toml");
    if cargo_path.exists() {
        let content = std::fs::read_to_string(&cargo_path)
            .map_err(|e| format!("读取 Cargo.toml 失败: {}", e))?;
        result.push_str("=== Cargo.toml ===\n");
        result.push_str(&content);
        result.push('\n');
    } else {
        result.push_str("（未找到 Cargo.toml）\n");
    }

    // 2. 读取 src/ 目录下的 .rs 文件
    let src_path = path.join("src");
    if src_path.exists() {
        result.push_str("\n=== 源码文件 ===\n");
        let mut rs_files: Vec<_> = Vec::new();
        collect_rs_files(&src_path, &src_path, &mut rs_files);

        if rs_files.is_empty() {
            result.push_str("（src/ 目录下无 .rs 文件）\n");
        }

        for (relative_path, full_path) in &rs_files {
            match std::fs::read_to_string(full_path) {
                Ok(content) => {
                    let line_count = content.lines().count();
                    let lines: Vec<&str> = content.lines().take(30).collect();
                    result.push_str(&format!(
                        "\n--- {}（共 {} 行，显示前 30 行）---\n",
                        relative_path, line_count
                    ));
                    result.push_str(&lines.join("\n"));
                    if line_count > 30 {
                        result.push_str(&format!("\n...（剩余 {} 行已省略）", line_count - 30));
                    }
                    result.push('\n');
                }
                Err(e) => {
                    result.push_str(&format!("\n--- {}（读取失败: {}）---\n", relative_path, e));
                }
            }
        }
    } else {
        result.push_str("（未找到 src/ 目录）\n");
    }

    Ok(result)
}

/// 递归收集 src/ 目录下所有 .rs 文件
fn collect_rs_files(
    base: &std::path::Path,
    dir: &std::path::Path,
    files: &mut Vec<(String, std::path::PathBuf)>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(base, &path, files);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Ok(relative) = path.strip_prefix(base) {
                    files.push((relative.display().to_string(), path));
                }
            }
        }
    }
}

// ─── Diff Plan 数据结构 ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DiffPlan {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    edit_files: Vec<EditFile>,
    #[serde(default)]
    new_files: Vec<NewFile>,
    #[serde(default)]
    #[allow(dead_code)]
    dependencies: Dependencies,
}

#[derive(Debug, Deserialize)]
struct EditFile {
    path: String,
    operation: String,
    #[serde(default)]
    anchor: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    start_marker: String,
    #[serde(default)]
    end_marker: String,
}

#[derive(Debug, Deserialize)]
struct NewFile {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct Dependencies {
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
}

// ─── Diff Plan 解析 ─────────────────────────────────────────────

/// 从 LLM 输出中提取 JSON 内容
///
/// 支持两种格式：
/// 1. ```json ... ``` 包裹的 JSON
/// 2. 裸 JSON（从第一个 { 到最后一个 }）
fn extract_json_from_llm_output(output: &str) -> Result<String, String> {
    // 尝试匹配 ```json ... ``` 块
    if let Some(start) = output.find("```json") {
        let after = &output[start + 7..];
        if let Some(end) = after.find("```") {
            return Ok(after[..end].trim().to_string());
        }
    }
    // 尝试匹配 ``` ... ``` 块（不带 json 标记）
    if let Some(start) = output.find("```") {
        let after = &output[start + 3..];
        // 跳过可能的语言标记行
        let after = if let Some(nl) = after.find('\n') {
            &after[nl + 1..]
        } else {
            after
        };
        if let Some(end) = after.find("```") {
            return Ok(after[..end].trim().to_string());
        }
    }
    // 尝试匹配裸 JSON 对象
    if let Some(start) = output.find('{') {
        if let Some(end) = output.rfind('}') {
            return Ok(output[start..=end].to_string());
        }
    }
    Err("未找到 JSON 格式的修改计划".to_string())
}

/// 解析 diff_plan JSON 字符串
fn parse_diff_plan(diff_plan_str: &str) -> Result<DiffPlan, String> {
    let json_str = extract_json_from_llm_output(diff_plan_str)?;
    serde_json::from_str::<DiffPlan>(&json_str)
        .map_err(|e| format!("解析修改计划 JSON 失败: {}", e))
}

// ─── 文件编辑操作 ───────────────────────────────────────────────

/// 执行 insert_after 操作：在 anchor 行之后插入 content
fn apply_insert_after(content: &str, anchor: &str, insert: &str) -> Result<String, String> {
    if let Some(pos) = content.find(anchor) {
        // 找到 anchor 所在行的行尾
        let remaining = &content[pos..];
        let line_end = remaining
            .find('\n')
            .map(|i| pos + i + 1)
            .unwrap_or(content.len());
        let mut new_content = String::with_capacity(content.len() + insert.len());
        new_content.push_str(&content[..line_end]);
        new_content.push_str(insert);
        if !insert.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(&content[line_end..]);
        Ok(new_content)
    } else {
        Err(format!("未找到锚点行: {}", anchor))
    }
}

/// 执行 replace_block 操作：替换 start_marker 到 end_marker 之间的内容
fn apply_replace_block(
    content: &str,
    start_marker: &str,
    end_marker: &str,
    replacement: &str,
) -> Result<String, String> {
    let start = content
        .find(start_marker)
        .ok_or_else(|| format!("未找到起始标记: {}", start_marker))?;
    let after_start = &content[start..];
    let end = after_start
        .find(end_marker)
        .map(|i| start + i + end_marker.len())
        .ok_or_else(|| format!("未找到结束标记: {}", end_marker))?;
    let mut new_content = String::with_capacity(content.len() + replacement.len());
    new_content.push_str(&content[..start]);
    new_content.push_str(replacement);
    new_content.push_str(&content[end..]);
    Ok(new_content)
}

/// 执行 append 操作：追加到文件末尾
fn apply_append(content: &str, append: &str) -> String {
    let trimmed = content.trim_end();
    let mut result = String::with_capacity(trimmed.len() + append.len() + 2);
    result.push_str(trimmed);
    result.push('\n');
    result.push('\n');
    result.push_str(append);
    if !append.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// 对单个文件执行编辑操作
fn apply_edit_to_file(project_path: &str, edit: &EditFile) -> Result<String, String> {
    let full_path = std::path::Path::new(project_path).join(&edit.path);
    if !full_path.exists() {
        return Err(format!("文件不存在: {}", edit.path));
    }

    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("读取文件 {} 失败: {}", edit.path, e))?;

    let new_content = match edit.operation.as_str() {
        "insert_after" => {
            if edit.anchor.is_empty() {
                return Err("insert_after 操作缺少 anchor 参数".to_string());
            }
            apply_insert_after(&content, &edit.anchor, &edit.content)?
        }
        "replace_block" => {
            if edit.start_marker.is_empty() || edit.end_marker.is_empty() {
                return Err("replace_block 操作缺少 start_marker 或 end_marker 参数".to_string());
            }
            apply_replace_block(
                &content,
                &edit.start_marker,
                &edit.end_marker,
                &edit.content,
            )?
        }
        "append" => apply_append(&content, &edit.content),
        other => return Err(format!("不支持的操作类型: {}", other)),
    };

    // 写回文件
    std::fs::write(&full_path, &new_content)
        .map_err(|e| format!("写入文件 {} 失败: {}", edit.path, e))?;

    // 生成操作描述
    let desc = match edit.operation.as_str() {
        "insert_after" => format!("在 '{}' 之后插入", edit.anchor),
        "replace_block" => format!(
            "替换从 '{}' 到 '{}' 的内容",
            edit.start_marker, edit.end_marker
        ),
        "append" => "追加到文件末尾".to_string(),
        _ => edit.operation.clone(),
    };
    let lines = edit.content.lines().count();
    Ok(format!("{}（{} 行）: {}", edit.path, lines, desc))
}

/// 创建新文件
fn create_new_file(project_path: &str, new_file: &NewFile) -> Result<String, String> {
    let full_path = std::path::Path::new(project_path).join(&new_file.path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
    }
    std::fs::write(&full_path, &new_file.content)
        .map_err(|e| format!("写入文件 {} 失败: {}", new_file.path, e))?;
    let lines = new_file.content.lines().count();
    Ok(format!("{}（{} 行）", new_file.path, lines))
}

// ─── 图节点工厂函数 ──────────────────────────────────────────

/// 创建节点：分析项目结构
///
/// 1. 从用户输入中提取项目路径
/// 2. 读取项目文件结构（emit ToolCalling/ToolResponded）
/// 3. 调用 LLM 分析项目（emit LLMCalling/LLMResponded）
/// 4. 将分析结果存入 GraphState
fn make_analyze_node(
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

            // 1. 提取项目路径
            let project_path = extract_project_path(&input);
            let mut updates = HashMap::new();
            updates.insert(
                "project_path".to_string(),
                serde_json::Value::String(project_path.clone()),
            );

            // 2. 读取项目结构
            let structure = match read_project_structure_with_events(
                &project_path,
                &bus,
                &trace_id,
                &session_id,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => {
                    // 项目结构读取失败也继续，让 LLM 基于有限信息分析
                    format!("（读取项目结构失败: {}）", e)
                }
            };

            updates.insert(
                "project_structure".to_string(),
                serde_json::Value::String(structure.clone()),
            );

            // 3. 调用 LLM 分析项目
            let user_prompt = format!("用户需求：{}\n\n项目结构：\n{}", input, structure);
            let result = llm_chat_with_events(
                llm.as_ref(),
                &bus,
                ANALYZE_SYSTEM,
                &user_prompt,
                &trace_id,
                &session_id,
            )
            .await;

            if result.success {
                updates.insert(
                    "project_analysis".to_string(),
                    serde_json::Value::String(result.output.clone()),
                );
                NodeResult::ok_with_state(result.output, updates)
            } else {
                result
            }
        })
    }
}

/// 创建节点：制定修改方案
///
/// 1. 从 GraphState 获取用户需求和项目分析结果
/// 2. 调用 LLM 生成 diff 修改计划（emit LLMCalling/LLMResponded）
/// 3. 将修改计划存入 GraphState
fn make_diff_plan_node(
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
            let project_analysis = state.get("project_analysis").unwrap_or_default();
            let project_path = state.get("project_path").unwrap_or_default();
            let project_structure = state.get("project_structure").unwrap_or_default();

            let user_prompt = format!(
                "用户需求：{}\n\n项目路径：{}\n\n项目结构：\n{}\n\n项目分析：\n{}",
                input, project_path, project_structure, project_analysis
            );

            let result = llm_chat_with_events(
                llm.as_ref(),
                &bus,
                DIFF_PLAN_SYSTEM,
                &user_prompt,
                &trace_id,
                &session_id,
            )
            .await;

            if result.success {
                let mut updates = HashMap::new();
                updates.insert(
                    "diff_plan".to_string(),
                    serde_json::Value::String(result.output.clone()),
                );
                NodeResult::ok_with_state(result.output, updates)
            } else {
                result
            }
        })
    }
}

/// 创建节点：执行文件编辑
///
/// 1. 从 GraphState 读取 diff_plan 和 project_path
/// 2. 解析 diff_plan JSON
/// 3. 对每个 edit_file 执行编辑操作（emit ToolCalling/ToolResponded）
/// 4. 对每个 new_file 创建新文件（emit ToolCalling/ToolResponded）
/// 5. 将编辑结果存入 GraphState
fn make_edit_files_node(
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
            let diff_plan_str = state.get("diff_plan").unwrap_or_default();

            // 1. 解析 diff_plan
            let plan = match parse_diff_plan(&diff_plan_str) {
                Ok(p) => p,
                Err(e) => {
                    return NodeResult::err(format!("解析修改计划失败: {}", e));
                }
            };

            let mut edit_results: Vec<String> = Vec::new();
            let mut errors: Vec<String> = Vec::new();
            let mut has_edit = false;

            // 2. 执行编辑操作
            for edit in &plan.edit_files {
                let tool_name = format!("edit_file:{}", edit.path);
                bus.emit_with_trace(
                    AgentEventData::ToolCalling {
                        tool_name: tool_name.clone(),
                        args: serde_json::json!({
                            "path": edit.path,
                            "operation": edit.operation,
                        }),
                    },
                    &trace_id,
                    Some(session_id.clone()),
                )
                .await;

                let start = std::time::Instant::now();
                let result = apply_edit_to_file(&project_path, edit);
                let elapsed = start.elapsed().as_millis() as u64;

                match result {
                    Ok(desc) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: desc.clone(),
                                success: true,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        edit_results.push(desc);
                        has_edit = true;
                    }
                    Err(e) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: e.clone(),
                                success: false,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        errors.push(format!("{}: {}", edit.path, e));
                    }
                }
            }

            // 3. 创建新文件
            for new_file in &plan.new_files {
                let tool_name = format!("create_file:{}", new_file.path);
                bus.emit_with_trace(
                    AgentEventData::ToolCalling {
                        tool_name: tool_name.clone(),
                        args: serde_json::json!({
                            "path": new_file.path,
                        }),
                    },
                    &trace_id,
                    Some(session_id.clone()),
                )
                .await;

                let start = std::time::Instant::now();
                let result = create_new_file(&project_path, new_file);
                let elapsed = start.elapsed().as_millis() as u64;

                match result {
                    Ok(desc) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: desc.clone(),
                                success: true,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        edit_results.push(format!("创建 {}", desc));
                        has_edit = true;
                    }
                    Err(e) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: e.clone(),
                                success: false,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        errors.push(format!("创建 {}: {}", new_file.path, e));
                    }
                }
            }

            // 4. 组装结果
            let mut output = String::new();
            if !plan.summary.is_empty() {
                output.push_str(&format!("📋 修改计划：{}\n\n", plan.summary));
            }
            if !edit_results.is_empty() {
                output.push_str("✅ 已执行：\n");
                for r in &edit_results {
                    output.push_str(&format!("  - {}\n", r));
                }
            }
            if !errors.is_empty() {
                output.push_str("\n❌ 失败：\n");
                for e in &errors {
                    output.push_str(&format!("  - {}\n", e));
                }
            }
            if !has_edit && errors.is_empty() {
                output
                    .push_str("⚠️  修改计划中没有需要编辑的文件或创建的新文件，无需执行编辑操作。");
            }

            let mut updates = HashMap::new();
            updates.insert(
                "edit_results".to_string(),
                serde_json::Value::String(edit_results.join("\n")),
            );
            updates.insert(
                "edit_errors".to_string(),
                serde_json::Value::String(errors.join("\n")),
            );
            updates.insert(
                "edit_summary".to_string(),
                serde_json::Value::String(plan.summary.clone()),
            );

            NodeResult::ok_with_state(output, updates)
        })
    }
}

// ─── 编译验证 ──────────────────────────────────────────────────

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

/// 带事件 emit 的 cargo check
async fn run_cargo_check_with_events(
    project_path: &str,
    bus: &EventBus,
    trace_id: &str,
    session_id: &str,
) -> (bool, String) {
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

/// 创建节点：编译验证
///
/// 1. 从 GraphState 读取 project_path
/// 2. 运行 cargo check（emit ToolCalling/ToolResponded）
/// 3. 将验证结果存入 GraphState（verify_success, verify_output）
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

            if project_path.is_empty() || project_path == "/Users/hezenghui/RustroverProjects" {
                return NodeResult::err("未指定项目路径，无法执行编译验证".to_string());
            }

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
            updates.insert(
                "verify_output".to_string(),
                serde_json::Value::String(output.clone()),
            );

            if success {
                record_fn_log(
                    None,
                    "",
                    LogLevel::Info,
                    format!("✅ 编译验证通过: {}", project_path),
                    None,
                );
                NodeResult::ok_with_state("✅ 编译检查通过".to_string(), updates)
            } else {
                record_fn_log(
                    None,
                    "",
                    LogLevel::Warn,
                    format!("❌ 编译验证失败: {}", project_path),
                    None,
                );
                // 截取前 1000 字符避免输出过长
                let truncated = if output.len() > 1000 {
                    format!("{}...（共 {} 字符，已截断）", &output[..1000], output.len())
                } else {
                    output
                };
                NodeResult::ok_with_state(format!("❌ 编译验证失败：\n{}", truncated), updates)
            }
        })
    }
}

// ─── 读取项目文件（全内容） ────────────────────────────────────

/// 读取项目文件全内容（emit ToolCalling/ToolResponded）
async fn read_project_files_with_events(
    project_path: &str,
    bus: &EventBus,
    trace_id: &str,
    session_id: &str,
) -> Result<String, String> {
    bus.emit_with_trace(
        AgentEventData::ToolCalling {
            tool_name: "read_project_files".to_string(),
            args: serde_json::json!({"project_path": project_path}),
        },
        trace_id,
        Some(session_id.to_string()),
    )
    .await;

    let start = std::time::Instant::now();
    let result = read_project_files(project_path);
    let elapsed = start.elapsed().as_millis() as u64;

    match &result {
        Ok(msg) => {
            bus.emit_with_trace(
                AgentEventData::ToolResponded {
                    tool_name: "read_project_files".to_string(),
                    result: format!(
                        "读取了 {} 个文件（{} 字符）",
                        msg.lines().filter(|l| l.starts_with("---")).count(),
                        msg.len()
                    ),
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
                    tool_name: "read_project_files".to_string(),
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

/// 读取项目所有 .rs 文件的全内容（纯函数，无事件）
///
/// 与 read_project_structure 不同，此函数返回文件完整内容（非摘要），
/// 供 LLM 精确分析编译错误。
fn read_project_files(project_path: &str) -> Result<String, String> {
    let path = std::path::Path::new(project_path);
    if !path.exists() {
        return Err(format!("项目路径不存在: {}", project_path));
    }

    let mut result = String::new();

    // 读取 Cargo.toml
    let cargo_path = path.join("Cargo.toml");
    if cargo_path.exists() {
        let content = std::fs::read_to_string(&cargo_path)
            .map_err(|e| format!("读取 Cargo.toml 失败: {}", e))?;
        result.push_str("=== Cargo.toml ===\n");
        result.push_str(&content);
        result.push_str("\n\n");
    }

    // 读取 src/ 下所有 .rs 文件
    let src_path = path.join("src");
    if src_path.exists() {
        let mut rs_files: Vec<(String, std::path::PathBuf)> = Vec::new();
        collect_rs_files(&src_path, &src_path, &mut rs_files);

        for (relative_path, full_path) in &rs_files {
            match std::fs::read_to_string(full_path) {
                Ok(content) => {
                    result.push_str(&format!(
                        "--- {}（{} 行）---\n",
                        relative_path,
                        content.lines().count()
                    ));
                    result.push_str(&content);
                    if !content.ends_with('\n') {
                        result.push('\n');
                    }
                    result.push('\n');
                }
                Err(e) => {
                    result.push_str(&format!("--- {}（读取失败: {}）---\n", relative_path, e));
                }
            }
        }
    }

    if result.is_empty() {
        return Err("项目中未找到任何文件".to_string());
    }

    Ok(result)
}

// ─── 修复编译错误节点 ──────────────────────────────────────────

/// 创建节点：修复编译错误
///
/// 1. 从 GraphState 读取 project_path 和 verify_output（编译错误）
/// 2. 读取当前项目文件（emit ToolCalling/ToolResponded）
/// 3. 调用 LLM 生成修复方案（emit LLMCalling/LLMResponded）
/// 4. 执行修复操作（emit ToolCalling/ToolResponded）
/// 5. 递增 fix_count 并存入 GraphState
fn make_fix_errors_node(
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
            let project_path = state.get("project_path").unwrap_or_default();
            let verify_output = state.get("verify_output").unwrap_or_default();
            let input = state.get("input").unwrap_or_default();

            if project_path.is_empty() || project_path == "/Users/hezenghui/RustroverProjects" {
                return NodeResult::err("未指定项目路径，无法修复编译错误".to_string());
            }

            // 1. 读取项目文件
            let files_content =
                match read_project_files_with_events(&project_path, &bus, &trace_id, &session_id)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        return NodeResult::err(format!("读取项目文件失败: {}", e));
                    }
                };

            // 2. 调用 LLM 生成修复方案
            let user_prompt = format!(
                "用户原始需求：{}\n\n项目路径：{}\n\n当前代码：\n{}\n\n编译错误：\n{}",
                input, project_path, files_content, verify_output
            );

            let llm_result = llm_chat_with_events(
                llm.as_ref(),
                &bus,
                FIX_SYSTEM,
                &user_prompt,
                &trace_id,
                &session_id,
            )
            .await;

            let fix_plan_str = if llm_result.success {
                llm_result.output
            } else {
                return NodeResult::err(format!("LLM 生成修复方案失败: {}", llm_result.output));
            };

            // 3. 解析修复方案
            let plan = match parse_diff_plan(&fix_plan_str) {
                Ok(p) => p,
                Err(e) => {
                    return NodeResult::err(format!("解析修复方案失败: {}", e));
                }
            };

            // 4. 执行修复操作
            let mut fix_results: Vec<String> = Vec::new();
            let mut fix_errors: Vec<String> = Vec::new();
            let mut has_fix = false;

            for edit in &plan.edit_files {
                let tool_name = format!("fix_file:{}", edit.path);
                bus.emit_with_trace(
                    AgentEventData::ToolCalling {
                        tool_name: tool_name.clone(),
                        args: serde_json::json!({
                            "path": edit.path,
                            "operation": edit.operation,
                        }),
                    },
                    &trace_id,
                    Some(session_id.clone()),
                )
                .await;

                let start = std::time::Instant::now();
                let result = apply_edit_to_file(&project_path, edit);
                let elapsed = start.elapsed().as_millis() as u64;

                match result {
                    Ok(desc) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: desc.clone(),
                                success: true,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        fix_results.push(desc);
                        has_fix = true;
                    }
                    Err(e) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: e.clone(),
                                success: false,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        fix_errors.push(format!("{}: {}", edit.path, e));
                    }
                }
            }

            // 创建新文件
            for new_file in &plan.new_files {
                let tool_name = format!("create_file:{}", new_file.path);
                bus.emit_with_trace(
                    AgentEventData::ToolCalling {
                        tool_name: tool_name.clone(),
                        args: serde_json::json!({"path": new_file.path}),
                    },
                    &trace_id,
                    Some(session_id.clone()),
                )
                .await;

                let start = std::time::Instant::now();
                let result = create_new_file(&project_path, new_file);
                let elapsed = start.elapsed().as_millis() as u64;

                match result {
                    Ok(desc) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: desc.clone(),
                                success: true,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        fix_results.push(format!("创建 {}", desc));
                        has_fix = true;
                    }
                    Err(e) => {
                        bus.emit_with_trace(
                            AgentEventData::ToolResponded {
                                tool_name: tool_name.clone(),
                                result: e.clone(),
                                success: false,
                                duration_ms: elapsed,
                            },
                            &trace_id,
                            Some(session_id.clone()),
                        )
                        .await;
                        fix_errors.push(format!("创建 {}: {}", new_file.path, e));
                    }
                }
            }

            // 5. 递增 fix_count
            let current_count = state
                .get("fix_count")
                .unwrap_or_default()
                .parse::<i64>()
                .unwrap_or(0);
            let new_count = current_count + 1;

            // 6. 组装结果
            let mut output = String::new();
            output.push_str(&format!("🔄 第 {} 次修复尝试\n\n", new_count));
            if !plan.summary.is_empty() {
                output.push_str(&format!("📋 修复方案：{}\n\n", plan.summary));
            }
            if !fix_results.is_empty() {
                output.push_str("✅ 已执行：\n");
                for r in &fix_results {
                    output.push_str(&format!("  - {}\n", r));
                }
            }
            if !fix_errors.is_empty() {
                output.push_str("\n❌ 修复失败：\n");
                for e in &fix_errors {
                    output.push_str(&format!("  - {}\n", e));
                }
            }
            if !has_fix && fix_errors.is_empty() {
                output.push_str("⚠️  修复方案中没有需要执行的操作。");
            }

            // 清理之前的 verify 状态，让 verify 节点重新检查
            let mut updates: HashMap<String, serde_json::Value> = HashMap::new();
            updates.insert(
                "fix_results".to_string(),
                serde_json::Value::String(fix_results.join("\n")),
            );
            updates.insert(
                "fix_errors".to_string(),
                serde_json::Value::String(fix_errors.join("\n")),
            );
            updates.insert(
                "fix_summary".to_string(),
                serde_json::Value::String(plan.summary.clone()),
            );
            updates.insert(
                "fix_count".to_string(),
                serde_json::Value::Number(serde_json::Number::from(new_count)),
            );
            // 清除上一次的 verify 结果，强制重新检查
            updates.insert(
                "verify_success".to_string(),
                serde_json::Value::String(String::new()),
            );
            updates.insert(
                "verify_output".to_string(),
                serde_json::Value::String(String::new()),
            );

            NodeResult::ok_with_state(output, updates)
        })
    }
}

/// 创建节点：完成（纯组装输出）
fn make_complete_node(
) -> impl Fn(GraphState) -> std::pin::Pin<Box<dyn std::future::Future<Output = NodeResult> + Send>>
       + Send
       + Sync
       + 'static {
    |state: GraphState| {
        Box::pin(async move {
            let project_path = state.get("project_path").unwrap_or_default();
            let diff_plan = state.get("diff_plan").unwrap_or_default();
            let project_analysis = state.get("project_analysis").unwrap_or_default();
            let edit_results = state.get("edit_results").unwrap_or_default();
            let edit_errors = state.get("edit_errors").unwrap_or_default();
            let edit_summary = state.get("edit_summary").unwrap_or_default();
            let verify_success = state.get("verify_success").unwrap_or_default();
            let verify_output = state.get("verify_output").unwrap_or_default();
            let fix_results = state.get("fix_results").unwrap_or_default();
            let fix_errors = state.get("fix_errors").unwrap_or_default();
            let fix_summary = state.get("fix_summary").unwrap_or_default();
            let fix_count = state.get("fix_count").unwrap_or_default();

            let mut summary = String::new();

            // 编译验证结果
            if verify_success == "true" {
                summary.push_str("✅ 编译验证通过\n");
            } else if !verify_success.is_empty() {
                summary.push_str("❌ 编译验证失败");
                if !fix_count.is_empty() && fix_count != "0" {
                    summary.push_str(&format!("（已尝试修复 {} 次）", fix_count));
                }
                summary.push('\n');
            }

            summary.push_str(&format!("\n📋 项目路径：{}\n", project_path));

            if !edit_summary.is_empty() {
                summary.push_str(&format!("📝 修改内容：{}\n", edit_summary));
            }

            if !edit_results.is_empty() {
                summary.push_str("\n✅ 已执行的文件操作：\n");
                for line in edit_results.lines() {
                    summary.push_str(&format!("  - {}\n", line));
                }
            }

            if !edit_errors.is_empty() {
                summary.push_str("\n❌ 编辑失败的操作：\n");
                for line in edit_errors.lines() {
                    summary.push_str(&format!("  - {}\n", line));
                }
            }

            // 修复结果
            if !fix_summary.is_empty() {
                summary.push_str(&format!("\n🔧 修复方案：{}\n", fix_summary));
            }
            if !fix_results.is_empty() {
                summary.push_str("\n✅ 已执行的修复操作：\n");
                for line in fix_results.lines() {
                    summary.push_str(&format!("  - {}\n", line));
                }
            }
            if !fix_errors.is_empty() {
                summary.push_str("\n❌ 修复失败的操作：\n");
                for line in fix_errors.lines() {
                    summary.push_str(&format!("  - {}\n", line));
                }
            }

            if !verify_output.is_empty() && verify_success != "true" {
                summary.push_str(&format!("\n🔍 编译输出：\n{}\n", verify_output));
            }

            if edit_results.is_empty()
                && edit_errors.is_empty()
                && verify_success.is_empty()
                && fix_results.is_empty()
            {
                summary.push_str(&format!(
                    "\n📐 项目分析：\n{}\n\n📝 修改计划：\n{}",
                    &project_analysis.chars().take(500).collect::<String>(),
                    &diff_plan.chars().take(500).collect::<String>(),
                ));
            }

            NodeResult::ok(summary)
        })
    }
}

// ─── 公开 API ──────────────────────────────────────────────────

/// 创建 Rust 项目编辑工作流图
///
/// 完整工作流：analyze → diff_plan → edit_files → verify
///                                            ↓ success → complete
///                                            ↓ failure → fix_errors → verify (loop, max 3次)
pub fn create_rust_edit_graph(llm: Arc<dyn LLM>, bus: Arc<EventBus>) -> Graph {
    let analyze_node = make_analyze_node(llm.clone(), bus.clone());
    let diff_plan_node = make_diff_plan_node(llm.clone(), bus.clone());
    let edit_files_node = make_edit_files_node(bus.clone());
    let verify_node = make_verify_node(bus.clone());
    let fix_errors_node = make_fix_errors_node(llm.clone(), bus.clone());
    let complete_node = make_complete_node();

    GraphBuilder::new()
        .name("rust_edit")
        .node("analyze", analyze_node)
        .node_tag("analyze", vec!["analysis".to_string(), "rust".to_string()])
        .node("diff_plan", diff_plan_node)
        .node_tag(
            "diff_plan",
            vec!["planning".to_string(), "rust".to_string()],
        )
        .node("edit_files", edit_files_node)
        .node_tag("edit_files", vec!["file_io".to_string()])
        .node("verify", verify_node)
        .node_tag(
            "verify",
            vec!["verification".to_string(), "rust".to_string()],
        )
        .node("fix_errors", fix_errors_node)
        .node_tag(
            "fix_errors",
            vec!["debugging".to_string(), "rust".to_string()],
        )
        .node("complete", complete_node)
        .node_tag("complete", vec!["reporting".to_string()])
        .edge("analyze", "diff_plan")
        .edge("diff_plan", "edit_files")
        .edge("edit_files", "verify")
        // verify 条件边：成功 → complete，失败 → fix_errors（最多 3 次）
        .conditional_edge("verify", |state| {
            let success = state.get("verify_success").unwrap_or_default();
            if success == "true" {
                subhuti_core::graph::Route::To("complete".to_string())
            } else {
                let fix_count = state
                    .get("fix_count")
                    .unwrap_or_default()
                    .parse::<i64>()
                    .unwrap_or(0);
                if fix_count >= 3 {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Warn,
                        "已尝试修复 3 次仍未通过，强制结束",
                        None,
                    );
                    subhuti_core::graph::Route::To("complete".to_string())
                } else {
                    subhuti_core::graph::Route::To("fix_errors".to_string())
                }
            }
        })
        .edge("fix_errors", "verify")
        .reducer("fix_count", reducers::max())
        .entry("analyze")
        .max_iterations(10)
        .build()
        .expect("rust_edit 图构建失败，请检查节点定义")
}
