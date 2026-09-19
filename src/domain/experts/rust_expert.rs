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
//!
//! ## Workflow 归属（架构关键）
//! 多步流水线（analyze → plan → edit → verify → complete + 条件 fix）**完全下沉到专家内部**，
//! 由 `skill_coding` 以确定性代码驱动（仅生成/规划走 LLM）。框架不再持有任何 Graph，
//! 专家对编排层是黑盒：编排层只看到「调用了哪个专家」，看不到内部 Workflow。

use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::orchestrator::{FlowContext, FlowNodeResult, ReactStage};

use crate::domain::events::DomainEvent;
use crate::domain::flow_exec::{flow_flag, flow_str, run_react_flow, FlowKeyId, FlowNodeExecutor};
use crate::domain::traits::{
    chat_stream_to_progress, DomainContext, DomainError, DomainExecutionContext, DomainExpert,
    DomainMessage, DomainResult, DomainRole, DomainSkill,
};

// 技能种类（决定 Flow 各节点如何执行）
//
// 这是所有技能的**唯一注册表**：id / 名称 / 描述 / 参数 / 阶段 都从这里派生，
// `skills()`（元数据）与 `execute_skill`（调度）共用同一份定义，消除
// `DomainSkill.id` 与 `match skill_id` 字符串的多处手工同步。
//
// 由 [`skill_catalog!`] 宏从一张数据表生成；新增技能只需补一行。
skill_catalog!(
    RustSkillKind,
    stages, &'static [ReactStage],
    {
        Chat {
            id: "rust-chat",
            name: "自由对话",
            desc: "与 Rust 编程专家自由对话，回答 Rust 相关问题和一般性咨询",
            params: ["question: 用户的问题或咨询内容（必填）"],
            custom: &[ReactStage::Analyze, ReactStage::Edit, ReactStage::Done],
        },
        Coding {
            id: "rust-coding",
            name: "项目编码",
            desc: "在项目工作目录中执行完整的编码任务：创建项目、生成代码、编译验证、修复错误。需要先在聊天设置中配置「项目工作目录」",
            params: ["task: 编码任务描述（必填）"],
            custom: &[ReactStage::Analyze, ReactStage::Plan, ReactStage::Edit, ReactStage::Verify, ReactStage::Done],
        },
        Generate {
            id: "rust-generate",
            name: "生成代码",
            desc: "根据需求描述生成完整的 Rust 代码，遵守六边形架构和编码规范",
            params: ["requirement: 需求描述（必填）"],
            custom: &[ReactStage::Analyze, ReactStage::Edit, ReactStage::Verify, ReactStage::Done],
        },
        Review {
            id: "rust-review",
            name: "代码审查",
            desc: "审查 Rust 代码，检查架构、错误处理、命名规范等",
            params: ["code: 待审查的代码（必填）"],
            custom: &[ReactStage::Analyze, ReactStage::Edit, ReactStage::Done],
        },
        Fix {
            id: "rust-fix",
            name: "修复编译错误",
            desc: "分析编译错误并生成修复代码",
            params: ["code_with_errors: 代码+编译错误信息（必填）"],
            custom: &[ReactStage::Analyze, ReactStage::Edit, ReactStage::Done],
        },
        Refactor {
            id: "rust-refactor",
            name: "代码重构",
            desc: "将代码重构为符合六边形架构和编码规范的版本",
            params: ["code: 待重构的代码（必填）"],
            custom: &[ReactStage::Edit, ReactStage::Done],
        },
        SkillList {
            id: "rust-skill-list",
            name: "技能列表",
            desc: "查询并展示当前专家可用的所有技能列表及其详细说明",
            params: ["format: 输出格式（可选，支持 markdown/plain）"],
            custom: &[ReactStage::Done],
        },
        KnowledgeQuery {
            id: "rust-knowledge-query",
            name: "知识库查询",
            desc: "查询 Rust 专家的知识库内容，包括四层知识库（Rust基础、设计模式、编码规范、项目结构）",
            params: ["topic: 查询的主题或关键词（可选，为空则展示所有知识库目录）"],
            custom: &[ReactStage::Analyze, ReactStage::Edit, ReactStage::Done],
        },
    }
);

/// 把 RustExpert 以「Skill = Flow + Tool」模式挂到固定 React 模板上。
///
/// 持有专家本体（可访问其私有辅助方法）与当前技能种类；节点逻辑统一交由
/// `RustExpert::flow_pure / flow_llm` 按技能 + 节点分派。
struct RustFlowExecutor {
    skill: RustSkillKind,
    expert: Arc<RustExpert>,
}

#[async_trait]
impl FlowNodeExecutor for RustFlowExecutor {
    async fn execute_pure(
        &self,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        self.expert
            .flow_pure(self.skill, stage, exec_ctx, ctx)
            .await
    }

    async fn execute_llm(
        &self,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        self.expert.flow_llm(self.skill, stage, exec_ctx, ctx).await
    }
}

/// 编码类技能共享的中间结果 key（`FlowKeyId` / `flow_str` / `flow_flag`
/// 共用定义见 [`crate::domain::flow_exec`]）
#[derive(Clone, Copy)]
enum CodingKey {
    HasProject,
    Existing,
    Ws,
    Sys,
    VerifyOk,
    PlanItems,
}

impl FlowKeyId for CodingKey {
    fn as_str(&self) -> &'static str {
        match self {
            CodingKey::HasProject => "coding.has_project",
            CodingKey::Existing => "coding.existing",
            CodingKey::Ws => "coding.ws",
            CodingKey::Sys => "coding.sys",
            CodingKey::VerifyOk => "coding.verify_ok",
            CodingKey::PlanItems => "coding.plan_items",
        }
    }
}

/// 代码生成类技能中间结果 key
#[derive(Clone, Copy)]
enum GenerateKey {
    Sys,
    Instruction,
    LastCode,
}

impl FlowKeyId for GenerateKey {
    fn as_str(&self) -> &'static str {
        match self {
            GenerateKey::Sys => "generate.sys",
            GenerateKey::Instruction => "generate.instruction",
            GenerateKey::LastCode => "generate.last_code",
        }
    }
}

/// Rust 编程专家
///
/// 嵌入 4 层知识库作为 system prompt，支持代码生成/审查/修复/重构。
/// 编译验证通过 DomainExecutionContext.toolchain 完成。
///
/// 执行模型：**Skill = Flow + Tool**。各技能复用固定 React 模板
/// `analyze→plan→edit→verify→done`（按需启用阶段），节点处理逻辑 + 真实
/// 工具绑定由 `RustFlowExecutor`（FlowNodeExecutor）提供，`FlowRunner` 负责
/// 顺序、上下文传递与事件发射。
#[derive(Clone)]
pub struct RustExpert {
    skills: Vec<DomainSkill>,
    tags: Vec<String>,
}

impl Default for RustExpert {
    fn default() -> Self {
        Self::new()
    }
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
            // 技能元数据从唯一注册表 `RustSkillKind::ALL` 派生，不手写重复 id/name/desc
            skills: RustSkillKind::ALL.iter().map(|k| k.meta()).collect(),
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

    /// 构建完整的系统提示词（静态版本，作为 fallback）
    fn build_system_prompt(&self) -> String {
        format!(
            "{}\n\n{}\n\n{}\n\n{}",
            self.layer1_rust_basics(),
            self.layer2_design_patterns(),
            self.layer3_personal_conventions(),
            self.layer4_project_context()
        )
    }

    /// 从藏经阁动态加载知识库内容并构建系统提示词
    ///
    /// 优先从藏经阁加载知识库切片，如果加载失败则 fallback 到静态知识库
    async fn build_system_prompt_dynamic(&self, exec_ctx: &DomainExecutionContext) -> String {
        // 尝试从藏经阁加载知识库
        if let Some(sutra) = &exec_ctx.sutra_library {
            let expert_id = self.id();

            // 1. 根据 expert_id 获取关联的知识库
            let kb_result = sutra.get_knowledge_base_by_expert(expert_id).await;
            if !kb_result.contains("⚠️") && !kb_result.is_empty() {
                tracing::info!("从藏经阁加载知识库: expert_id={}", expert_id);

                // 解析知识库列表
                if let Ok(kbases) = serde_json::from_str::<Vec<serde_json::Value>>(&kb_result) {
                    if !kbases.is_empty() {
                        let mut all_content = String::new();

                        // 2. 遍历每个知识库，获取其切片内容
                        for kb in &kbases {
                            if let Some(kb_id) = kb.get("id").and_then(|v| v.as_str()) {
                                let kb_name = kb
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("未知知识库");
                                all_content.push_str(&format!("\n## 知识库: {}\n\n", kb_name));

                                let chunks_result = sutra.list_chunks(kb_id).await;
                                if let Ok(chunks) =
                                    serde_json::from_str::<Vec<serde_json::Value>>(&chunks_result)
                                {
                                    for chunk in &chunks {
                                        let title = chunk
                                            .get("title")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("未命名");
                                        let content = chunk
                                            .get("content")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        all_content
                                            .push_str(&format!("### {}\n{}\n\n", title, content));
                                    }
                                }
                            }
                        }

                        if !all_content.is_empty() {
                            tracing::info!(
                                "成功从藏经阁加载知识库内容: {} 字节",
                                all_content.len()
                            );
                            // 截断预算：防止知识库全量内容撑爆模型上下文窗口（触发 400 超限）
                            const MAX_CHARS: usize = 12_000;
                            if all_content.chars().count() > MAX_CHARS {
                                let cut_at = all_content
                                    .char_indices()
                                    .take(MAX_CHARS)
                                    .map(|(i, _)| i)
                                    .last()
                                    .unwrap_or(0);
                                all_content.truncate(cut_at);
                                all_content
                                    .push_str("\n\n[知识库内容已按 token 预算截断，如需更多资料请使用 rust-knowledge-query 技能按需召回]");
                                tracing::warn!(
                                    "知识库内容超过预算，已截断到 {} 字符",
                                    all_content.chars().count()
                                );
                            }
                            return all_content;
                        }
                    }
                }
            }
        }

        // 知识库为空时，退一步走藏经阁**记忆召回**。
        //
        // 这一步之前完全没有：Rust 专家只认知识库切片，既读不到会话沉淀的记忆，
        // 也导致 `last_retrieve_cache` 永远没有 Rust 领域的记录 → 反馈分析器
        // 统计出来的命中率恒为 0.00%，指标彻底失去意义。
        if let Some(sutra) = &exec_ctx.sutra_library {
            let recalled = sutra.library_retrieve(&exec_ctx.ctx.input, 3).await;
            if !recalled.contains("未找到")
                && !recalled.contains("⚠️")
                && !recalled.trim().is_empty()
            {
                tracing::info!(
                    "从藏经阁召回记忆: {} 字节（知识库无切片，改用记忆通路）",
                    recalled.len()
                );
                return format!(
                    "{}\n\n以下是藏经阁中与当前问题相关的历史记忆，请优先参考：\n{}",
                    self.build_system_prompt(),
                    recalled
                );
            }
        }

        // fallback: 使用静态知识库
        tracing::info!("藏经阁无可用记忆/知识，使用静态知识库");
        self.build_system_prompt()
    }

    /// 把四层静态知识导出为 (标题, 正文) 条目，供藏经阁冷启动灌入
    ///
    /// 这些知识原本只在 `build_system_prompt()` 里硬编码、每次请求原样拼进提示词，
    /// 既不能被检索（藏经阁里查不到），也不能被反馈/命中率度量。
    /// 导出来灌进藏经阁后，它们和会话沉淀的记忆走同一套检索与打分。
    pub fn static_knowledge_entries(&self) -> Vec<(String, String)> {
        vec![
            (
                "Rust 基础知识与最佳实践".to_string(),
                self.layer1_rust_basics(),
            ),
            ("Rust 设计模式".to_string(), self.layer2_design_patterns()),
            (
                "个人编码约定".to_string(),
                self.layer3_personal_conventions(),
            ),
            ("项目上下文".to_string(), self.layer4_project_context()),
        ]
    }

    /// 获取技能列表信息（用于注入到系统提示词中）
    fn format_skills_info(&self) -> String {
        let mut result = String::new();

        for skill in &self.skills {
            result.push_str(&format!(
                "- **{}** (ID: {}): {}\n",
                skill.name, skill.id, skill.description
            ));
        }

        result
    }

    /// 格式化技能列表用于直接响应（不依赖 LLM）
    fn format_skills_for_response(&self) -> String {
        let mut result = String::from("我是 Rust 编程专家，拥有以下技能：\n\n");

        for skill in &self.skills {
            result.push_str(&format!(
                "- **{}** (ID: `{}`): {}\n",
                skill.name, skill.id, skill.description
            ));
        }

        result.push_str("\n您可以告诉我使用哪个技能来完成任务。");
        result
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

    // ─── Flow 节点逻辑（Skill = Flow + Tool）───────────────────────

    /// 按技能 + 阶段构造执行器并跑固定 React 模板，返回累积产物
    async fn run_skill_flow(
        &self,
        skill: RustSkillKind,
        exec_ctx: DomainExecutionContext,
        stages: &[ReactStage],
    ) -> DomainResult<String> {
        let ex = Arc::new(RustFlowExecutor {
            skill,
            expert: Arc::new(self.clone()),
        });
        run_react_flow(&exec_ctx, "rust.react", stages, ex).await
    }

    /// 纯工具节点：按 React 阶段 + 技能分派（analyze / verify / done）
    async fn flow_pure(
        &self,
        skill: RustSkillKind,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        match stage {
            ReactStage::Analyze => match skill {
                RustSkillKind::Coding => self.flow_coding_analyze(exec_ctx, ctx).await,
                _ => Ok(FlowNodeResult {
                    output: String::new(),
                    success: true,
                }),
            },
            ReactStage::Verify => match skill {
                RustSkillKind::Coding => self.flow_coding_verify(exec_ctx, ctx).await,
                RustSkillKind::Generate => self.flow_generate_verify(exec_ctx, ctx).await,
                _ => Ok(FlowNodeResult {
                    output: String::new(),
                    success: true,
                }),
            },
            ReactStage::Done => match skill {
                RustSkillKind::Coding => self.flow_coding_done(ctx).await,
                RustSkillKind::SkillList => Ok(FlowNodeResult {
                    output: self.format_skills_for_response(),
                    success: true,
                }),
                _ => Ok(FlowNodeResult {
                    output: String::new(),
                    success: true,
                }),
            },
            ReactStage::Plan | ReactStage::Edit => {
                unreachable!("纯工具节点不能收到 plan/edit 阶段: {stage:?}")
            }
        }
    }

    /// LLM 决策节点：按 React 阶段 + 技能分派（plan / edit）
    async fn flow_llm(
        &self,
        skill: RustSkillKind,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        match stage {
            ReactStage::Plan => match skill {
                RustSkillKind::Coding => self.flow_coding_plan(exec_ctx, ctx).await,
                // 其余技能的 plan 仅为阶段占位（无 LLM 规划）
                _ => Ok(FlowNodeResult {
                    output: String::new(),
                    success: true,
                }),
            },
            ReactStage::Edit => match skill {
                RustSkillKind::Coding => self.flow_coding_edit(exec_ctx, ctx).await,
                RustSkillKind::Chat => self.flow_chat_edit(exec_ctx, ctx).await,
                RustSkillKind::Review => self.flow_lang_edit(exec_ctx, ctx, "review").await,
                RustSkillKind::Fix => self.flow_lang_edit(exec_ctx, ctx, "fix").await,
                RustSkillKind::Refactor => self.flow_lang_edit(exec_ctx, ctx, "refactor").await,
                RustSkillKind::KnowledgeQuery => self.flow_knowledge_edit(exec_ctx, ctx).await,
                RustSkillKind::Generate => self.flow_generate_edit(exec_ctx, ctx).await,
                RustSkillKind::SkillList => Err(DomainError::ExecutionError(
                    "技能 rust-skill-list 不支持 edit 阶段".to_string(),
                )),
            },
            ReactStage::Analyze | ReactStage::Verify | ReactStage::Done => {
                unreachable!("LLM 决策节点只能收到 plan/edit 阶段: {stage:?}")
            }
        }
    }

    // ── coding：analyze / plan / edit / verify / done ─────────────

    async fn flow_coding_analyze(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let ws = exec_ctx
            .ctx
            .workspace_folder
            .as_ref()
            .ok_or_else(|| {
                DomainError::Precondition(
                    "未配置项目工作目录。请在聊天设置中填写「项目工作目录」路径，或通过 extra.workspace_folder 传入。".to_string(),
                )
            })?
            .clone();
        let fs = exec_ctx.require_file_system()?.clone();
        let has_project = fs.exists(&format!("{}/Cargo.toml", &ws)).await;
        let existing = fs.search_files("**/*.rs", &ws).await.unwrap_or_default();
        // 状态写入 ctx.results，供后续 plan/edit/verify/done 使用
        ctx.results.insert(
            CodingKey::Ws.as_str().to_string(),
            FlowNodeResult {
                output: ws.clone(),
                success: true,
            },
        );
        ctx.results.insert(
            CodingKey::HasProject.as_str().to_string(),
            FlowNodeResult {
                output: if has_project { "true" } else { "false" }.to_string(),
                success: true,
            },
        );
        ctx.results.insert(
            CodingKey::Existing.as_str().to_string(),
            FlowNodeResult {
                output: existing.join(", "),
                success: true,
            },
        );
        let mut out = format!("📂 **项目目录**: `{}`\n\n", ws);
        if has_project {
            out.push_str(&format!(
                "📁 项目已存在，现有 Rust 文件: {} 个\n\n",
                existing.len()
            ));
        } else {
            out.push_str("🆕 项目尚未创建，需要执行 `cargo init` 初始化\n\n");
        }
        Ok(FlowNodeResult {
            output: out,
            success: true,
        })
    }

    async fn flow_coding_plan(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let has_project = flow_flag(ctx, CodingKey::HasProject);
        let existing = flow_str(ctx, CodingKey::Existing);
        let ws = flow_str(ctx, CodingKey::Ws);
        let skills_info = self.format_skills_info();
        let plan_prompt = format!(
            "你是一个 Rust 编程专家，正在项目目录 `{}` 中工作。\n\n\
             当前项目状态：\n\
             - Cargo.toml 存在: {}\n\
             - 已存在的 Rust 文件: {}\n\n\
             ## 你的可用技能\n{}\n\
             用户任务：{}\n\n\
             请先制定一个详细的执行计划，以 markdown 任务清单（`- [ ]`）格式列出每一步。\n\
             例如：\n\
             - [ ] 创建项目结构（Cargo.toml + src/main.rs）\n\
             - [ ] 实现核心逻辑\n\
             - [ ] 编译验证\n\n\
             注意：只输出计划本身，不要输出代码。",
            ws,
            if has_project { "是" } else { "否" },
            existing,
            skills_info,
            ctx.input,
        );
        let plan = exec_ctx
            .llm
            .chat(self.build_messages(
                "你是一个项目规划专家，负责制定清晰的执行计划。",
                &plan_prompt,
            ))
            .await?;
        let plan_items: Vec<String> = plan
            .lines()
            .filter(|l| l.trim().starts_with("- [ ]"))
            .map(|l| l.trim().to_string())
            .collect();
        ctx.results.insert(
            CodingKey::PlanItems.as_str().to_string(),
            FlowNodeResult {
                output: plan_items.join("|"),
                success: true,
            },
        );
        let out = format!("## 📋 执行计划\n\n{}\n\n---\n\n## ⚡ 执行过程\n\n", plan);
        Ok(FlowNodeResult {
            output: out,
            success: true,
        })
    }

    async fn flow_coding_edit(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let ws = flow_str(ctx, CodingKey::Ws);
        let has_project = flow_flag(ctx, CodingKey::HasProject);
        let existing = flow_str(ctx, CodingKey::Existing);
        let cmd = exec_ctx.require_command()?.clone();
        let fs = exec_ctx.require_file_system()?.clone();
        let skills_info = self.format_skills_info();
        let mut out = String::new();
        if !has_project {
            let init = cmd
                .run_command(
                    "cargo",
                    &["init", "--name", "my_project", &ws].map(String::from),
                    &ws,
                )
                .await
                .map_err(|e| DomainError::ExecutionError(format!("cargo init 失败: {}", e)))?;
            out.push_str(&format!("> `cargo init`: exit={}\n", init.exit_code));
            if !init.stderr.is_empty() {
                let len = init.stderr.len().min(200);
                out.push_str(&format!("> stderr: `{}`\n", &init.stderr[..len]));
            }
        }
        let sys_prompt = format!(
            "你是一个 Rust 编程专家，正在项目目录 `{}` 中工作。\n\
             \n当前项目状态：\n\
             - Cargo.toml 存在: {}\n\
             - 已存在的 Rust 文件: {}\n\
             \n## 你的可用技能\n{}\n\
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
            existing,
            skills_info,
            ctx.input,
        );
        ctx.results.insert(
            CodingKey::Sys.as_str().to_string(),
            FlowNodeResult {
                output: sys_prompt.clone(),
                success: true,
            },
        );
        let user_prompt = format!(
            "请为项目 `{}` 实现以下功能：\n\n{}\n\n输出所有需要创建或修改的文件，每个文件用 File: 路径 标记。",
            ws, ctx.input
        );
        let llm_response = exec_ctx
            .llm
            .chat(self.build_messages(&sys_prompt, &user_prompt))
            .await?;
        match self
            .write_files_from_llm_output(&ws, &llm_response, &fs)
            .await
        {
            Ok(files) => {
                for f in &files {
                    out.push_str(&format!("> 📄 `{}`\n", f));
                }
                out.push_str(&format!("> 共写入 {} 个文件\n", files.len()));
            }
            Err(e) => {
                out.push_str(&format!("> ⚠️ 部分文件写入失败: {}\n", e));
            }
        }
        Ok(FlowNodeResult {
            output: out,
            success: true,
        })
    }

    async fn flow_coding_verify(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let ws = flow_str(ctx, CodingKey::Ws);
        let sys = flow_str(ctx, CodingKey::Sys);
        let cmd = exec_ctx.require_command()?.clone();
        let fs = exec_ctx.require_file_system()?.clone();
        let toolchain = exec_ctx.toolchain.clone();
        const MAX_FIX_RETRIES: usize = 3;
        let mut out = String::new();
        for attempt in 0..=MAX_FIX_RETRIES {
            let check = if let Some(tc) = &toolchain {
                let r = tc.check(&ws).await;
                Some((r.success, r.errors.join("\n")))
            } else {
                None
            };
            let (ok, detail) = match check {
                Some((success, errors)) => {
                    if success {
                        (true, "> `cargo check` (toolchain): 通过\n".to_string())
                    } else {
                        (false, format!("> ⚠️ 编译错误:\n```\n{}\n```\n", errors))
                    }
                }
                None => {
                    let r = cmd
                        .run_command("cargo", &["check"].map(String::from), &ws)
                        .await
                        .map_err(|e| {
                            DomainError::ExecutionError(format!("cargo check 失败: {}", e))
                        })?;
                    if r.exit_code == 0 {
                        (true, format!("> `cargo check`: exit={}\n", r.exit_code))
                    } else {
                        let stderr_snippet = if r.stderr.len() > 2000 {
                            format!("{}...（截断）", &r.stderr[..2000])
                        } else {
                            r.stderr.clone()
                        };
                        (
                            false,
                            format!("> ⚠️ 编译错误:\n```\n{}\n```\n", stderr_snippet),
                        )
                    }
                }
            };
            if ok {
                ctx.results.insert(
                    CodingKey::VerifyOk.as_str().to_string(),
                    FlowNodeResult {
                        output: "true".to_string(),
                        success: true,
                    },
                );
                out.push_str(&detail);
                return Ok(FlowNodeResult {
                    output: out,
                    success: true,
                });
            }
            out.push_str(&detail);
            if attempt >= MAX_FIX_RETRIES {
                ctx.results.insert(
                    CodingKey::VerifyOk.as_str().to_string(),
                    FlowNodeResult {
                        output: "false".to_string(),
                        success: true,
                    },
                );
                return Ok(FlowNodeResult {
                    output: out,
                    success: true,
                });
            }
            let fix_prompt = format!(
                "以下 Rust 项目（目录 `{}`）编译失败。\n\n编译错误：\n{}\n\n请分析错误原因并修复。输出所有需要修改的文件，每个文件用 File: 路径 标记。",
                ws, detail
            );
            let fix_response = exec_ctx
                .llm
                .chat(self.build_messages(&sys, &fix_prompt))
                .await?;
            match self
                .write_files_from_llm_output(&ws, &fix_response, &fs)
                .await
            {
                Ok(files) => {
                    for f in &files {
                        out.push_str(&format!("> 📄 修复 `{}`\n", f));
                    }
                }
                Err(e) => {
                    out.push_str(&format!("> ⚠️ 修复写入失败: {}\n", e));
                }
            }
        }
        Ok(FlowNodeResult {
            output: out,
            success: true,
        })
    }

    async fn flow_coding_done(&self, ctx: &mut FlowContext) -> DomainResult<FlowNodeResult> {
        ctx.output = ctx.output.replace("- [ ]", "- [x]");
        let ok = flow_flag(ctx, CodingKey::VerifyOk);
        let footer = if ok {
            "\n\n---\n\n## ✅ 任务完成\n\n所有步骤均已完成，项目已可正常编译。"
        } else {
            "\n\n---\n\n## ⚠️ 需要手动修复\n\n自动修复未能解决所有编译错误，请查看上方错误信息手动修复。"
        };
        Ok(FlowNodeResult {
            output: footer.to_string(),
            success: true,
        })
    }

    // ── chat：单轮自由对话 ────────────────────────────────────────

    async fn flow_chat_edit(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let question = &ctx.input;
        let is_knowledge_q = is_knowledge_query(question);
        let sys = if let Some(ref custom) = exec_ctx.ctx.system_prompt {
            custom.clone()
        } else if is_knowledge_q {
            tracing::info!("检测到知识库查询，从藏经阁动态加载知识库");
            self.build_system_prompt_dynamic(exec_ctx).await
        } else {
            "你是 Rust 编程专家，擅长 Rust 语言、系统编程、Web 后端、架构设计等领域。\
             \n\n请用简洁、专业的方式回答用户的问题。如果用户问的是 Rust 相关问题，请给出详细解答。\
             \n如果用户只是打招呼或闲聊，请友好回应但保持专业。\
             \n\n注意：除非用户明确要求生成代码，否则不要主动生成大量代码，先以对话方式回答。"
                .to_string()
        };
        let mut messages = vec![DomainMessage {
            role: DomainRole::System,
            content: sys,
        }];
        for msg in &exec_ctx.ctx.history {
            if msg.role != DomainRole::System {
                messages.push(msg.clone());
            }
        }
        messages.push(DomainMessage {
            role: DomainRole::User,
            content: question.to_string(),
        });
        let text = chat_stream_to_progress(&exec_ctx.llm, messages, &exec_ctx.progress_tx).await?;
        Ok(FlowNodeResult {
            output: text,
            success: true,
        })
    }

    // ── review / fix / refactor：单轮 LLM（真流式） ───────────────

    async fn flow_lang_edit(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
        kind: &str,
    ) -> DomainResult<FlowNodeResult> {
        let code = &ctx.input;
        let skills_info = self.format_skills_info();
        let (role_desc, msg) = match kind {
            "review" => (
                "你是 Rust 代码审查专家。检查：架构分层、错误处理、异步安全、命名一致性、不必要的 clone、潜在死锁。",
                format!("请审查以下 Rust 代码：\n\n```rust\n{}\n```", code),
            ),
            "fix" => (
                "你是 Rust 编译错误修复专家。\n常见错误修复策略：\n\
                 - E0597 (borrow lifetime): 减少不必要的借用\n\
                 - E0382 (move): 使用 clone 或引用\n\
                 - E0277 (trait bound): 添加必要的 trait 约束\n\
                 - E0507 (move out): 使用 clone 或 ref",
                format!("## 代码与编译错误\n\n{}", code),
            ),
            _ => (
                "你是 Rust 代码重构专家。将代码重构为符合六边形架构和编码规范的版本。\n\
                 目标：分离端口定义和实现、引入 trait 抽象、使用 Arc<dyn Port> 依赖注入、添加文档注释。",
                format!("请重构以下 Rust 代码：\n\n```rust\n{}\n```", code),
            ),
        };
        let sys = format!(
            "{}\n\n## 可用技能\n{}\n\n{}",
            self.build_system_prompt_dynamic(exec_ctx).await,
            skills_info,
            role_desc
        );
        let text = chat_stream_to_progress(
            &exec_ctx.llm,
            self.build_messages(&sys, &msg),
            &exec_ctx.progress_tx,
        )
        .await?;
        Ok(FlowNodeResult {
            output: text,
            success: true,
        })
    }

    // ── knowledge-query：藏经阁召回（原 skill_knowledge_query 逻辑） ──

    async fn flow_knowledge_edit(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let topic = Self::extract_param(&ctx.input, "topic");
        let text = self.skill_knowledge_query(exec_ctx.clone(), &topic).await?;
        Ok(FlowNodeResult {
            output: text,
            success: true,
        })
    }

    // ── generate：生成 + 验证修复闭环 ─────────────────────────────

    async fn flow_generate_edit(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let skills_info = self.format_skills_info();
        let sys = if let Some(ref custom) = exec_ctx.ctx.system_prompt {
            custom.clone()
        } else {
            format!(
                "{}\n\n## 可用技能\n{}",
                self.build_system_prompt_dynamic(exec_ctx).await,
                skills_info
            )
        };
        let ws_hint = if let Some(ref ws) = exec_ctx.ctx.workspace_folder {
            format!(
                "\n\n项目工作目录: {}\n如果项目已存在，请在指定目录下操作；如果首次生成，请按项目结构规划。",
                ws
            )
        } else {
            String::new()
        };
        let gen_instruction = format!(
            "你是 Rust 编程专家。根据以下需求生成完整、可编译的 Rust 代码。{ws_hint}\n\n\
             ## 你的可用技能\n{}\n\
             生成要求：\n\
             1. 严格遵守六边形架构分层\n\
             2. DTO 派生 Serialize/Deserialize\n\
             3. 不要省略任何必要的 use 语句\n\
             4. 用 ```rust 代码块标注文件路径\n\n\
             ---\n需求：{}",
            skills_info, ctx.input,
        );
        let response = exec_ctx
            .llm
            .chat(self.build_messages(&sys, &gen_instruction))
            .await?;
        // 供 verify 节点复用：system 提示 / 原始生成指令
        ctx.results.insert(
            GenerateKey::Sys.as_str().to_string(),
            FlowNodeResult {
                output: sys,
                success: true,
            },
        );
        ctx.results.insert(
            GenerateKey::Instruction.as_str().to_string(),
            FlowNodeResult {
                output: gen_instruction,
                success: true,
            },
        );
        Ok(FlowNodeResult {
            output: response,
            success: true,
        })
    }

    async fn flow_generate_verify(
        &self,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let sys = flow_str(ctx, GenerateKey::Sys);
        let instruction = flow_str(ctx, GenerateKey::Instruction);
        // 首次从 edit 节点的产出发验证，避免重复生成
        let mut response = flow_str(ctx, ReactStage::Edit);
        let mut current_req = instruction;
        let max_retries = 3;
        for retry in 0..=max_retries {
            if retry > 0 {
                let code = self.extract_code(&response);
                current_req = format!(
                    "原始需求：{}\n\n上次生成的代码：\n```rust\n{}\n```\n\n编译错误（请修复，只输出修复后的完整代码）：\n{}",
                    ctx.input,
                    code,
                    "请参考上轮结果自行校验"
                );
                response = exec_ctx
                    .llm
                    .chat(self.build_messages(&sys, &current_req))
                    .await?;
            }
            let toolchain = match &exec_ctx.toolchain {
                Some(t) => t.clone(),
                None => {
                    return Ok(FlowNodeResult {
                        output: format!(
                            "✅ 代码已生成（未启用编译验证，设置 toolchain 以启用自动验证）\n\n{}",
                            response
                        ),
                        success: true,
                    });
                }
            };
            let result = toolchain.check("").await;
            if result.success {
                return Ok(FlowNodeResult {
                    output: format!(
                        "✅ 编译通过({} 轮)\n\n{}\n\n---\n## 编译检查\n{}",
                        retry + 1,
                        response,
                        result.output
                    ),
                    success: true,
                });
            }
            if retry >= max_retries {
                return Ok(FlowNodeResult {
                    output: format!(
                        "⚠️ 编译未通过（已重试 {} 次）\n\n## 生成的代码\n{}\n\n## 编译错误\n{}",
                        max_retries,
                        response,
                        result.errors.join("\n")
                    ),
                    success: true,
                });
            }
            // 携误差继续下一轮修复
            let code = self.extract_code(&response);
            ctx.results.insert(
                GenerateKey::LastCode.as_str().to_string(),
                FlowNodeResult {
                    output: code.clone(),
                    success: true,
                },
            );
            if current_req.is_empty() {
                current_req = format!(
                    "原始需求：{}\n\n编译错误（请修复，只输出修复后的完整代码）：\n{}",
                    ctx.input,
                    result.errors.join("\n")
                );
            } else {
                current_req = format!(
                    "原始需求：{}\n\n上次生成的代码：\n```rust\n{}\n```\n\n编译错误（请修复，只输出修复后的完整代码）：\n{}",
                    ctx.input,
                    code,
                    result.errors.join("\n")
                );
            }
        }
        Err(DomainError::ExecutionError(
            "代码生成验证循环异常退出".to_string(),
        ))
    }

    // ── 技能实现已迁移至 Flow 节点逻辑 ──────────────────────────

    /// 技能: rust-knowledge-query — 查询知识库内容（通过藏经阁引擎召回）
    async fn skill_knowledge_query(
        &self,
        exec_ctx: DomainExecutionContext,
        topic: &str,
    ) -> DomainResult<String> {
        // 解析参数，支持 JSON 格式和纯字符串格式
        let topic = Self::extract_param(topic, "topic");
        tracing::info!(
            "[skill_knowledge_query] 查询知识库, topic={}, has_sutra={}",
            topic,
            exec_ctx.sutra_library.is_some()
        );

        let topic = topic.trim();

        // 如果没有指定主题，从藏经阁获取知识库列表
        if topic.is_empty() || topic == "all" || topic == "目录" || topic == "list" {
            // 尝试从藏经阁获取知识库列表
            if let Some(sutra) = &exec_ctx.sutra_library {
                let expert_id = self.id();
                tracing::info!(
                    "[skill_knowledge_query] 从藏经阁加载知识库: expert_id={}",
                    expert_id
                );
                let kb_result = sutra.get_knowledge_base_by_expert(expert_id).await;
                tracing::info!("[skill_knowledge_query] get_knowledge_base_by_expert 返回: len={}, has_warning={}", kb_result.len(), kb_result.contains("⚠️"));
                if !kb_result.contains("⚠️") && !kb_result.is_empty() {
                    // 格式化知识库列表
                    if let Ok(kbases) = serde_json::from_str::<Vec<serde_json::Value>>(&kb_result) {
                        tracing::info!("[skill_knowledge_query] 解析到 {} 个知识库", kbases.len());
                        if !kbases.is_empty() {
                            return Ok(self.format_kbases_catalog(&kbases, sutra).await);
                        }
                    }
                }
            }
            // fallback: 返回静态目录
            tracing::info!("[skill_knowledge_query] 使用静态目录 fallback");
            Ok(self.format_knowledge_catalog())
        } else {
            // 根据主题从藏经阁召回知识库内容
            if let Some(sutra) = &exec_ctx.sutra_library {
                // 1. 先尝试使用 library_retrieve 进行语义召回
                // 发射 MemoryRetrieved（retrieve 阶段）：由 TraceContext 统一判断是否激活
                exec_ctx
                    .trace_context()
                    .emit(DomainEvent::MemoryRetrieved {
                        query: topic.to_string(),
                        results_count: 0,
                    })
                    .await;
                let retrieve_result = sutra.library_retrieve(topic, 5).await;
                if !retrieve_result.contains("⚠️") && !retrieve_result.contains("未找到") {
                    tracing::info!("[skill_knowledge_query] 藏经阁召回成功");
                    // 还需要获取详细内容，尝试从知识库切片中查找
                    let expert_id = self.id();
                    let kb_result = sutra.get_knowledge_base_by_expert(expert_id).await;
                    if !kb_result.contains("⚠️") && !kb_result.is_empty() {
                        if let Ok(kbases) =
                            serde_json::from_str::<Vec<serde_json::Value>>(&kb_result)
                        {
                            let detailed =
                                self.search_knowledge_in_sutra(&kbases, sutra, topic).await;
                            if !detailed.is_empty() {
                                return Ok(format!(
                                    "{}\n\n---\n\n## 详细知识库内容\n\n{}",
                                    retrieve_result, detailed
                                ));
                            }
                        }
                    }
                    return Ok(retrieve_result);
                }

                // 2. 如果召回失败，尝试直接搜索知识库切片
                let expert_id = self.id();
                let kb_result = sutra.get_knowledge_base_by_expert(expert_id).await;
                if !kb_result.contains("⚠️") && !kb_result.is_empty() {
                    if let Ok(kbases) = serde_json::from_str::<Vec<serde_json::Value>>(&kb_result) {
                        let detailed = self.search_knowledge_in_sutra(&kbases, sutra, topic).await;
                        if !detailed.is_empty() {
                            return Ok(detailed);
                        }
                    }
                }
            }

            // fallback: 使用静态搜索
            Ok(self.search_knowledge(topic))
        }
    }

    /// 格式化知识库目录（从藏经阁动态获取）
    async fn format_kbases_catalog(
        &self,
        kbases: &[serde_json::Value],
        sutra: &Arc<dyn subhuti_core::SutraLibraryPort>,
    ) -> String {
        let mut result = String::from("## Rust 编程专家知识库目录\n\n");

        for kb in kbases {
            let kb_name = kb.get("name").and_then(|v| v.as_str()).unwrap_or("未知");
            let kb_desc = kb.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let kb_id = kb.get("id").and_then(|v| v.as_str()).unwrap_or("");

            result.push_str(&format!("### 📚 {}\n{}\n\n", kb_name, kb_desc));

            // 获取该知识库的切片列表
            let chunks_result = sutra.list_chunks(kb_id).await;
            if let Ok(chunks) = serde_json::from_str::<Vec<serde_json::Value>>(&chunks_result) {
                for chunk in &chunks {
                    let title = chunk
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("未命名");
                    result.push_str(&format!("- {}\n", title));
                }
            }
            result.push('\n');
        }

        result
    }

    /// 从藏经阁知识库中搜索指定主题
    async fn search_knowledge_in_sutra(
        &self,
        kbases: &[serde_json::Value],
        sutra: &Arc<dyn subhuti_core::SutraLibraryPort>,
        topic: &str,
    ) -> String {
        let topic_lower = topic.to_lowercase();
        let mut result = String::new();

        for kb in kbases {
            let kb_name = kb.get("name").and_then(|v| v.as_str()).unwrap_or("未知");
            let kb_id = kb.get("id").and_then(|v| v.as_str()).unwrap_or("");

            let chunks_result = sutra.list_chunks(kb_id).await;
            if let Ok(chunks) = serde_json::from_str::<Vec<serde_json::Value>>(&chunks_result) {
                let mut matched_chunks = Vec::new();

                for chunk in &chunks {
                    let title = chunk
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let content = chunk
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();

                    // 简单的关键词匹配
                    if title.contains(&topic_lower) || content.contains(&topic_lower) {
                        matched_chunks.push(chunk);
                    }
                }

                if !matched_chunks.is_empty() {
                    result.push_str(&format!("### 📚 {} — 相关内容\n\n", kb_name));
                    for chunk in &matched_chunks {
                        let title = chunk
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("未命名");
                        let content = chunk.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        result.push_str(&format!("**{}**\n{}\n\n", title, content));
                    }
                }
            }
        }

        if result.is_empty() {
            format!("未在藏经阁知识库中找到与「{}」相关的内容。", topic)
        } else {
            result
        }
    }

    /// 从参数中提取指定字段的值，支持 JSON 格式和纯字符串格式
    fn extract_param(params: &str, field: &str) -> String {
        let trimmed = params.trim();

        // 尝试解析 JSON 格式
        if trimmed.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(value) = json.get(field) {
                    if let Some(s) = value.as_str() {
                        return s.to_string();
                    }
                }
                // 如果没有指定字段，尝试获取第一个字符串值
                if let Some(obj) = json.as_object() {
                    for (_, v) in obj {
                        if let Some(s) = v.as_str() {
                            return s.to_string();
                        }
                    }
                }
            }
        }

        // 否则返回原始字符串
        trimmed.to_string()
    }

    /// 格式化知识库目录结构
    fn format_knowledge_catalog(&self) -> String {
        r#"## Rust 编程专家知识库目录

### 📚 第一层：Rust 基础知识
- Tokens 异步运行时
- Serde 序列化/反序列化
- 错误处理（Result、panic、自定义错误）
- Arc<Mutex<T>> 并发原语
- Trait Object 动态分发

### 🏗️ 第二层：设计模式范式库
- 六边形分层架构（domain → application → adapter）
- 入站端口装饰器（横切逻辑：trace、鉴权、限流）
- 事件总线与观察者模式
- 仓储模式（Repository）
- DDD 聚合根与领域事件

### 📐 第三层：个人编码规范
- 架构约束（六边形分层、端口分离）
- 命名规范（Port/Service/Adapter 后缀）
- 错误处理规范
- 异步约定
- 文件组织规范

### 📂 第四层：项目上下文
- 项目目录结构
- 领域层/应用层/适配层职责
- 核心组件说明

---

💡 提示：可以使用以下关键词查询具体内容：
- "设计模式"、"架构"、"DDD" → 查询第二层
- "命名"、"规范"、"错误处理" → 查询第三层
- "tokio"、"serde"、"async" → 查询第一层"#
            .to_string()
    }

    /// 搜索知识库内容
    fn search_knowledge(&self, topic: &str) -> String {
        let topic_lower = topic.to_lowercase();

        // 检查匹配的层级
        let mut matched_layers = Vec::new();

        // 第二层关键词匹配
        let layer2_keywords = [
            "设计模式",
            "架构",
            "DDD",
            "六边形",
            "仓储",
            "聚合",
            "事件",
            "装饰器",
            "模式",
        ];
        if layer2_keywords
            .iter()
            .any(|kw| topic_lower.contains(kw) || topic.contains(kw))
        {
            matched_layers.push("第二层：设计模式范式库");
        }

        // 第三层关键词匹配
        let layer3_keywords = ["命名", "规范", "错误处理", "异步", "文件组织", "约束"];
        if layer3_keywords
            .iter()
            .any(|kw| topic_lower.contains(kw) || topic.contains(kw))
        {
            matched_layers.push("第三层：个人编码规范");
        }

        // 第一层关键词匹配
        let layer1_keywords = [
            "tokio",
            "serde",
            "async",
            "await",
            "arc",
            "mutex",
            "trait",
            "rust基础",
        ];
        if layer1_keywords.iter().any(|kw| topic_lower.contains(kw)) {
            matched_layers.push("第一层：Rust 基础知识");
        }

        // 第四层关键词匹配
        let layer4_keywords = ["项目结构", "目录", "文件", "组件", "项目"];
        if layer4_keywords
            .iter()
            .any(|kw| topic_lower.contains(kw) || topic.contains(kw))
        {
            matched_layers.push("第四层：项目上下文");
        }

        // 如果没有匹配，返回所有知识库
        if matched_layers.is_empty() {
            format!(
                "## 未找到与「{}」相关的专题，以下是完整知识库内容：\n\n{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n{}",
                topic,
                self.layer1_rust_basics(),
                self.layer2_design_patterns(),
                self.layer3_personal_conventions(),
                self.layer4_project_context()
            )
        } else {
            let mut result = format!("## 知识库查询结果：「{}」\n\n", topic);
            for name in &matched_layers {
                let content = match *name {
                    "第一层：Rust 基础知识" => self.layer1_rust_basics(),
                    "第二层：设计模式范式库" => self.layer2_design_patterns(),
                    "第三层：个人编码规范" => self.layer3_personal_conventions(),
                    "第四层：项目上下文" => self.layer4_project_context(),
                    _ => String::new(),
                };
                result.push_str(&format!("### {}\n\n{}\n\n---\n\n", name, content));
            }
            result
        }
    }

    // ─── 验证修复闭环 ─────────────────────────────────────────────

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

        while let Some(file_start) = remaining.find("File: ") {
            // 查找 "File: " 标记
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

/// 判断是否为知识库/专家能力查询
///
/// 当用户询问知识库内容、专家能力、系统设定等时，返回 true
/// 用于决定是否加载完整的四层知识库
fn is_knowledge_query(input: &str) -> bool {
    let input_lower = input.to_lowercase();

    let knowledge_keywords = [
        "知识库",
        "知识",
        "你会什么",
        "你能做什么",
        "你有什么能力",
        "能力",
        "技能",
        "skill",
        "skills",
        "专家",
        "是什么",
        "介绍",
        "系统提示",
        "system prompt",
        "system_prompt",
        "四层",
        "layer",
        "layer1",
        "layer2",
        "layer3",
        "layer4",
        "设计模式",
        "架构",
        "编码规范",
        "规范",
        "rust知识",
        "rust 知识",
        "rust知识库",
        "rust 专家",
        "编程专家",
    ];

    knowledge_keywords.iter().any(|kw| input_lower.contains(kw))
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
        let input = &exec_ctx.ctx.input;
        let has_skill_id = exec_ctx.skill_id.is_some();

        tracing::info!("[run] 输入={}, has_skill_id={}", input, has_skill_id);

        // 如果有指定技能，直接分发到对应处理（跳过规划）
        if has_skill_id {
            let params = exec_ctx
                .skill_params
                .clone()
                .unwrap_or_else(|| exec_ctx.ctx.input.clone());
            let sid = exec_ctx.skill_id.clone().unwrap();
            self.execute_skill(&sid, &params, exec_ctx).await
        } else {
            // 默认：使用 LLM 自动规划模式
            // 让 LLM 分析用户意图，自主选择技能组合（包括 rust-skill-list 查询技能列表）
            tracing::info!("[run] 使用 LLM 规划模式，自主选择技能");
            self.plan_and_execute(exec_ctx).await
        }
    }

    async fn execute_skill(
        &self,
        skill_id: &str,
        params: &str,
        exec_ctx: DomainExecutionContext,
    ) -> DomainResult<String> {
        // 技能参数即 Flow 输入（与原 skill_* 方法口径一致）
        let exec_ctx = DomainExecutionContext {
            ctx: DomainContext {
                input: params.to_string(),
                ..exec_ctx.ctx
            },
            ..exec_ctx
        };
        // rust-coding 智能路由：非编码查询降级为聊天（与原 skill_coding 口径一致）
        if skill_id == RustSkillKind::Coding.id() && !Self::is_coding_query(params) {
            tracing::info!("检测到非编码查询，转为聊天模式: {}", params);
            return self
                .run_skill_flow(RustSkillKind::Chat, exec_ctx, RustSkillKind::Chat.stages())
                .await;
        }
        // 从唯一注册表查技能：id → 种类 + 阶段（消除 match skill_id 魔法字符串）
        let Some(kind) = RustSkillKind::from_id(skill_id) else {
            // 未知技能，走默认 run
            let new_ctx = DomainExecutionContext {
                ctx: DomainContext {
                    input: format!("技能ID: {}, 参数: {}", skill_id, params),
                    ..exec_ctx.ctx
                },
                // 清掉 skill_id，避免 run() 的 has_skill_id 分支再次命中，
                // 导致 execute_skill ↔ run 无限递归（无效 skill_id 超时卡死）。
                skill_id: None,
                ..exec_ctx
            };
            return self.run(new_ctx).await;
        };
        self.run_skill_flow(kind, exec_ctx, kind.stages()).await
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
    fn test_has_eight_skills() {
        let expert = RustExpert::new();
        assert_eq!(expert.skills().len(), 8);
    }

    #[test]
    fn test_skill_list_skill_exists() {
        let expert = RustExpert::new();
        let skill_list = expert.skills();
        let skill = skill_list.iter().find(|s| s.id == "rust-skill-list");
        assert!(skill.is_some(), "rust-skill-list 技能应该存在");
        let skill = skill.unwrap();
        assert_eq!(skill.name, "技能列表");
    }

    #[test]
    fn test_knowledge_query_skill_exists() {
        let expert = RustExpert::new();
        let skill_list = expert.skills();
        let skill = skill_list.iter().find(|s| s.id == "rust-knowledge-query");
        assert!(skill.is_some(), "rust-knowledge-query 技能应该存在");
        let skill = skill.unwrap();
        assert_eq!(skill.name, "知识库查询");
    }

    #[test]
    fn test_knowledge_catalog_not_empty() {
        let expert = RustExpert::new();
        let catalog = expert.format_knowledge_catalog();
        assert!(catalog.contains("知识库目录"));
        assert!(catalog.contains("Rust 基础知识"));
        assert!(catalog.contains("设计模式"));
        assert!(catalog.contains("编码规范"));
    }

    #[test]
    fn test_is_knowledge_query() {
        // 知识库查询 → true
        assert!(is_knowledge_query("rust知识库中大概写了什么"));
        assert!(is_knowledge_query("介绍一下你的知识库"));
        assert!(is_knowledge_query("你有什么能力"));
        assert!(is_knowledge_query("你会什么"));
        assert!(is_knowledge_query("介绍一下Rust专家"));
        assert!(is_knowledge_query("设计模式有哪些"));
        assert!(is_knowledge_query("编码规范是什么"));
        assert!(is_knowledge_query("你的skill有哪些"));
        // 闲聊/普通对话 → false
        assert!(!is_knowledge_query("你好"));
        assert!(!is_knowledge_query("我是张三"));
        assert!(!is_knowledge_query("今天天气怎么样"));
        assert!(!is_knowledge_query("帮我写一个函数"));
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
