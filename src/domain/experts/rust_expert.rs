//! # Rust 编程专家
//!
//! 专属 Rust 编程专家：大模型基座 + 知识库（设计范式+个人习惯） + 工具链验证闭环。
//!
//! ## 四层知识库
//! 1. **通用 Rust 基础**：标准库 API、常用 crate 最佳实践
//! 2. **设计范式库**：六边形架构、装饰器、仓储、事件总线、DDD 聚合根等
//! 3. **个人编码规范**：命名、错误处理、异步约定、内存习惯
//! 4. **项目上下文**：静态项目结构说明
//!
//! ## 生成流程（验证修复闭环）
//! 需求拆解 → 知识检索(内嵌) → 约束注入 → LLM生成 → cargo check → 错误修复 → 循环直到通过

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::domain::traits::{
    DomainContext, DomainError, DomainExecutionContext, DomainExpert, DomainMessage, DomainResult,
    DomainRole, DomainSkill,
};

/// Rust 编程专家
///
/// 嵌入 4 层知识库作为 system prompt，支持代码生成/审查/修复/重构。
/// 编译验证通过 DomainExecutionContext.toolchain 完成。
pub struct RustExpert {
    skills: Vec<DomainSkill>,
    tags: Vec<String>,
}

impl RustExpert {
    pub fn new() -> Self {
        Self {
            tags: vec![
                "rust".to_string(),
                "code".to_string(),
                "programming".to_string(),
                "architecture".to_string(),
                "planning".to_string(),
                "coding".to_string(),
                "generation".to_string(),
                "file_io".to_string(),
                "verification".to_string(),
                "debugging".to_string(),
                "analysis".to_string(),
                "reporting".to_string(),
            ],
            skills: vec![
                DomainSkill {
                    id: "rust-chat".to_string(),
                    name: "自由对话".to_string(),
                    description: "与 Rust 编程专家自由对话，回答 Rust 相关问题和一般性咨询"
                        .to_string(),
                    parameters: vec!["question: 用户的问题或咨询内容（必填）".to_string()],
                },
                DomainSkill {
                    id: "rust-coding".to_string(),
                    name: "项目编码".to_string(),
                    description: "在项目工作目录中执行完整的编码任务：创建项目、生成代码、编译验证、修复错误。需要先在聊天设置中配置「项目工作目录」"
                        .to_string(),
                    parameters: vec!["task: 编码任务描述（必填）".to_string()],
                },
                DomainSkill {
                    id: "rust-generate".to_string(),
                    name: "生成代码".to_string(),
                    description: "根据需求描述生成完整的 Rust 代码，遵守六边形架构和编码规范"
                        .to_string(),
                    parameters: vec!["requirement: 需求描述（必填）".to_string()],
                },
                DomainSkill {
                    id: "rust-review".to_string(),
                    name: "代码审查".to_string(),
                    description: "审查 Rust 代码，检查架构、错误处理、命名规范等".to_string(),
                    parameters: vec!["code: 待审查的代码（必填）".to_string()],
                },
                DomainSkill {
                    id: "rust-fix".to_string(),
                    name: "修复编译错误".to_string(),
                    description: "分析编译错误并生成修复代码".to_string(),
                    parameters: vec!["code_with_errors: 代码+编译错误信息（必填）".to_string()],
                },
                DomainSkill {
                    id: "rust-refactor".to_string(),
                    name: "代码重构".to_string(),
                    description: "将代码重构为符合六边形架构和编码规范的版本".to_string(),
                    parameters: vec!["code: 待重构的代码（必填）".to_string()],
                },
            ],
        }
    }

    // ─── 查询类型判断 ────────────────────────────────────────────

    /// 判断是否为编码相关查询
    fn is_coding_query(task: &str) -> bool {
        let lower = task.to_lowercase();
        let keywords = [
            "代码",
            "编程",
            "开发",
            "写代码",
            "code",
            "programming",
            "rust",
            "创建",
            "编译",
            "运行",
            "构建",
            "测试",
            "实现",
            "函数",
            "bug",
            "错误",
            "安装",
            "配置",
            "依赖",
            "库",
            "框架",
            "接口",
            "api",
            "模块",
            "struct",
            "fn",
            "cargo",
            "src",
            "main.rs",
            "lib.rs",
            "package",
            "toml",
            "项目",
            "工程",
            "应用",
            "程序",
            "脚本",
            "命令行",
            "cli",
            "部署",
            "执行",
            "调用",
            "编码",
            "生成",
            "写",
            "修改",
            "编辑",
            "重构",
            "debug",
            "fix",
            "修复",
            "添加",
            "创建项目",
        ];
        keywords.iter().any(|kw| lower.contains(kw))
    }

    // ─── 四层知识库：内嵌为系统提示词 ────────────────────────────

    /// 构建完整的系统提示词
    fn build_system_prompt(&self) -> String {
        format!(
            "{}\n\n{}\n\n{}\n\n{}",
            self.layer1_rust_basics(),
            self.layer2_design_patterns(),
            self.layer3_personal_conventions(),
            self.layer4_project_context()
        )
    }

    /// 第一层：通用 Rust 基础知识
    fn layer1_rust_basics(&self) -> String {
        r#"## 第一层：Rust 基础知识与最佳实践

### 常用组件
- **异步运行时**: 使用 tokio，spawn 任务用 `tokio::spawn`
- **序列化**: serde + serde_json，所有 DTO 派生 `Serialize, Deserialize`
- **错误处理**: thiserror 定义领域错误，应用层用 anyhow
- **共享状态**: Arc<T> 跨线程共享，Mutex/RwLock 保护可变数据
- **Trait object**: `Arc<dyn Trait>` 作为依赖注入载体
- **异步 trait**: 手动 Pin<Box<dyn Future>> 或使用 async_trait

### 避免事项
- 禁止在业务代码中随意 unwrap/expect，用 Result 传播
- 禁止在领域层使用 println! / eprintln!，统一用 tracing
- 禁止不必要的 String::clone()，优先借用 &str
- 禁止在 async fn 中 hold MutexGuard 跨越 .await 点
"#
        .to_string()
    }

    /// 第二层：设计范式库
    fn layer2_design_patterns(&self) -> String {
        r#"## 第二层：设计模式范式库

### 范式 1：六边形分层架构
- 领域层 (`domain/`): 纯业务逻辑，定义 trait 和 DTO，不依赖外部框架
- 应用层 (`application/`): 用例编排，实现入站端口，依赖领域出站端口
- 适配层 (`adapter/`): 协议转换（inbound 接收请求，outbound 对接外部）
- 依赖方向: adapter → application → domain

### 范式 2：入站端口装饰器
- 场景: 给业务用例加横切逻辑（trace、鉴权、限流）
- 结构: 装饰器持有 `inner: Arc<dyn Port>`，实现同一个 Port trait
- 方法内做前置/后置逻辑，转发调用 inner
- 边界: 只适合切面逻辑，嵌套不超过 3 层

### 范式 3：事件总线与观察者
- 出站适配层订阅框架 EventBus → 转换事件 → 喂给应用层 TraceObserverPort
- TraceAppService 管理 trace 生命周期，TraceEventBridge 追加细粒度 span
- 通过 trace_id 串联

### 范式 4：仓储模式
- Port: `trait XxxRepositoryPort { fn get_by_id(...) }`
- 实现: PostgreSQL 适配器 + 内存降级适配器
- 组合根选择具体实现

### 范式 5：DDD 聚合根与领域事件
- 聚合根负责一致性边界，只通过标识引用其他聚合
- 领域事件在聚合根方法内收集，通过 EventBus 发布
"#
        .to_string()
    }

    /// 第三层：个人编码规范（强制约束）
    fn layer3_personal_conventions(&self) -> String {
        r#"## 第三层：个人编码规范（强制约束）

### 架构约束
- 业务代码严格遵守六边形分层，入站/出站端口分离
- 领域层不依赖任何基础设施（数据库、HTTP、框架）
- 应用层不包含具体的适配逻辑，只做端口调度

### 命名规范
- 端口 trait: XxxPort 后缀（ChatPort、SkillPort、ToolchainPort）
- 应用服务: XxxService 后缀（OrchestrationService）
- 装饰器: TraceXxx 前缀（TraceAppService）
- 适配器: XxxAdapter 后缀（DomainExpertAdapter、RustToolchainAdapter）
- 领域实体: 不强制后缀，用语义化命名（Expert、Skill）

### 错误处理规范
- 领域层用自定义 enum DomainError
- 应用层将 DomainError 翻译为响应 DTO
- 适配层只做错误格式转换，不加业务逻辑

### 异步约定
- trait 中异步方法统一返回 `Pin<Box<dyn Future<Output = T> + Send>>`
- 不使用 #[async_trait] 宏以保持代码透明

### 文件组织
- 每个领域专家一个文件: `domain/experts/{name}.rs`
- 适配器端口实现一个文件: `adapter/outbound/{name}_adapter.rs`
- 组合根只由 composition_root.rs 负责
"#
        .to_string()
    }

    /// 第四层：项目上下文（静态结构说明）
    fn layer4_project_context(&self) -> String {
        r#"## 第四层：当前项目上下文

### 项目结构
```
src/
├── domain/           # 领域层（纯业务逻辑）
│   ├── dto.rs        # 数据传输对象
│   ├── ports.rs      # 出站端口定义
│   ├── traits.rs     # 领域 trait（DomainExpert, DomainLlm）
│   └── experts/      # 领域专家实现
├── application/      # 应用层（用例编排）
│   ├── ports.rs      # 入站端口定义
│   ├── orchestration_service.rs
│   ├── trace_decorator.rs
│   ├── composition_root.rs
│   └── observer.rs
└── adapter/          # 适配层（协议转换）
    ├── inbound/      # 入站适配器（HTTP handler）
    └── outbound/     # 出站适配器（框架集成、数据库、工具链）
```

### 关键 trait
- domain::traits::DomainExpert: 领域专家核心接口
- domain::traits::DomainExecutionContext: 专家执行上下文
- domain::traits::DomainSkill: 技能定义
- application::ports::*: 所有入站端口
"#
        .to_string()
    }

    // ─── 技能实现 ────────────────────────────────────────────────

    /// 技能: rust-chat — 自由对话（非编码请求）
    async fn skill_chat(
        &self,
        exec_ctx: DomainExecutionContext,
        question: &str,
    ) -> DomainResult<String> {
        // 如果用户自定义了 system_prompt，优先使用
        let sys = if let Some(ref custom) = exec_ctx.ctx.system_prompt {
            custom.clone()
        } else {
            // 闲聊模式：使用轻量级系统提示词，不加载完整知识库
            "你是 Rust 编程专家，擅长 Rust 语言、系统编程、Web 后端、架构设计等领域。\
             \n\n请用简洁、专业的方式回答用户的问题。如果用户问的是 Rust 相关问题，请给出详细解答。\
             \n如果用户只是打招呼或闲聊，请友好回应但保持专业。\
             \n\n注意：除非用户明确要求生成代码，否则不要主动生成大量代码，先以对话方式回答。"
                .to_string()
        };

        // 构建消息列表：system + 历史 + 当前问题
        let mut messages = Vec::new();

        // 1. 添加系统提示
        messages.push(DomainMessage {
            role: DomainRole::System,
            content: sys,
        });

        // 2. 添加历史消息（排除历史中的 System 消息，避免重复）
        for msg in &exec_ctx.ctx.history {
            if msg.role != DomainRole::System {
                messages.push(msg.clone());
            }
        }

        // 3. 添加当前用户问题
        messages.push(DomainMessage {
            role: DomainRole::User,
            content: question.to_string(),
        });

        tracing::info!(
            "skill_chat: 历史消息数={}, 当前问题={}",
            exec_ctx.ctx.history.len(),
            question
        );

        exec_ctx.llm.chat(messages).await
    }

    /// 技能: rust-coding — 在项目工作目录中执行完整编码任务
    ///
    /// 流程：生成计划 → 展示待办 → 创建代码 → 写入文件 → cargo check → 修复
    async fn skill_coding(
        &self,
        mut exec_ctx: DomainExecutionContext,
        task: &str,
    ) -> DomainResult<String> {
        // ── 智能路由：判断是否为编码查询 ──────────────────────
        // 如果不是编码相关的查询，走聊天模式
        if !Self::is_coding_query(task) {
            tracing::info!("检测到非编码查询，转为聊天模式: {}", task);
            return self.skill_chat(exec_ctx, task).await;
        }

        let ws = exec_ctx
            .ctx
            .workspace_folder
            .as_ref()
            .ok_or_else(|| {
                DomainError::ExecutionError(
                    "未配置项目工作目录。请在聊天设置中填写「项目工作目录」路径。".to_string(),
                )
            })?
            .clone();

        let fs = exec_ctx
            .file_system
            .as_ref()
            .ok_or_else(|| DomainError::ExecutionError("文件系统端口未注入".to_string()))?
            .clone();
        let cmd = exec_ctx
            .command
            .as_ref()
            .ok_or_else(|| DomainError::ExecutionError("命令执行端口未注入".to_string()))?
            .clone();
        let llm = exec_ctx.llm.clone();

        let mut output = String::new();
        let progress_tx = exec_ctx.progress_tx.clone();

        // 辅助函数：推送进度到 SSE 流
        let send_progress = |tx: &Option<mpsc::Sender<String>>, msg: String| {
            if let Some(sender) = tx {
                // 非阻塞发送，若通道满则丢弃（不影响主流程）
                let _ = sender.try_send(msg);
            }
        };

        // ── 0. 检查项目状态 ──────────────────────────────────────
        let has_project = fs.exists(&format!("{}/Cargo.toml", &ws)).await;
        let existing_files = fs.search_files("**/*.rs", &ws).await.unwrap_or_default();

        output.push_str(&format!("📂 **项目目录**: `{}`\n\n", ws));
        if !has_project {
            output.push_str("🆕 项目尚未创建，需要执行 `cargo init` 初始化\n\n");
        } else {
            output.push_str(&format!(
                "📁 项目已存在，现有 Rust 文件: {} 个\n\n",
                existing_files.len()
            ));
        }

        // ── 1. LLM 生成计划（markdown 清单） ────────────────────
        output.push_str("## 📋 执行计划\n\n");
        let plan_prompt = format!(
            "你是一个 Rust 编程专家，正在项目目录 `{}` 中工作。\n\n\
            当前项目状态：\n\
            - Cargo.toml 存在: {}\n\
            - 已存在的 Rust 文件: {}\n\n\
            用户任务：{}\n\n\
            请先制定一个详细的执行计划，以 markdown 任务清单（`- [ ]`）格式列出每一步。\n\
            例如：\n\
            - [ ] 创建项目结构（Cargo.toml + src/main.rs）\n\
            - [ ] 实现核心逻辑\n\
            - [ ] 编译验证\n\n\
            注意：只输出计划本身，不要输出代码。",
            ws,
            if has_project { "是" } else { "否" },
            existing_files.join(", "),
            task,
        );
        let plan = llm
            .chat(self.build_messages(
                "你是一个项目规划专家，负责制定清晰的执行计划。",
                &plan_prompt,
            ))
            .await?;
        output.push_str(&plan);
        output.push_str("\n\n---\n\n## ⚡ 执行过程\n\n");

        // 推送计划到前端（让用户在执行前看到待办列表）
        let plan_json = serde_json::json!({
            "type": "plan",
            "message": plan,
        })
        .to_string();
        send_progress(&progress_tx, plan_json);

        // 解析计划中的 - [ ] 项，用于后续更新状态
        let plan_items: Vec<String> = plan
            .lines()
            .filter(|l| l.trim().starts_with("- [ ]"))
            .map(|l| l.trim().to_string())
            .collect();
        let plan_total = plan_items.len();
        let mut next_item = 0usize;

        // 辅助函数：标记未完成的计划项并返回标记数量
        // count=Some(n): 标记下 n 项（用于中间阶段展示进度）
        // count=None: 标记全部剩余（用于最终完成阶段）
        let mark_done =
            |out: &mut String, next: &mut usize, count: Option<usize>, items: &[String]| -> usize {
                let mut marked = 0usize;
                let limit = count.unwrap_or(items.len());
                while *next < items.len() && marked < limit {
                    let old = &items[*next];
                    let new = old.replace("- [ ]", "- [x]");
                    *out = out.replace(old, &new);
                    *next += 1;
                    marked += 1;
                }
                marked
            };

        // 辅助函数：推送当前执行状态到前端
        let push_progress = |tx: &Option<mpsc::Sender<String>>,
                             out: &String,
                             step_msg: &str,
                             done: usize,
                             total: usize| {
            let progress_json = serde_json::json!({
                "type": "step",
                "message": step_msg,
                "todo_state": out,
                "done_count": done,
                "total_count": total,
            })
            .to_string();
            send_progress(tx, progress_json);
        };

        // ── 2. 如果项目不存在，先 cargo init ─────────────────────
        if !has_project {
            mark_done(&mut output, &mut next_item, Some(1), &plan_items);
            push_progress(
                &progress_tx,
                &output,
                "初始化项目...",
                next_item,
                plan_total,
            );
            let init = cmd
                .run_command(
                    "cargo",
                    &["init", "--name", "my_project", &ws].map(String::from),
                    &ws,
                )
                .await
                .map_err(|e| DomainError::ExecutionError(format!("cargo init 失败: {}", e)))?;
            output.push_str(&format!("> `cargo init`: exit={}\n", init.exit_code));
            if !init.stderr.is_empty() {
                let stderr = &init.stderr[..init.stderr.len().min(200)];
                output.push_str(&format!("> stderr: `{}`\n", stderr));
            }
        }

        // ── 3. LLM 生成代码 ─────────────────────────────────────
        mark_done(&mut output, &mut next_item, Some(1), &plan_items);
        push_progress(&progress_tx, &output, "生成代码...", next_item, plan_total);
        let sys_prompt = format!(
            "你是一个 Rust 编程专家，正在项目目录 `{}` 中工作。\n\
             \n当前项目状态：\n\
             - Cargo.toml 存在: {}\n\
             - 已存在的 Rust 文件: {}\n\
             \n你的任务：{}\n\
             \n## 输出格式\n\
             生成代码时，每个文件用以下格式标记：\n\
             \nFile: path/to/file.rs\n\
             ```rust\n\
             // 代码内容\n\
             ```\n\
             \n如果项目还不存在，需要生成 Cargo.toml 和 src/main.rs。\n\
             \n## 要求\n\
             1. 每个文件必须用 File: 路径 标记\n\
             2. 代码块用 ```rust 标注\n\
             3. 如果修改已有文件，要输出完整的文件内容（不要省略）\n\
             4. 完成所有修改后，给出 `cargo check` 可以执行的命令提示",
            ws,
            if has_project { "是" } else { "否" },
            existing_files.join(", "),
            task,
        );
        let user_prompt = format!(
            "请为项目 `{}` 实现以下功能：\n\n{}\n\n输出所有需要创建或修改的文件，每个文件用 File: 路径 标记。",
            ws, task
        );
        let llm_response = llm
            .chat(self.build_messages(&sys_prompt, &user_prompt))
            .await?;

        // ── 4. 写入文件 ─────────────────────────────────────────
        mark_done(&mut output, &mut next_item, Some(1), &plan_items);
        push_progress(&progress_tx, &output, "写入文件...", next_item, plan_total);
        let written = self
            .write_files_from_llm_output(&ws, &llm_response, &fs)
            .await;
        match written {
            Ok(files) => {
                for f in &files {
                    output.push_str(&format!("> 📄 `{}`\n", f));
                }
                output.push_str(&format!("> 共写入 {} 个文件\n", files.len()));
            }
            Err(e) => {
                output.push_str(&format!("> ⚠️ 部分文件写入失败: {}\n", e));
            }
        }

        // ── 5. 编译验证 ─────────────────────────────────────────
        // 最终阶段：标记全部剩余计划项为完成
        mark_done(&mut output, &mut next_item, None, &plan_items);
        push_progress(&progress_tx, &output, "编译验证...", next_item, plan_total);
        let check = cmd
            .run_command("cargo", &["check"].map(String::from), &ws)
            .await
            .map_err(|e| DomainError::ExecutionError(format!("cargo check 失败: {}", e)))?;
        output.push_str(&format!("> `cargo check`: exit={}\n", check.exit_code));

        if check.exit_code == 0 {
            output.push_str("\n\n---\n\n## ✅ 任务完成\n\n所有步骤均已完成，项目已可正常编译。");
        } else {
            // 编译错误，尝试修复
            let stderr_snippet = if check.stderr.len() > 2000 {
                format!("{}...（截断）", &check.stderr[..2000])
            } else {
                check.stderr.clone()
            };
            output.push_str(&format!("> ⚠️ 编译错误:\n```\n{}\n```\n", stderr_snippet));

            let fix_prompt = format!(
                "以下 Rust 项目编译失败。\n\n编译错误：\n{}\n\n请分析错误原因并修复。输出所有需要修改的文件，每个文件用 File: 路径 标记。",
                stderr_snippet
            );
            let fix_response = llm
                .chat(self.build_messages(&sys_prompt, &fix_prompt))
                .await?;

            let written_fix = self
                .write_files_from_llm_output(&ws, &fix_response, &fs)
                .await;
            match written_fix {
                Ok(files) => {
                    for f in &files {
                        output.push_str(&format!("> 📄 修复 `{}`\n", f));
                    }
                }
                Err(e) => {
                    output.push_str(&format!("> ⚠️ 修复写入失败: {}\n", e));
                }
            }

            // 再次检查
            let check2 = cmd
                .run_command("cargo", &["check"].map(String::from), &ws)
                .await
                .map_err(|e| DomainError::ExecutionError(format!("cargo check 失败: {}", e)))?;
            output.push_str(&format!(
                "> `cargo check` (重试): exit={}\n",
                check2.exit_code
            ));
            if check2.exit_code == 0 {
                output.push_str("\n\n---\n\n## ✅ 任务完成\n\n编译错误已修复，项目可正常编译。");
            } else {
                output.push_str("\n\n---\n\n## ⚠️ 需要手动修复\n\n自动修复未能解决所有编译错误，请查看上方错误信息手动修复。");
            }
        }

        Ok(output)
    }

    /// 技能: rust-generate — 从需求生成代码（带验证循环）
    async fn skill_generate(
        &self,
        exec_ctx: DomainExecutionContext,
        requirement: &str,
    ) -> DomainResult<String> {
        // 如果用户自定义了 system_prompt，优先使用
        let sys = if let Some(ref custom) = exec_ctx.ctx.system_prompt {
            custom.clone()
        } else {
            self.build_system_prompt()
        };

        // 如果设置了 workspace_folder，附加到指令中
        let ws_hint = if let Some(ref ws) = exec_ctx.ctx.workspace_folder {
            format!("\n\n项目工作目录: {}\n如果项目已存在，请在指定目录下操作；如果首次生成，请按项目结构规划。", ws)
        } else {
            String::new()
        };

        let gen_instruction = format!(
            "你是 Rust 编程专家。根据以下需求生成完整、可编译的 Rust 代码。{ws_hint}\n\n\
             生成要求：\n\
             1. 严格遵守六边形架构分层\n\
             2. DTO 派生 Serialize/Deserialize\n\
             3. 不要省略任何必要的 use 语句\n\
             4. 用 ```rust 代码块标注文件路径\n\n\
             ---\n需求：{}",
            requirement,
        );

        self.generate_with_verify(exec_ctx, &sys, &gen_instruction, requirement)
            .await
    }

    /// 技能: rust-review — 审查代码
    async fn skill_review(
        &self,
        exec_ctx: DomainExecutionContext,
        code: &str,
    ) -> DomainResult<String> {
        let sys = format!(
            "{}\n\n你是 Rust 代码审查专家。检查：架构分层、错误处理、异步安全、命名一致性、不必要的 clone、潜在死锁。",
            self.build_system_prompt()
        );
        let msg = format!("请审查以下 Rust 代码：\n\n```rust\n{}\n```", code);
        exec_ctx.llm.chat(self.build_messages(&sys, &msg)).await
    }

    /// 技能: rust-fix — 修复编译错误
    async fn skill_fix(
        &self,
        exec_ctx: DomainExecutionContext,
        input: &str,
    ) -> DomainResult<String> {
        let sys = format!(
            "{}\n\n你是 Rust 编译错误修复专家。\n常见错误修复策略：\n\
             - E0597 (borrow lifetime): 减少不必要的借用\n\
             - E0382 (move): 使用 clone 或引用\n\
             - E0277 (trait bound): 添加必要的 trait 约束\n\
             - E0507 (move out): 使用 clone 或 ref",
            self.build_system_prompt()
        );
        let msg = format!("## 代码与编译错误\n\n{}", input);
        exec_ctx.llm.chat(self.build_messages(&sys, &msg)).await
    }

    /// 技能: rust-refactor — 重构代码符合范式
    async fn skill_refactor(
        &self,
        exec_ctx: DomainExecutionContext,
        code: &str,
    ) -> DomainResult<String> {
        let sys = format!(
            "{}\n\n你是 Rust 代码重构专家。将代码重构为符合六边形架构和编码规范的版本。\n\
             目标：分离端口定义和实现、引入 trait 抽象、使用 Arc<dyn Port> 依赖注入、添加文档注释。",
            self.build_system_prompt()
        );
        let msg = format!("请重构以下 Rust 代码：\n\n```rust\n{}\n```", code);
        exec_ctx.llm.chat(self.build_messages(&sys, &msg)).await
    }

    // ─── 验证修复闭环 ─────────────────────────────────────────────

    /// 代码生成 + 编译验证 + 错误修复循环
    async fn generate_with_verify(
        &self,
        exec_ctx: DomainExecutionContext,
        sys: &str,
        instruction: &str,
        requirement: &str,
    ) -> DomainResult<String> {
        let max_retries = 3;
        let mut current_req = instruction.to_string();

        for retry in 0..=max_retries {
            // 1. 调用 LLM 生成代码
            let response = exec_ctx
                .llm
                .chat(self.build_messages(sys, &current_req))
                .await?;

            // 2. 如果没有工具链，直接返回
            let toolchain = match &exec_ctx.toolchain {
                Some(t) => t.clone(),
                None => {
                    return Ok(format!(
                        "✅ 代码已生成（未启用编译验证，设置 toolchain 以启用自动验证）\n\n{}",
                        response
                    ));
                }
            };

            // 3. 运行 cargo check 验证
            let result = toolchain.check("").await;
            if result.success {
                return Ok(format!(
                    "✅ 编译通过（{} 轮验证）\n\n{}\n\n---\n## 编译检查\n{}",
                    retry + 1,
                    response,
                    result.output
                ));
            }

            // 4. 达到最大重试次数
            if retry >= max_retries {
                return Ok(format!(
                    "⚠️ 编译未通过（已重试 {} 次）\n\n## 生成的代码\n{}\n\n## 编译错误\n{}",
                    max_retries,
                    response,
                    result.errors.join("\n")
                ));
            }

            // 5. 用错误信息重新请求 LLM 修复
            let code = self.extract_code(&response);
            current_req = format!(
                "原始需求：{}\n\n上次生成的代码：\n```rust\n{}\n```\n\n编译错误（请修复，只输出修复后的完整代码）：\n{}",
                requirement, code,
                result.errors.join("\n")
            );
        }

        Err(DomainError::ExecutionError(
            "代码生成验证循环异常退出".to_string(),
        ))
    }

    /// 从 LLM 响应中提取代码块
    fn extract_code(&self, response: &str) -> String {
        let mut code = String::new();
        let mut in_code_block = false;
        for line in response.lines() {
            if line.trim().starts_with("```") {
                if in_code_block {
                    break;
                }
                in_code_block = true;
                continue;
            }
            if in_code_block {
                code.push_str(line);
                code.push('\n');
            }
        }
        if code.is_empty() {
            response.to_string()
        } else {
            code
        }
    }

    /// 构建消息列表（system + user）
    fn build_messages(&self, sys: &str, user: &str) -> Vec<DomainMessage> {
        vec![
            DomainMessage {
                role: DomainRole::System,
                content: sys.to_string(),
            },
            DomainMessage {
                role: DomainRole::User,
                content: user.to_string(),
            },
        ]
    }

    /// 从 LLM 输出中解析 File: 标记的文件并写入磁盘
    ///
    /// 支持格式：
    /// ```text
    /// File: src/main.rs
    /// ```rust
    /// fn main() {}
    /// ```
    /// ```
    async fn write_files_from_llm_output(
        &self,
        workspace: &str,
        llm_output: &str,
        fs: &Arc<dyn crate::domain::ports::FileSystemPort>,
    ) -> Result<Vec<String>, String> {
        let mut written = Vec::new();
        let mut remaining = llm_output;

        loop {
            // 查找 "File: " 标记
            let file_start = match remaining.find("File: ") {
                Some(pos) => pos,
                None => break,
            };
            remaining = &remaining[file_start + 6..];

            // 提取文件路径
            let line_end = remaining.find('\n').unwrap_or(remaining.len());
            let file_path = remaining[..line_end].trim();
            remaining = &remaining[line_end..];

            // 查找代码块开始
            let block_start = match remaining.find("```") {
                Some(pos) => pos,
                None => break,
            };
            remaining = &remaining[block_start + 3..];

            // 跳过语言标注行（如 ```rust）
            if let Some(lang_end) = remaining.find('\n') {
                remaining = &remaining[lang_end + 1..];
            }

            // 查找代码块结束
            let block_end = match remaining.find("```") {
                Some(pos) => pos,
                None => break,
            };
            let content = remaining[..block_end].to_string();
            remaining = &remaining[block_end + 3..];

            // 写入文件
            // 如果 file_path 已经是绝对路径，直接使用；否则拼接到 workspace
            let full_path = if file_path.starts_with('/') {
                file_path.to_string()
            } else {
                format!("{}/{}", workspace, file_path)
            };
            fs.write_file(&full_path, &content)
                .await
                .map_err(|e| format!("写入文件 {} 失败: {}", file_path, e))?;
            written.push(file_path.to_string());
        }

        if written.is_empty() {
            // 没有 File: 标记，尝试提取第一个代码块作为 src/main.rs
            let main_path = format!("{}/src/main.rs", workspace);
            let has_main = fs.exists(&main_path).await;
            if !has_main {
                // 查找第一个 ```rust 代码块
                if let Some(start) = llm_output.find("```rust") {
                    let after = &llm_output[start + 7..];
                    if let Some(end) = after.find("```") {
                        let content = after[..end].trim().to_string();
                        fs.write_file(&main_path, &content)
                            .await
                            .map_err(|e| format!("写入 src/main.rs 失败: {}", e))?;
                        written.push("src/main.rs".to_string());
                    }
                }
            }
        }

        Ok(written)
    }
}

// ─── 输入分类 ────────────────────────────────────────────────

/// 检测输入是否为编码/技术请求
///
/// 通过关键词匹配判断用户意图，返回 false 表示走闲聊模式。
/// 避免将"你好"、"你是谁"等非编码请求送入代码生成流程。
fn is_coding_request(input: &str) -> bool {
    let input_lower = input.to_lowercase();
    let char_count = input.chars().count();

    // 明确排除的闲聊/问候/身份陈述模式
    let chat_patterns = [
        "你好",
        "您好",
        "hello",
        "hi ",
        "你是谁",
        "你叫什么",
        "你能做什么",
        "在吗",
        "在不在",
        "谢谢",
        "感谢",
        "再见",
        "拜拜",
        "bye",
        "goodbye",
        "早上好",
        "下午好",
        "晚上好",
        "help",
        "帮助",
        "你是什么",
        "who are you",
        "what can you do",
        // 身份相关（第一人称陈述/询问）
        "我是",
        "我叫",
        "我名字",
        "我的名字",
        "我是谁",
        "我叫什么",
        "我是谁",
        "介绍",
        "自我介绍",
        "认识",
        "很高兴",
        "见到",
    ];
    for p in &chat_patterns {
        if input_lower.contains(p) {
            return false;
        }
    }

    // 编码/技术关键词（匹配任意一个即视为编码请求）
    let code_keywords = [
        "生成",
        "写一个",
        "实现",
        "开发",
        "创建",
        "代码",
        "函数",
        "struct",
        "impl",
        "trait",
        "enum",
        "fn ",
        "cargo",
        "编译",
        "重构",
        "审查",
        "修改",
        "修复",
        "bug",
        "错误",
        "报错",
        "error",
        "怎么",
        "如何",
        "怎样",
        "什么",
        "区别",
        "对比",
        "原理",
        "架构",
        "设计模式",
        "六边形",
        "async",
        "tokio",
        "serde",
        "anyhow",
        "thiserror",
        "cli",
        "web",
        "http",
        "api",
        "数据库",
        "sql",
        "测试",
        "test",
        "性能",
        "优化",
        "配置",
        "部署",
    ];
    for kw in &code_keywords {
        if input_lower.contains(kw) {
            return true;
        }
    }

    // 默认：如果输入太短（<8个字符）且没有匹配到任何技术关键词，视为闲聊
    if char_count < 8 {
        return false;
    }

    // 较长输入默认视为技术问题
    true
}

// ─── DomainExpert 实现 ──────────────────────────────────────────

#[async_trait]
impl DomainExpert for RustExpert {
    fn id(&self) -> &str {
        "rust-expert"
    }

    fn name(&self) -> &str {
        "Rust 编程专家"
    }

    fn tags(&self) -> &[String] {
        &self.tags
    }

    fn skills(&self) -> &[DomainSkill] {
        &self.skills
    }

    async fn run(&self, exec_ctx: DomainExecutionContext) -> DomainResult<String> {
        // 如果有指定技能，分发到对应处理
        if let Some(ref skill_id) = exec_ctx.skill_id {
            let params = exec_ctx
                .skill_params
                .clone()
                .unwrap_or_else(|| exec_ctx.ctx.input.clone());
            let sid = skill_id.clone();
            self.execute_skill(&sid, &params, exec_ctx).await
        } else {
            // 默认：检测输入是否为编码请求
            let input = exec_ctx.ctx.input.clone();
            let has_ws = exec_ctx.ctx.workspace_folder.is_some()
                && exec_ctx.file_system.is_some()
                && exec_ctx.command.is_some();

            if is_coding_request(&input) && has_ws {
                // 有工作目录 + 文件系统/命令端口 → 使用项目编码技能
                self.skill_coding(exec_ctx, &input).await
            } else if is_coding_request(&input) {
                // 无工作目录 → 纯代码生成（不操作文件系统）
                self.skill_generate(exec_ctx, &input).await
            } else {
                // 非编码请求 → 闲聊
                self.skill_chat(exec_ctx, &input).await
            }
        }
    }

    async fn execute_skill(
        &self,
        skill_id: &str,
        params: &str,
        exec_ctx: DomainExecutionContext,
    ) -> DomainResult<String> {
        match skill_id {
            "rust-chat" => self.skill_chat(exec_ctx, params).await,
            "rust-coding" => self.skill_coding(exec_ctx, params).await,
            "rust-generate" => self.skill_generate(exec_ctx, params).await,
            "rust-review" => self.skill_review(exec_ctx, params).await,
            "rust-fix" => self.skill_fix(exec_ctx, params).await,
            "rust-refactor" => self.skill_refactor(exec_ctx, params).await,
            _ => {
                // 未知技能，走默认 run
                let new_ctx = DomainExecutionContext {
                    ctx: DomainContext {
                        input: format!("技能ID: {}, 参数: {}", skill_id, params),
                        ..exec_ctx.ctx
                    },
                    ..exec_ctx
                };
                self.run(new_ctx).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::outbound::command_adapter::LocalCommandAdapter;
    use crate::adapter::outbound::file_system_adapter::LocalFileSystemAdapter;
    use crate::domain::ports::CommandPort;
    use crate::domain::ports::FileSystemPort;

    #[test]
    fn test_knowledge_layers_not_empty() {
        let expert = RustExpert::new();
        assert!(!expert.layer1_rust_basics().is_empty());
        assert!(!expert.layer2_design_patterns().is_empty());
        assert!(!expert.layer3_personal_conventions().is_empty());
        assert!(!expert.layer4_project_context().is_empty());
    }

    #[test]
    fn test_extract_code_block() {
        let expert = RustExpert::new();
        let response = "```rust\nfn main() {}\n```";
        assert_eq!(expert.extract_code(response), "fn main() {}\n");

        let no_code = "plain text response";
        assert_eq!(expert.extract_code(no_code), "plain text response");
    }

    #[test]
    fn test_has_six_skills() {
        let expert = RustExpert::new();
        assert_eq!(expert.skills().len(), 6);
    }

    #[test]
    fn test_is_coding_request() {
        // 闲聊/问候 → 非编码请求
        assert!(!is_coding_request("你好"));
        assert!(!is_coding_request("你是谁"));
        assert!(!is_coding_request("hello"));
        assert!(!is_coding_request("谢谢"));
        assert!(!is_coding_request("hi"));
        // 身份陈述 → 非编码请求
        assert!(!is_coding_request("我是张三"));
        assert!(!is_coding_request("我叫李四"));
        assert!(!is_coding_request("我的名字叫王五"));
        assert!(!is_coding_request("我是谁"));
        // 太短的输入 → 非编码请求
        assert!(!is_coding_request("啊"));
        assert!(!is_coding_request("ok"));
        // 编码/技术请求 → 编码请求
        assert!(is_coding_request("生成一个Rust函数"));
        assert!(is_coding_request("帮我写一个web服务器"));
        assert!(is_coding_request("如何实现async trait"));
        assert!(is_coding_request("这个代码报错怎么修复"));
        assert!(is_coding_request("Rust的六边形架构怎么设计"));
    }

    // ─── write_files_from_llm_output 测试 ────────────────────────

    /// 测试解析 LLM 输出中的 File: 标记并写入文件
    #[tokio::test]
    async fn test_write_files_from_llm_output_parses_file_markers() {
        let expert = RustExpert::new();
        let fs: Arc<dyn FileSystemPort> = Arc::new(LocalFileSystemAdapter::new());
        let tmp = std::env::temp_dir().join("subhuti-test-write-files");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        // 模拟LLM输出：多个File:标记文件
        let llm_output = r#"我将创建一个简单的 Rust 项目。

File: src/main.rs
```rust
fn main() {
    println!("Hello, world!");
}
```

File: Cargo.toml
```toml
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
```

File: src/lib.rs
```rust
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
```
"#;

        let ws = tmp.to_string_lossy().to_string();
        let result = expert
            .write_files_from_llm_output(&ws, llm_output, &fs)
            .await;

        assert!(result.is_ok(), "写入文件失败: {:?}", result.err());
        let written = result.unwrap();
        assert_eq!(written.len(), 3, "应该写入3个文件");

        // 验证文件内容
        let main_rs = fs.read_file(&format!("{}/src/main.rs", &ws)).await.unwrap();
        assert!(main_rs.contains("Hello, world!"), "main.rs 内容不符");

        let cargo_toml = fs.read_file(&format!("{}/Cargo.toml", &ws)).await.unwrap();
        assert!(cargo_toml.contains("test-project"), "Cargo.toml 内容不符");

        let lib_rs = fs.read_file(&format!("{}/src/lib.rs", &ws)).await.unwrap();
        assert!(lib_rs.contains("pub fn add"), "lib.rs 内容不符");

        // 清理
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    /// 测试没有 File: 标记时，自动提取第一个代码块作为 src/main.rs
    #[tokio::test]
    async fn test_write_files_from_llm_output_fallback_to_main_rs() {
        let expert = RustExpert::new();
        let fs: Arc<dyn FileSystemPort> = Arc::new(LocalFileSystemAdapter::new());
        let tmp = std::env::temp_dir().join("subhuti-test-fallback");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        tokio::fs::create_dir_all(&tmp.join("src")).await.unwrap();

        // 没有 File: 标记，只有代码块
        let llm_output = r#"Here's the code:
```rust
fn main() {
    println!("Hello from fallback!");
}
```"#;

        let ws = tmp.to_string_lossy().to_string();
        let result = expert
            .write_files_from_llm_output(&ws, llm_output, &fs)
            .await;

        assert!(result.is_ok(), "回退写入失败: {:?}", result.err());
        let written = result.unwrap();
        assert_eq!(written.len(), 1, "应该回退写入1个文件");
        assert_eq!(written[0], "src/main.rs", "应该写入 src/main.rs");

        // 验证内容
        let content = fs.read_file(&format!("{}/src/main.rs", &ws)).await.unwrap();
        assert!(content.contains("Hello from fallback!"));

        // 清理
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    /// 测试空的 LLM 输出
    #[tokio::test]
    async fn test_write_files_from_llm_output_empty_output() {
        let expert = RustExpert::new();
        let fs: Arc<dyn FileSystemPort> = Arc::new(LocalFileSystemAdapter::new());
        let tmp = std::env::temp_dir().join("subhuti-test-empty");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let ws = tmp.to_string_lossy().to_string();
        let result = expert.write_files_from_llm_output(&ws, "", &fs).await;

        assert!(result.is_ok(), "空输出应返回 Ok");
        assert!(result.unwrap().is_empty(), "空输出应写入0个文件");

        // 清理
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    // ─── 文件系统适配器测试 ──────────────────────────────────────

    /// 测试文件系统适配器在项目目录中的基本操作
    #[tokio::test]
    async fn test_file_system_adapter_crud() {
        let fs = LocalFileSystemAdapter::new();
        let test_dir = std::env::temp_dir().join("subhuti-test-fs-adapter");
        let _ = tokio::fs::remove_dir_all(&test_dir).await;
        tokio::fs::create_dir_all(&test_dir).await.unwrap();
        let ws = test_dir.to_string_lossy().to_string();

        // 1. 写入文件
        fs.write_file(&format!("{}/test.txt", &ws), "Hello, FS!")
            .await
            .unwrap();
        assert!(fs.exists(&format!("{}/test.txt", &ws)).await);

        // 2. 读取文件
        let content = fs.read_file(&format!("{}/test.txt", &ws)).await.unwrap();
        assert_eq!(content, "Hello, FS!");

        // 3. 创建目录并写入嵌套文件
        fs.write_file(&format!("{}/src/lib.rs", &ws), "pub fn foo() {}")
            .await
            .unwrap();
        assert!(fs.exists(&format!("{}/src/lib.rs", &ws)).await);

        // 4. 搜索文件
        let rs_files = fs.search_files("**/*.rs", &ws).await.unwrap();
        assert_eq!(rs_files.len(), 1);
        assert!(rs_files[0].ends_with("src/lib.rs"));

        // 5. 列出目录
        let entries = fs.list_dir(&ws).await.unwrap();
        assert!(entries.contains(&"test.txt".to_string()));
        assert!(entries.contains(&"src".to_string()));

        // 6. 删除文件
        fs.delete_file(&format!("{}/test.txt", &ws)).await.unwrap();
        assert!(!fs.exists(&format!("{}/test.txt", &ws)).await);

        // 清理
        let _ = tokio::fs::remove_dir_all(&test_dir).await;
    }

    // ─── 命令执行适配器测试 ──────────────────────────────────────

    /// 测试命令执行适配器在项目目录中的基本操作
    #[tokio::test]
    async fn test_command_adapter_basic() {
        let cmd = LocalCommandAdapter::new();
        let test_dir = std::env::temp_dir().join("subhuti-test-cmd-adapter");
        let _ = tokio::fs::remove_dir_all(&test_dir).await;
        tokio::fs::create_dir_all(&test_dir).await.unwrap();
        let ws = test_dir.to_string_lossy().to_string();

        // 1. 测试简单命令 (echo)
        let result = cmd
            .run_command("echo", &["hello", "world"].map(String::from), &ws)
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world"));

        // 2. 测试 cargo init 创建项目
        let init = cmd
            .run_command(
                "cargo",
                &["init", "--name", "test-project", &ws].map(String::from),
                &ws,
            )
            .await
            .unwrap();
        assert_eq!(init.exit_code, 0, "cargo init 失败: {}", init.stderr);
        assert!(tokio::fs::try_exists(test_dir.join("Cargo.toml"))
            .await
            .unwrap());

        // 3. 测试 cargo check
        let check = cmd
            .run_command("cargo", &["check"].map(String::from), &ws)
            .await
            .unwrap();
        assert_eq!(check.exit_code, 0, "cargo check 失败: {}", check.stderr);

        // 清理
        let _ = tokio::fs::remove_dir_all(&test_dir).await;
    }
}
