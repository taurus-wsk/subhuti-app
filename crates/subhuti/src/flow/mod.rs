//! # Flow Layer - 流程层
//!
//! 职责：Agent 智能闭环，支持多种流程策略
//!
//! ## 设计理念
//!
//! - **Flow trait**: 标准接口，用户可自定义流程
//! - **内置流程**: Simple、ReAct、Plan-Act 等
//! - **可插拔**: 类似 Gin 中间件，流程可替换
//!
//! ## 内置流程
//!
//! | 流程 | 适用场景 |
//! |------|----------|
//! | SimpleFlow | 简单对话，无工具调用 |
//! | ReactFlow | ReAct 循环，自动工具调用 |
//! | PlanActFlow | 先规划再执行 |
//!
//! ## 自定义流程
//!
//! ```rust,ignore
//! use subhuti::flow::{Flow, FlowContext};
//!
//! struct MyCustomFlow;
//!
//! #[async_trait]
//! impl Flow for MyCustomFlow {
//!     async fn execute(&self, ctx: &mut FlowContext) -> Result<String> {
//!         // 自定义流程逻辑
//!     }
//! }
//! ```

mod plan_act;
mod react;
mod simple;

pub use plan_act::PlanActFlow;
pub use react::ReactFlow;
pub use simple::SimpleFlow;

use crate::context::RunContext;
use crate::memory::Memory;
use crate::runtime::llm;
use crate::runtime::tools;
use crate::runtime::{Runtime, Session};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 流程步骤 - Skill 可以预设的执行步骤
///
/// 每个步骤代表流程中的一个原子操作，可以是：
/// - Tool: 直接调用工具（不需要 AI 思考）
/// - Knowledge: 查询知识库（不需要 AI 思考）
/// - LLM: 调用 LLM 生成内容（需要 AI）
/// - Condition: 条件判断（代码逻辑）
/// - Memory: 记忆操作（读写记忆）
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowStep {
    /// 调用工具
    Tool {
        /// 工具名称
        name: String,
        /// 工具参数（可以是静态值或从输入提取）
        args: serde_json::Value,
        /// 是否将结果保存到上下文
        save_to_context: Option<String>,
    },

    /// 查询知识库
    Knowledge {
        /// 查询关键词
        query: String,
        /// 是否将结果保存到上下文
        save_to_context: Option<String>,
    },

    /// 调用 LLM
    LLM {
        /// 提示词模板（可以使用 {{context}} 插入上下文）
        prompt: String,
        /// 是否使用历史上下文
        use_context: bool,
    },

    /// 调用 LLM 并保存结果到上下文
    LLMToContext {
        /// 提示词模板
        prompt: String,
        /// 保存结果的上下文键
        save_to_context: String,
        /// 是否使用历史上下文
        use_context: bool,
    },

    /// 条件判断
    Condition {
        /// 条件表达式
        condition: String,
        /// 条件为真时执行的步骤
        if_true: Vec<FlowStep>,
        /// 条件为假时执行的步骤
        if_false: Vec<FlowStep>,
    },

    /// 记忆操作
    Memory {
        /// 操作类型：read/write/search
        action: String,
        /// 键或查询内容
        key: String,
        /// 值（写入时使用）
        value: Option<String>,
    },

    /// 并行执行多个步骤
    Parallel {
        /// 并行执行的步骤列表
        steps: Vec<FlowStep>,
    },

    /// 循环执行
    Loop {
        /// 循环变量名
        variable: String,
        /// 循环次数或迭代源
        iterations: usize,
        /// 循环体步骤
        body: Vec<FlowStep>,
    },
}

impl FlowStep {
    /// 创建工具调用步骤
    pub fn tool(name: impl Into<String>, args: serde_json::Value) -> Self {
        FlowStep::Tool {
            name: name.into(),
            args,
            save_to_context: None,
        }
    }

    /// 创建工具调用步骤（保存结果）
    pub fn tool_with_context(
        name: impl Into<String>,
        args: serde_json::Value,
        context_key: impl Into<String>,
    ) -> Self {
        FlowStep::Tool {
            name: name.into(),
            args,
            save_to_context: Some(context_key.into()),
        }
    }

    /// 创建知识库查询步骤
    pub fn knowledge(query: impl Into<String>) -> Self {
        FlowStep::Knowledge {
            query: query.into(),
            save_to_context: None,
        }
    }

    /// 创建知识库查询步骤（保存结果）
    pub fn knowledge_with_context(
        query: impl Into<String>,
        context_key: impl Into<String>,
    ) -> Self {
        FlowStep::Knowledge {
            query: query.into(),
            save_to_context: Some(context_key.into()),
        }
    }

    /// 创建 LLM 调用步骤
    pub fn llm(prompt: impl Into<String>) -> Self {
        FlowStep::LLM {
            prompt: prompt.into(),
            use_context: true,
        }
    }

    /// 创建 LLM 调用步骤（不使用上下文）
    pub fn llm_no_context(prompt: impl Into<String>) -> Self {
        FlowStep::LLM {
            prompt: prompt.into(),
            use_context: false,
        }
    }

    /// 创建 LLM 调用步骤，保存结果到上下文
    pub fn llm_to_context(prompt: impl Into<String>, context_key: impl Into<String>) -> Self {
        FlowStep::LLMToContext {
            prompt: prompt.into(),
            save_to_context: context_key.into(),
            use_context: false,
        }
    }

    /// 创建条件判断步骤
    pub fn condition(
        condition: impl Into<String>,
        if_true: Vec<FlowStep>,
        if_false: Vec<FlowStep>,
    ) -> Self {
        FlowStep::Condition {
            condition: condition.into(),
            if_true,
            if_false,
        }
    }

    /// 创建记忆读取步骤
    pub fn memory_read(key: impl Into<String>) -> Self {
        FlowStep::Memory {
            action: "read".into(),
            key: key.into(),
            value: None,
        }
    }

    /// 创建记忆写入步骤
    pub fn memory_write(key: impl Into<String>, value: impl Into<String>) -> Self {
        FlowStep::Memory {
            action: "write".into(),
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// 创建并行执行步骤
    pub fn parallel(steps: Vec<FlowStep>) -> Self {
        FlowStep::Parallel { steps }
    }
}

/// 流程配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlowConfig {
    /// 最大循环次数
    pub max_iterations: usize,
    /// 是否启用自动重试
    pub auto_retry: bool,
    /// 重试次数
    pub max_retries: usize,
    /// 收敛阈值 (连续无工具调用次数)
    pub convergence_threshold: usize,
    /// 是否启用反思
    pub enable_reflection: bool,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            auto_retry: true,
            max_retries: 3,
            convergence_threshold: 2,
            enable_reflection: true,
        }
    }
}

/// 流程状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowState {
    /// 初始
    Init,
    /// 计划中
    Planning,
    /// 执行中
    Acting,
    /// 观察中
    Observing,
    /// 反思中
    Reflecting,
    /// 完成
    Completed,
    /// 失败
    Failed,
}

impl std::fmt::Display for FlowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowState::Init => write!(f, "Init"),
            FlowState::Planning => write!(f, "Planning"),
            FlowState::Acting => write!(f, "Acting"),
            FlowState::Observing => write!(f, "Observing"),
            FlowState::Reflecting => write!(f, "Reflecting"),
            FlowState::Completed => write!(f, "Completed"),
            FlowState::Failed => write!(f, "Failed"),
        }
    }
}

/// 流程上下文 - 所有流程共享的执行流程上下文
#[derive(Debug)]
pub struct FlowContext<'a> {
    /// 会话
    pub session: &'a mut Session,
    /// 运行时
    pub runtime: &'a Runtime,
    /// 记忆
    pub memory: &'a Memory,
    /// 配置
    pub config: FlowConfig,
    /// 当前状态
    pub state: FlowState,
    /// 迭代次数
    pub iteration: usize,
    /// 用户输入
    pub input: &'a str,
    /// 上下文存储（用于保存步骤执行结果）
    pub context_data: std::collections::HashMap<String, String>,
}

impl<'a> FlowContext<'a> {
    /// 创建新的流程上下文
    pub fn new(
        session: &'a mut Session,
        runtime: &'a Runtime,
        memory: &'a Memory,
        config: FlowConfig,
    ) -> Self {
        Self {
            session,
            runtime,
            memory,
            config,
            state: FlowState::Init,
            iteration: 0,
            input: "",
            context_data: std::collections::HashMap::new(),
        }
    }

    /// 创建带输入的流程上下文
    pub fn with_input(
        session: &'a mut Session,
        runtime: &'a Runtime,
        memory: &'a Memory,
        config: FlowConfig,
        input: &'a str,
    ) -> Self {
        Self {
            session,
            runtime,
            memory,
            config,
            state: FlowState::Init,
            iteration: 0,
            input,
            context_data: std::collections::HashMap::new(),
        }
    }

    /// 从 RunContext 创建 FlowContext
    ///
    /// 分层设计：
    /// - run_ctx: 请求级（session）
    /// - runtime: 全局（LLM、工具）
    /// - memory: 全局（记忆系统）
    pub fn from_run_context(
        run_ctx: &'a mut RunContext,
        runtime: &'a Runtime,
        memory: &'a Memory,
        config: FlowConfig,
        input: &'a str,
    ) -> Self {
        Self {
            session: &mut run_ctx.session,
            runtime,
            memory,
            config,
            state: FlowState::Init,
            iteration: 0,
            input,
            context_data: std::collections::HashMap::new(),
        }
    }

    /// 设置状态
    pub fn set_state(&mut self, state: FlowState) {
        tracing::info!("Flow state: {} -> {}", self.state, state);
        self.state = state;
    }

    /// 增加迭代次数
    pub fn increment_iteration(&mut self) {
        self.iteration += 1;
        tracing::debug!("Iteration: {}", self.iteration);
    }

    /// 检查是否超过最大迭代
    pub fn is_exceeded_max_iterations(&self) -> bool {
        self.iteration >= self.config.max_iterations
    }

    /// 调用 LLM
    pub async fn call_llm(&self) -> Result<String> {
        let messages = self.session.to_context();
        self.runtime.call_llm(messages).await
    }

    /// 调用 LLM（支持工具调用）
    pub async fn call_llm_with_tools(&self) -> Result<llm::LLMResponse> {
        let messages = self.session.to_context();
        self.runtime.call_llm_with_tools(messages).await
    }

    /// 执行工具
    pub async fn execute_tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<tools::ToolResult> {
        self.runtime.execute_tool(name, args).await
    }

    /// 获取可用工具
    pub fn get_tools(&self) -> Vec<tools::ToolInfo> {
        self.runtime.get_tools()
    }

    /// 保存上下文数据
    pub fn save_context(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.context_data.insert(key.into(), value.into());
    }

    /// 获取上下文数据
    pub fn get_context(&self, key: &str) -> Option<&String> {
        self.context_data.get(key)
    }

    /// 执行预设的流程步骤
    ///
    /// 这是 Skill 的核心功能：按预设步骤执行，减少 AI 思考消耗
    pub async fn execute_steps(&mut self, steps: &[FlowStep]) -> Result<String> {
        let mut final_result = String::new();

        for step in steps {
            // 使用 Box::pin 解决递归异步函数问题
            let result = Box::pin(self.execute_step(step)).await?;
            if !result.is_empty() {
                final_result = result;
            }
        }

        Ok(final_result)
    }

    /// 执行单个步骤
    async fn execute_step(&mut self, step: &FlowStep) -> Result<String> {
        match step {
            FlowStep::Tool {
                name,
                args,
                save_to_context,
            } => {
                tracing::info!("Executing tool: {}", name);

                // 替换 args 中的模板变量
                let processed_args = self.process_args_template(args.clone());

                let result = self.runtime.execute_tool(name, processed_args).await?;
                let output = if result.success {
                    result.content
                } else {
                    result.error.unwrap_or_else(|| "Tool failed".to_string())
                };

                if let Some(key) = save_to_context {
                    self.save_context(key, &output);
                }

                Ok(output)
            }

            FlowStep::Knowledge {
                query,
                save_to_context,
            } => {
                tracing::info!("Querying knowledge: {}", query);

                // 替换 query 中的模板变量
                let processed_query = self.process_prompt_template(query);

                let results = self.memory.search_knowledge(&processed_query, 5);
                let output = results
                    .iter()
                    .map(|r| r.item.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n");

                if let Some(key) = save_to_context {
                    self.save_context(key, &output);
                }

                Ok(output)
            }

            FlowStep::LLM {
                prompt,
                use_context,
            } => {
                tracing::info!("Calling LLM with prompt: {}", prompt);

                // 替换模板变量
                let processed_prompt = self.process_prompt_template(prompt);

                let messages = if *use_context {
                    // 使用历史上下文 + 当前提示
                    let mut ctx = self.session.to_context();
                    ctx.push(crate::runtime::llm::Message::user(&processed_prompt));
                    ctx
                } else {
                    vec![crate::runtime::llm::Message::user(&processed_prompt)]
                };

                self.runtime.call_llm(messages).await
            }

            FlowStep::LLMToContext {
                prompt,
                save_to_context,
                use_context,
            } => {
                tracing::info!("Calling LLM with prompt: {} -> {}", prompt, save_to_context);

                // 替换模板变量
                let processed_prompt = self.process_prompt_template(prompt);

                let messages = if *use_context {
                    let mut ctx = self.session.to_context();
                    ctx.push(crate::runtime::llm::Message::user(&processed_prompt));
                    ctx
                } else {
                    vec![crate::runtime::llm::Message::user(&processed_prompt)]
                };

                let result = self.runtime.call_llm(messages).await?;
                self.save_context(save_to_context, &result);
                Ok(result)
            }

            FlowStep::Condition {
                condition,
                if_true,
                if_false,
            } => {
                tracing::info!("Checking condition: {}", condition);
                let should_execute_true = self.evaluate_condition(condition);

                if should_execute_true {
                    Box::pin(self.execute_steps(if_true)).await
                } else {
                    Box::pin(self.execute_steps(if_false)).await
                }
            }

            FlowStep::Memory { action, key, value } => {
                tracing::info!("Memory action: {} - {}", action, key);
                match action.as_str() {
                    "read" => {
                        let results = self.memory.search_short_term(key, 1);
                        Ok(results
                            .first()
                            .map(|r| r.item.content.clone())
                            .unwrap_or_default())
                    }
                    "write" => {
                        if let Some(v) = value {
                            self.memory
                                .write_short_term(format!("{}: {}", key, v), "skill")?;
                        }
                        Ok("".to_string())
                    }
                    "search" => {
                        let results = self.memory.search_short_term(key, 5);
                        Ok(results
                            .iter()
                            .map(|r| r.item.content.clone())
                            .collect::<Vec<_>>()
                            .join("\n"))
                    }
                    _ => Err(anyhow::anyhow!("Unknown memory action: {}", action)),
                }
            }

            FlowStep::Parallel { steps } => {
                tracing::info!("Executing parallel steps");
                // 并行执行（简化实现，实际可以用 futures::join!）
                let mut results = Vec::new();
                for s in steps {
                    results.push(Box::pin(self.execute_step(s)).await?);
                }
                Ok(results.join("\n"))
            }

            FlowStep::Loop {
                variable,
                iterations,
                body,
            } => {
                tracing::info!("Executing loop: {} iterations", iterations);
                let mut results = Vec::new();
                for i in 0..*iterations {
                    self.save_context(variable, i.to_string());
                    results.push(Box::pin(self.execute_steps(body)).await?);
                }
                Ok(results.join("\n"))
            }
        }
    }

    /// 处理提示词模板
    fn process_prompt_template(&self, prompt: &str) -> String {
        let mut result = prompt.to_string();

        // 替换 {{input}}
        result = result.replace("{{input}}", self.input);

        // 替换 {{context.*}}
        for (key, value) in &self.context_data {
            result = result.replace(&format!("{{{{context.{}}}}}", key), value);
        }

        result
    }

    /// 处理 JSON 参数中的模板变量
    fn process_args_template(&self, args: serde_json::Value) -> serde_json::Value {
        match args {
            serde_json::Value::String(s) => {
                serde_json::Value::String(self.process_prompt_template(&s))
            }
            serde_json::Value::Object(map) => {
                let processed_map = map
                    .into_iter()
                    .map(|(k, v)| (k, self.process_args_template(v)))
                    .collect();
                serde_json::Value::Object(processed_map)
            }
            serde_json::Value::Array(arr) => {
                let processed_arr = arr
                    .into_iter()
                    .map(|v| self.process_args_template(v))
                    .collect();
                serde_json::Value::Array(processed_arr)
            }
            other => other,
        }
    }

    /// 评估条件表达式（简化实现）
    fn evaluate_condition(&self, condition: &str) -> bool {
        // 简化实现：检查上下文是否存在
        if condition.starts_with("has_context:") {
            let key = condition.strip_prefix("has_context:").unwrap_or("");
            self.context_data.contains_key(key)
        } else if condition.starts_with("input_contains:") {
            let keyword = condition.strip_prefix("input_contains:").unwrap_or("");
            self.input.contains(keyword)
        } else {
            // 默认返回 true
            true
        }
    }
}

/// Flow trait - 流程抽象接口
///
/// 所有流程必须实现此 trait，支持自定义流程策略
#[async_trait]
pub trait Flow: Send + Sync {
    /// 流程名称
    fn name(&self) -> &str;

    /// 流程描述
    fn description(&self) -> &str;

    /// 执行流程
    async fn execute(&self, ctx: &mut FlowContext<'_>) -> Result<String>;
}

/// 流程类型枚举 - 内置流程选择
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum FlowType {
    /// 简单对话流程
    Simple,
    /// ReAct 循环流程
    #[default]
    React,
    /// Plan-Act 流程
    PlanAct,
    /// 自定义流程
    Custom,
}

impl std::fmt::Display for FlowType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowType::Simple => write!(f, "Simple"),
            FlowType::React => write!(f, "React"),
            FlowType::PlanAct => write!(f, "PlanAct"),
            FlowType::Custom => write!(f, "Custom"),
        }
    }
}

/// 统一流程管理器
///
/// 支持注册和切换不同的流程策略
pub struct FlowManager {
    config: FlowConfig,
    flow_type: FlowType,
    custom_flow: Option<Arc<dyn Flow>>,
}

impl std::fmt::Debug for FlowManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowManager")
            .field("config", &self.config)
            .field("flow_type", &self.flow_type)
            .field("has_custom_flow", &self.custom_flow.is_some())
            .finish()
    }
}

impl FlowManager {
    /// 创建新的 FlowManager (默认 ReactFlow)
    pub fn new() -> Self {
        Self::with_config(FlowConfig::default())
    }

    /// 使用配置创建
    pub fn with_config(config: FlowConfig) -> Self {
        Self {
            config,
            flow_type: FlowType::default(),
            custom_flow: None,
        }
    }

    /// 设置流程类型
    pub fn set_flow_type(&mut self, flow_type: FlowType) {
        self.flow_type = flow_type;
        tracing::info!("Flow type set to: {}", flow_type);
    }

    /// 注册自定义流程
    pub fn register_custom_flow(&mut self, flow: Arc<dyn Flow>) {
        self.custom_flow = Some(flow);
        self.flow_type = FlowType::Custom;
        tracing::info!("Custom flow registered");
    }

    /// 获取当前流程类型
    pub fn flow_type(&self) -> FlowType {
        self.flow_type
    }

    /// 执行流程（使用默认流程类型）
    pub async fn execute(
        &self,
        session: &mut Session,
        runtime: &Runtime,
        memory: &Memory,
    ) -> Result<String> {
        self.execute_with_flow_type(session, runtime, memory, self.flow_type)
            .await
    }

    /// 执行流程（显式指定流程类型）
    pub async fn execute_with_flow_type(
        &self,
        session: &mut Session,
        runtime: &Runtime,
        memory: &Memory,
        flow_type: FlowType,
    ) -> Result<String> {
        let mut ctx = FlowContext::new(session, runtime, memory, self.config.clone());

        // 根据流程类型选择执行器
        match flow_type {
            FlowType::Simple => {
                let flow = SimpleFlow::new();
                flow.execute(&mut ctx).await
            }
            FlowType::React => {
                let flow = ReactFlow::with_config(self.config.clone());
                flow.execute(&mut ctx).await
            }
            FlowType::PlanAct => {
                let flow = PlanActFlow::with_config(self.config.clone());
                flow.execute(&mut ctx).await
            }
            FlowType::Custom => {
                if let Some(flow) = &self.custom_flow {
                    flow.execute(&mut ctx).await
                } else {
                    Err(anyhow::anyhow!("No custom flow registered"))
                }
            }
        }
    }

    /// 执行预设步骤（Skill 使用）
    ///
    /// 当 Skill 匹配成功时，使用预设的流程步骤执行
    /// 这样可以减少 AI 思考消耗，提高效率
    pub async fn execute_steps(
        &self,
        session: &mut Session,
        runtime: &Runtime,
        memory: &Memory,
        input: &str,
        steps: &[FlowStep],
    ) -> Result<String> {
        let mut ctx = FlowContext::with_input(session, runtime, memory, self.config.clone(), input);

        ctx.execute_steps(steps).await
    }

    /// 获取配置
    pub fn config(&self) -> &FlowConfig {
        &self.config
    }
}

impl Default for FlowManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_config_default() {
        let config = FlowConfig::default();
        assert_eq!(config.max_iterations, 10);
        assert!(config.auto_retry);
        assert!(config.enable_reflection);
    }

    #[test]
    fn test_flow_manager_types() {
        let mut manager = FlowManager::new();
        assert_eq!(manager.flow_type(), FlowType::React);

        manager.set_flow_type(FlowType::Simple);
        assert_eq!(manager.flow_type(), FlowType::Simple);
    }
}
