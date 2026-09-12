//! # 默认临时技能（L3 降级）装配
//!
//! 方案 B：降级是执行链的内部机制，默认藏在 `plan_and_execute` 里，专家零感知。
//!
//! 本模块在**领域层自实现 `StepFallback`**（`DomainLlmFallback`）：它以与普通技能
//! 完全一致的上下文（原始输入 / 自定义 system_prompt / 工作目录 / 失败反馈）为兜底，
//! 让 LLM 直接完成失败步骤并输出与普通技能相同的结果文本。它只在需要读写文件/运行
//! 命令时（已注入 file_system / command port）才附带工具；否则走**纯文本兜底**。
//!
//! - 引擎侧：只提供纯机制的 `execute_plan_adaptive` + `StepFallback` 接口
//! - 领域侧：本模块把 `exec_ctx` 的 port 渲染成 `ToolInfo`、把上下文装配给 LLM
//!
//! 递归防护：降级提示词明确禁止生成计划/调用专家接口，工具列表仅含 file_/command_，
//! 实现层永不触碰 `generate_plan` / `plan_and_execute` / `execute_skill`。

use std::sync::Arc;

use crate::domain::ports::{CommandPort, FileSystemPort};
use crate::domain::traits::DomainExecutionContext;

/// 把 `exec_ctx` 中的可用 port 渲染成 LLM 可见的工具 schema（function-calling）
///
/// 只暴露已注入的 port：file_system（读写/列/搜文件）与 command（跑 shell 命令）。
/// 哪个没注入就只提供已注入的那些，避免调用不存在的能力。
pub fn build_tools(exec_ctx: &DomainExecutionContext) -> Vec<subhuti_core::ToolInfo> {
    use subhuti_core::ToolInfo;

    let mut tools: Vec<ToolInfo> = Vec::new();
    if exec_ctx.file_system.is_some() {
        tools.extend([
            ToolInfo::from_name_and_desc(
                "file_read",
                "读取指定路径文件的内容",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "要读取的文件绝对路径" }
                    },
                    "required": ["path"]
                }),
            ),
            ToolInfo::from_name_and_desc(
                "file_write",
                "把内容写入指定路径文件（自动创建父目录，相同内容则跳过）",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "目标文件绝对路径" },
                        "content": { "type": "string", "description": "要写入的文件内容" }
                    },
                    "required": ["path", "content"]
                }),
            ),
            ToolInfo::from_name_and_desc(
                "file_list",
                "列出指定目录下的文件名（不递归）",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "目录绝对路径" }
                    },
                    "required": ["path"]
                }),
            ),
            ToolInfo::from_name_and_desc(
                "file_search",
                "按 glob 模式搜索文件名",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "glob 模式，如 **/*.rs" },
                        "root": { "type": "string", "description": "搜索根目录" }
                    },
                    "required": ["pattern"]
                }),
            ),
        ]);
    }
    if exec_ctx.command.is_some() {
        tools.push(ToolInfo::from_name_and_desc(
            "command_run",
            "在指定工作目录执行 shell 命令（如 cargo、git），返回 stdout/stderr/退出码",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "命令名，如 cargo、git" },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "命令参数" },
                    "cwd": { "type": "string", "description": "工作目录" }
                },
                "required": ["command"]
            }),
        ));
    }
    tools
}

/// 领域层工具执行器：按 LLM 返回的工具名 + JSON 参数调用真实 port
struct DomainToolExecutor {
    fs: Option<Arc<dyn FileSystemPort>>,
    command: Option<Arc<dyn CommandPort>>,
}

impl subhuti_core::ToolExecutor for DomainToolExecutor {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: serde_json::Value,
    ) -> subhuti_core::BoxFuture<'a, subhuti_core::Result<String>> {
        Box::pin(async move {
            let fs = self.fs.clone();
            let command = self.command.clone();
            let err = |m: String| subhuti_core::Error::Expert(m);

            // 智谱/OpenAI 兼容接口的工具 arguments 常是「JSON 字符串」（而非对象），
            // 存成 Value 后 .get() 取不到键。这里统一归一化为对象：
            //  - 字符串 → 尝试解析为 JSON；解析失败则视为空参数
            //  - 对象 → 直接用
            //  - 其它（null/数组）→ 空对象（字段缺失走各自的「缺少 xxx 参数」报错）
            let args: serde_json::Value = match &args {
                serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s)
                    .unwrap_or_else(|_| serde_json::json!({})),
                serde_json::Value::Object(_) => args,
                _ => serde_json::json!({}),
            };

            match name {
                "file_read" => {
                    let path = args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| err("file_read 缺少 path 参数".to_string()))?
                        .to_string();
                    let fs = fs.ok_or_else(|| err("file_system 未注入".to_string()))?;
                    fs.read_file(&path)
                        .await
                        .map_err(|e| err(format!("file_read 失败: {e}")))
                }
                "file_write" => {
                    let path = args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| err("file_write 缺少 path 参数".to_string()))?
                        .to_string();
                    let content = args
                        .get("content")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| err("file_write 缺少 content 参数".to_string()))?
                        .to_string();
                    let fs = fs.ok_or_else(|| err("file_system 未注入".to_string()))?;
                    fs.write_file(&path, &content)
                        .await
                        .map_err(|e| err(format!("file_write 失败: {e}")))
                        .map(|_| "写入成功".to_string())
                }
                "file_list" => {
                    let path = args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| err("file_list 缺少 path 参数".to_string()))?
                        .to_string();
                    let fs = fs.ok_or_else(|| err("file_system 未注入".to_string()))?;
                    let items = fs
                        .list_dir(&path)
                        .await
                        .map_err(|e| err(format!("file_list 失败: {e}")))?;
                    Ok(items.join("\n"))
                }
                "file_search" => {
                    let pattern = args
                        .get("pattern")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| err("file_search 缺少 pattern 参数".to_string()))?
                        .to_string();
                    let root = args
                        .get("root")
                        .and_then(|v| v.as_str())
                        .unwrap_or(".")
                        .to_string();
                    let fs = fs.ok_or_else(|| err("file_system 未注入".to_string()))?;
                    let found = fs
                        .search_files(&pattern, &root)
                        .await
                        .map_err(|e| err(format!("file_search 失败: {e}")))?;
                    Ok(found.join("\n"))
                }
                "command_run" => {
                    // LLM 生成的参数形态多样：可能 command 是命令名+args 数组，
                    // 也可能整条命令写进 command/line/cmd 字符串。这里做健壮兼容。
                    let raw: Option<String> = ["command", "cmd", "line"]
                        .iter()
                        .find_map(|k| args.get(*k).and_then(|v| v.as_str()))
                        .map(|s| s.to_string());

                    // 从 args 字段抽取参数（兼容数组或字符串）
                    let raw_args: Vec<String> = args
                        .get("args")
                        .map(|v| {
                            if let Some(arr) = v.as_array() {
                                arr.iter()
                                    .filter_map(|x| x.as_str())
                                    .map(|s| s.to_string())
                                    .collect()
                            } else if let Some(s) = v.as_str() {
                                // 空格拆分成参数，简单 shell 分词
                                s.split_whitespace().map(|x| x.to_string()).collect()
                            } else {
                                Vec::new()
                            }
                        })
                        .unwrap_or_default();

                    let (cmd, arg_list) = match raw {
                        Some(c) => {
                            // 整条命令行拆分成 命令名 + 参数
                            let mut parts: Vec<String> =
                                c.split_whitespace().map(|x| x.to_string()).collect();
                            if parts.is_empty() {
                                return Err(err("command_run 缺少 command 参数".to_string()));
                            }
                            let cmd = parts.remove(0);
                            // 命令行内的参数优先；若 args 数组还带了额外参数再追加
                            let mut arg_list = parts;
                            for a in raw_args {
                                if !arg_list.contains(&a) {
                                    arg_list.push(a);
                                }
                            }
                            (cmd, arg_list)
                        }
                        None => {
                            // 无 command 字段：尝试 args[0] 当命令名
                            if raw_args.is_empty() {
                                return Err(err("command_run 缺少 command 参数".to_string()));
                            }
                            let mut rest = raw_args.clone();
                            let cmd = rest.remove(0);
                            (cmd, rest)
                        }
                    };

                    let cwd = args
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .unwrap_or(".")
                        .to_string();
                    let command_ref = command.ok_or_else(|| err("command 未注入".to_string()))?;
                    let out = command_ref
                        .run_command(&cmd, &arg_list, &cwd)
                        .await
                        .map_err(|e| err(format!("command_run 失败: {e}")))?;
                    Ok(format!(
                        "exit={}\nstdout:\n{}\nstderr:\n{}",
                        out.exit_code, out.stdout, out.stderr
                    ))
                }
                other => Err(err(format!("未知工具: {}", other))),
            }
        })
    }
}

/// 领域层自实现的 L3 降级兜底器
///
/// 与普通技能共享同一套上下文（原始输入 / 自定义 system_prompt / 工作目录），
/// 失败时让 LLM 直接完成该步骤并输出普通的「结果文本」。
/// - 有工具（已注入 file_system / command）→ 走多轮 function-calling
/// - 无工具 → 纯文本一次性生成
///
/// 任何情况下都**不**生成计划、不调用技能/专家接口，杜绝递归。
struct DomainLlmFallback {
    llm: Arc<dyn subhuti_core::LLM>,
    /// 可为空；空则退化为纯文本兜底
    tools: Vec<subhuti_core::ToolInfo>,
    executor: Option<Arc<dyn subhuti_core::ToolExecutor>>,
    expert_name: String,
    system_prompt: Option<String>,
    user_input: String,
    workspace: Option<String>,
}

impl subhuti_core::StepFallback for DomainLlmFallback {
    fn execute<'a>(
        &'a self,
        step: &'a subhuti_core::PlanStep,
        failure_history: &'a [String],
    ) -> subhuti_core::BoxFuture<'a, subhuti_core::Result<String>> {
        Box::pin(async move {
            let goal = format!(
                "目标步骤：{}（skill_id={}）\n步骤参数：{}",
                step.description, step.skill_id, step.params
            );
            let orig = if self.user_input.trim().is_empty() {
                "（无）".to_string()
            } else {
                self.user_input.clone()
            };
            let ws = self.workspace.clone().unwrap_or_default();
            let history = if failure_history.is_empty() {
                "（无失败反馈）".to_string()
            } else {
                failure_history.join("\n")
            };

            // 递归防护写进提示词，与普通技能同一套（system_prompt / 原始诉求/工作目录）
            let constraint = "\
                【约束】请直接完成上面的步骤，并像专家正常输出结果一样，给出该步骤的结果文本。\n\
                - 不要生成执行计划，不要调用任何技能或专家接口，不要出现规划/调度/重试专家等行为。\n\
                - 若需读写文件或运行命令，可使用下方提供的工具；否则直接用文字输出结果。";
            let system = match &self.system_prompt {
                Some(c) if !c.trim().is_empty() => format!("{}\n\n{}", c.trim(), constraint),
                _ => format!(
                    "你是 {}，现在你的一项预定义技能多次执行失败，由你为这个步骤做兜底。\n\n{}",
                    self.expert_name, constraint
                ),
            };
            let user = format!(
                "{}\n\n用户原始诉求：\n{}\n\n工作目录：{}\n\n历史上技能失败反馈：\n{}",
                goal, orig, ws, history
            );

            let mut messages = vec![
                subhuti_core::Message {
                    role: subhuti_core::Role::System,
                    content: system,
                    tool_call_id: None,
                },
                subhuti_core::Message {
                    role: subhuti_core::Role::User,
                    content: user,
                    tool_call_id: None,
                },
            ];

            // 空工具 → 纯文本兜底：一次调用，直接取结果
            if self.tools.is_empty() {
                let out = self.llm.chat(messages).await?.trim().to_string();
                return Ok(if out.is_empty() {
                    "（降级兜底未产出内容）".to_string()
                } else {
                    out
                });
            }

            // 有工具 → 多轮 function-calling（与普通技能在同一上下文中补齐动作）
            let llm = self.llm.clone();
            let executor = self.executor.clone();
            for _ in 0..3 {
                let resp: subhuti_core::LLMResponse = llm
                    .chat_with_tools(messages.clone(), self.tools.clone())
                    .await?;
                if let Some(tc) = resp.tool_call {
                    let ex = executor
                        .as_ref()
                        .ok_or_else(|| subhuti_core::Error::Expert("工具执行器缺失".into()))?;
                    let out = ex.execute(&tc.name, tc.arguments).await?;
                    messages.push(subhuti_core::Message {
                        role: subhuti_core::Role::Tool,
                        content: out,
                        tool_call_id: Some(tc.id),
                    });
                    continue;
                }
                let out = resp.content.trim().to_string();
                return Ok(if out.is_empty() {
                    "（降级兜底未产出内容）".to_string()
                } else {
                    out
                });
            }
            Err(subhuti_core::Error::Expert(
                "L3 降级兜底达到迭代上限，仍未完成目标".into(),
            ))
        })
    }
}

/// 组装默认临时技能（L3 降级）
///
/// 只要有引擎 LLM 即可装配（`engine_llm`）：已注入 file_system / command port 时
/// 附带文件/命令工具，否则退化为纯文本兜底直接生成结果。供所有专家共享，专家零感知。
pub fn assemble_fallback(
    exec_ctx: &DomainExecutionContext,
    expert_name: &str,
) -> Option<Arc<dyn subhuti_core::StepFallback>> {
    let engine_llm = exec_ctx.engine_llm.clone()?;

    let tools = build_tools(exec_ctx);
    let executor = if tools.is_empty() {
        None
    } else {
        Some(Arc::new(DomainToolExecutor {
            fs: exec_ctx.file_system.clone(),
            command: exec_ctx.command.clone(),
        }) as Arc<dyn subhuti_core::ToolExecutor>)
    };

    Some(Arc::new(DomainLlmFallback {
        llm: engine_llm,
        tools,
        executor,
        expert_name: expert_name.to_string(),
        system_prompt: exec_ctx.ctx.system_prompt.clone(),
        user_input: exec_ctx.ctx.input.clone(),
        workspace: exec_ctx.ctx.workspace_folder.clone(),
    }))
}
