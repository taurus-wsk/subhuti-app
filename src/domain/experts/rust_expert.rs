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

use async_trait::async_trait;

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

    /// 技能: rust-generate — 从需求生成代码（带验证循环）
    async fn skill_generate(
        &self,
        exec_ctx: DomainExecutionContext,
        requirement: &str,
    ) -> DomainResult<String> {
        let sys = self.build_system_prompt();
        let gen_instruction = format!(
            "你是 Rust 编程专家。根据以下需求生成完整、可编译的 Rust 代码。\n\n\
             生成要求：\n\
             1. 严格遵守六边形架构分层\n\
             2. DTO 派生 Serialize/Deserialize\n\
             3. 不要省略任何必要的 use 语句\n\
             4. 用 ```rust 代码块标注文件路径\n\n\
             ---\n需求：{}",
            requirement
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
            // 默认：按代码生成处理
            let input = exec_ctx.ctx.input.clone();
            self.skill_generate(exec_ctx, &input).await
        }
    }

    async fn execute_skill(
        &self,
        skill_id: &str,
        params: &str,
        exec_ctx: DomainExecutionContext,
    ) -> DomainResult<String> {
        match skill_id {
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
    fn test_has_four_skills() {
        let expert = RustExpert::new();
        assert_eq!(expert.skills().len(), 4);
    }
}
