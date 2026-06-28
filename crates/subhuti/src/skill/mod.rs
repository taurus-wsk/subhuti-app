//! # Skill Layer - Skill 层
//!
//! **纯代码风格的 Skill 系统，支持预设主流程模板**
//!
//! ## 设计理念
//!
//! - **全代码实现**: Skill 用代码实现，不需要声明式步骤
//! - **预设主流程**: 提供几个常用的 Flow 模板（ReAct、Plan-Act 等）
//! - **灵活选择**: Skill 开始前可以选择使用预设模板或完全自定义
//!
//! ## 核心创新
//!
//! 传统设计：Skill 定义声明式步骤 → 表达能力受限
//! Subhuti 设计：Skill 纯代码实现 + 可选预设 Flow 模板 → 灵活性最大化
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! use subhuti::skill::{Skill, SkillContext, FlowTemplate};
//!
//! // 示例1：使用预设 ReAct 流程模板
//! struct WeatherSkill;
//!
//! impl Skill for WeatherSkill {
//!     fn name(&self) -> &str { "weather" }
//!
//!     fn matches(&self, input: &str) -> f32 {
//!         if input.contains("天气") { 0.9 } else { 0.0 }
//!     }
//!
//!     // 选择预设流程模板
//!     fn flow_template(&self) -> Option<FlowTemplate> {
//!         Some(FlowTemplate::ReAct)
//!     }
//!
//!     // 纯代码实现
//!     async fn execute(&self, ctx: SkillContext) -> Result<String> {
//!         // 步骤1：调用工具查询天气
//!         let weather_data = ctx.call_tool("get_weather", json!({"city": "北京"})).await?;
//!
//!         // 步骤2：调用 LLM 生成回复
//!         let response = ctx.call_llm(vec![
//!             Message::user(format!("根据天气数据 {} 回答用户问题", weather_data))
//!         ]).await?;
//!
//!         Ok(response)
//!     }
//! }
//!
//! // 示例2：完全自定义流程
//! struct OrderSkill;
//!
//! impl Skill for OrderSkill {
//!     fn name(&self) -> &str { "order" }
//!
//!     fn matches(&self, input: &str) -> f32 {
//!         if input.contains("下单") { 0.9 } else { 0.0 }
//!     }
//!
//!     // 不使用预设模板，完全自定义
//!     fn flow_template(&self) -> Option<FlowTemplate> {
//!         None // None 表示完全自定义
//!     }
//!
//!     async fn execute(&self, ctx: SkillContext) -> Result<String> {
//!         // 完全自定义的业务逻辑
//!         let order = parse_order(ctx.input)?;
//!
//!         if !check_inventory(&order).await? {
//!             return Ok("库存不足");
//!         }
//!
//!         let result = ctx.call_tool("create_order", order).await?;
//!
//!         if result.success {
//!             send_notification(&order.user_id).await?;
//!         }
//!
//!         Ok(format!("订单创建成功: {}", result.order_id))
//!     }
//! }
//! ```

use crate::context::{RunContext, TokenStats};
use crate::flow::FlowStep;
use crate::{
    memory::Memory,
    runtime::{Runtime, Session},
    Result,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 预设主流程模板
///
/// 提供几个常用的流程模板，Skill 可以选择使用
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowTemplate {
    /// ReAct 流程：Reasoning + Acting
    /// 适合需要多轮思考和工具调用的场景
    ReAct,

    /// Plan-Act 流程：Plan → Act → Observe
    /// 适合需要规划的任务
    PlanAct,

    /// Simple 流程：简单对话
    /// 适合简单的问答场景
    Simple,

    /// Chain-of-Thought 流程：思维链
    /// 适合需要复杂推理的场景
    ChainOfThought,
}

/// Skill 上下文
pub struct SkillContext<'a> {
    /// 用户输入
    pub input: &'a str,
    /// 会话
    pub session: &'a mut Session,
    /// 运行时
    pub runtime: &'a Runtime,
    /// 记忆
    pub memory: &'a Memory,
    /// 匹配度
    pub confidence: f32,
    /// 选择的流程模板
    pub flow_template: Option<FlowTemplate>,
    /// Token 统计（Arc 支持跨调用共享）
    pub tokens: Arc<RwLock<TokenStats>>,
}

impl<'a> SkillContext<'a> {
    /// 创建新的上下文
    pub fn new(
        input: &'a str,
        session: &'a mut Session,
        runtime: &'a Runtime,
        memory: &'a Memory,
        confidence: f32,
        flow_template: Option<FlowTemplate>,
    ) -> Self {
        Self {
            input,
            session,
            runtime,
            memory,
            confidence,
            flow_template,
            tokens: Arc::new(RwLock::new(TokenStats::default())),
        }
    }

    /// 从 RunContext 创建 SkillContext
    ///
    /// 全局资源（runtime、memory）单独传入，
    /// 请求级资源（session、tokens）从 RunContext 获取。
    ///
    /// 设计理念：类似 HTTP 的 State + Extensions 模式
    /// - runtime/memory: 全局共享，只读（类似 AppState）
    /// - session/tokens: 请求级，可变（类似 Request Extensions）
    pub fn from_run_context(
        input: &'a str,
        run_ctx: &'a mut RunContext,
        runtime: &'a Runtime,
        memory: &'a Memory,
        confidence: f32,
        flow_template: Option<FlowTemplate>,
    ) -> Self {
        Self {
            input,
            session: &mut run_ctx.session,
            runtime,
            memory,
            confidence,
            flow_template,
            tokens: run_ctx.tokens.clone(),
        }
    }

    /// 获取 Token 统计
    pub async fn get_tokens(&self) -> TokenStats {
        self.tokens.read().await.clone()
    }

    /// 添加 Token 统计
    pub async fn add_tokens(&self, response: &crate::runtime::llm::LLMResponse) {
        let mut tokens = self.tokens.write().await;
        tokens.add(response);
    }

    /// 调用工具
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String> {
        let result = self.runtime.execute_tool(name, args).await?;
        if result.success {
            Ok(result.content)
        } else {
            Err(anyhow::anyhow!(
                "Tool failed: {}",
                result.error.unwrap_or_default()
            ))
        }
    }

    /// 调用 LLM
    pub async fn call_llm(&self, messages: Vec<crate::runtime::llm::Message>) -> Result<String> {
        let response = self.runtime.call_llm_with_stats(messages).await?;

        // 累加 token 统计
        self.add_tokens(&response).await;

        Ok(response.content)
    }

    /// 调用 LLM（流式输出）
    ///
    /// callback: 每收到一块数据时调用
    pub async fn call_llm_streaming(
        &self,
        messages: Vec<crate::runtime::llm::Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        self.runtime.call_llm_streaming(messages, callback).await
    }

    /// 获取记忆
    pub fn get_memory(&self, key: &str) -> Option<String> {
        // 搜索短期记忆
        let results = self.memory.search_short_term(key, 1);
        results.first().map(|r| r.item.content.clone())
    }

    /// 设置记忆
    pub fn set_memory(&self, key: &str, value: &str) {
        // 写入短期记忆
        let _ = self
            .memory
            .write_short_term(format!("{}: {}", key, value), "skill");
    }
}

/// Skill trait - Skill 抽象接口
///
/// **纯代码风格，支持预设主流程模板**
///
/// Skill 用代码实现，不需要声明式步骤
/// 可以选择使用预设的 Flow 模板，也可以完全自定义
///
/// ## 流程模板选择机制
///
/// Skill 可以选择以下三种流程模板之一：
/// 1. Simple：简单流程，适合直接工具调用
/// 2. ReAct：多轮思考流程，适合复杂推理
/// 3. PlanAct：先规划再执行，适合需要规划的任务
///
/// 使用方式：
/// - 重写 `flow_template()` 返回要使用的模板
/// - 实现对应模板的 `execute_xxx()` 方法
/// - 如果不选择模板，直接实现 `execute()` 完全自定义
#[async_trait]
pub trait Skill: Send + Sync + 'static {
    /// Skill 名称
    fn name(&self) -> &str;

    /// Skill 描述
    fn description(&self) -> &str {
        ""
    }

    /// 匹配度（0.0-1.0）
    /// 返回值越高表示越匹配
    fn matches(&self, input: &str) -> f32;

    /// 关键词列表（用于索引优化）
    ///
    /// 返回触发此 Skill 的关键词列表
    /// 用于构建倒排索引，优化大规模 Skill 匹配性能
    ///
    /// 示例：
    /// - WeatherSkill: ["天气", "温度", "预报"]
    /// - CalculatorSkill: ["计算", "加", "减", "乘", "除"]
    fn keywords(&self) -> Vec<String> {
        Vec::new() // 默认返回空，不参与关键词索引
    }

    /// 选择预设流程模板（可选）
    ///
    /// 返回 Some(template) 表示使用预设模板，execute() 会自动路由到对应方法
    /// 返回 None 表示完全自定义，需要实现 execute()
    fn flow_template(&self) -> Option<FlowTemplate> {
        None
    }

    /// 获取所有实现的流程模板版本
    ///
    /// 默认实现：检查 flow_template() 返回的模板
    /// Skill 可以重写此方法返回所有支持的模板版本
    fn supported_templates(&self) -> Vec<FlowTemplate> {
        self.flow_template().map(|t| vec![t]).unwrap_or_default()
    }

    /// 核心执行方法（纯代码实现）
    ///
    /// 默认实现：优先使用 ctx.flow_template（API 请求传入的模板），
    /// 如果未指定，则使用 skill.flow_template()（Skill 默认模板）
    async fn execute(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // 优先使用请求传入的模板，否则使用 Skill 默认模板
        let template = ctx.flow_template.or_else(|| self.flow_template());

        match template {
            Some(FlowTemplate::Simple) => self.execute_simple(ctx).await,
            Some(FlowTemplate::ReAct) => self.execute_react(ctx).await,
            Some(FlowTemplate::PlanAct) => self.execute_plan_act(ctx).await,
            Some(FlowTemplate::ChainOfThought) => self.execute_chain_of_thought(ctx).await,
            None => Err(anyhow::anyhow!(
                "Skill {} did not select a flow template and did not override execute()",
                self.name()
            )),
        }
    }

    /// Simple 流程执行方法
    ///
    /// 适合简单直接的任务，如直接调用工具
    /// 需要在 flow_template() 中返回 FlowTemplate::Simple
    async fn execute_simple(&self, _ctx: &mut SkillContext<'_>) -> Result<String> {
        Err(anyhow::anyhow!(
            "Skill {} selected Simple template but did not implement execute_simple()",
            self.name()
        ))
    }

    /// ReAct 流程执行方法
    ///
    /// 适合需要多轮思考和工具调用的场景
    /// 需要在 flow_template() 中返回 FlowTemplate::ReAct
    async fn execute_react(&self, _ctx: &mut SkillContext<'_>) -> Result<String> {
        Err(anyhow::anyhow!(
            "Skill {} selected ReAct template but did not implement execute_react()",
            self.name()
        ))
    }

    /// PlanAct 流程执行方法
    ///
    /// 适合需要先规划再执行的复杂任务
    /// 需要在 flow_template() 中返回 FlowTemplate::PlanAct
    async fn execute_plan_act(&self, _ctx: &mut SkillContext<'_>) -> Result<String> {
        Err(anyhow::anyhow!(
            "Skill {} selected PlanAct template but did not implement execute_plan_act()",
            self.name()
        ))
    }

    /// Chain-of-Thought 流程执行方法
    ///
    /// 适合需要复杂推理的场景
    /// 需要在 flow_template() 中返回 FlowTemplate::ChainOfThought
    async fn execute_chain_of_thought(&self, _ctx: &mut SkillContext<'_>) -> Result<String> {
        Err(anyhow::anyhow!(
            "Skill {} selected ChainOfThought template but did not implement execute_chain_of_thought()",
            self.name()
        ))
    }

    /// 流式执行方法（纯代码实现）
    ///
    /// callback: 每收到一块数据时调用
    /// 默认实现：调用非流式 execute 方法，然后一次性返回结果
    /// Skill 可以重写此方法实现真正的流式输出
    async fn execute_streaming(
        &self,
        ctx: &mut SkillContext<'_>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<String> {
        let result = self.execute(ctx).await?;
        callback(result.clone());
        Ok(result)
    }

    /// 是否支持流式输出
    ///
    /// 返回 true 表示 Skill 支持流式输出
    /// 返回 false 表示需要先完成工具调用等操作后再输出
    fn supports_streaming(&self) -> bool {
        false
    }

    /// 优先级（数字越小优先级越高）
    fn priority(&self) -> i32 {
        0
    }

    // ============ 向后兼容：保留旧的 flow_steps 方法 ============
    // 以下方法已废弃，仅用于向后兼容

    /// 预设的流程步骤（已废弃）
    #[deprecated(note = "请使用 execute_xxx() 方法代替")]
    fn flow_steps(&self) -> Vec<FlowStep> {
        Vec::new()
    }

    /// 是否需要 LLM 前置处理（已废弃）
    #[deprecated(note = "请使用 flow_template 选择预设模板")]
    fn requires_llm_preprocess(&self) -> bool {
        false
    }
}

/// Skill 信息
#[derive(Debug, Clone)]
pub struct SkillInfo {
    /// Skill 名称
    pub name: String,
    /// Skill 描述
    pub description: String,
    /// 优先级
    pub priority: i32,
    /// 当前使用的流程模板
    pub flow_template: Option<FlowTemplate>,
    /// 所有实现的流程模板版本
    pub flow_templates: Vec<FlowTemplate>,
}

/// Skill 匹配结果
pub struct SkillMatch {
    /// 匹配的 Skill
    pub skill: Arc<dyn Skill>,
    /// 匹配度
    pub confidence: f32,
    /// Skill 信息
    pub info: SkillInfo,
}

impl Clone for SkillMatch {
    fn clone(&self) -> Self {
        SkillMatch {
            skill: self.skill.clone(),
            confidence: self.confidence,
            info: self.info.clone(),
        }
    }
}

impl std::fmt::Debug for SkillMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillMatch")
            .field("confidence", &self.confidence)
            .field("info", &self.info)
            .finish()
    }
}

/// Skill 管理器
///
/// 使用 HashMap + 关键词索引优化大规模 Skill 匹配性能
/// 支持 1000+ Skill 的高效查找和匹配
pub struct SkillManager {
    /// 名称索引（HashMap，O(1) 查找）
    skills_by_name: HashMap<String, Arc<dyn Skill>>,
    /// 关键词倒排索引（关键词 -> Skill 列表）
    keyword_index: HashMap<String, Vec<Arc<dyn Skill>>>,
    /// 所有 Skill 列表（用于无关键词匹配时的遍历）
    skills: Vec<Arc<dyn Skill>>,
    /// 匹配阈值（低于此值不触发 Skill）
    match_threshold: f32,
    /// 是否启用 LLM 回退（当没有 Skill 匹配时）
    fallback_to_llm: bool,
}

impl std::fmt::Debug for SkillManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let skill_names: Vec<String> = self.skills.iter().map(|s| s.name().to_string()).collect();
        f.debug_struct("SkillManager")
            .field("skills", &skill_names)
            .field("skill_count", &self.skills.len())
            .field("keyword_count", &self.keyword_index.len())
            .field("match_threshold", &self.match_threshold)
            .field("fallback_to_llm", &self.fallback_to_llm)
            .finish()
    }
}

impl SkillManager {
    /// 创建新的 SkillManager
    pub fn new() -> Self {
        Self {
            skills_by_name: HashMap::new(),
            keyword_index: HashMap::new(),
            skills: Vec::new(),
            match_threshold: 0.0,
            fallback_to_llm: true,
        }
    }

    /// 设置匹配阈值
    pub fn set_match_threshold(&mut self, threshold: f32) {
        self.match_threshold = threshold;
    }

    /// 设置是否启用 LLM 回退
    pub fn set_fallback_to_llm(&mut self, enabled: bool) {
        self.fallback_to_llm = enabled;
    }

    /// 注册 Skill（自动构建索引）
    pub fn register(&mut self, skill: impl Skill) {
        let skill_arc = Arc::new(skill);
        let name = skill_arc.name().to_string();

        // 1. 添加到名称索引（O(1) 查找）
        self.skills_by_name.insert(name.clone(), skill_arc.clone());

        // 2. 添加到关键词索引（倒排索引）
        for keyword in skill_arc.keywords() {
            self.keyword_index
                .entry(keyword)
                .or_default()
                .push(skill_arc.clone());
        }

        // 3. 添加到列表
        self.skills.push(skill_arc.clone());

        // 4. 按优先级排序
        self.skills.sort_by_key(|a| a.priority());

        tracing::info!(
            "Registered skill: {} (keywords: {})",
            name,
            skill_arc.keywords().len()
        );
    }

    /// 批量注册 Skill
    pub fn register_many(&mut self, skills: Vec<impl Skill + 'static>) {
        for skill in skills {
            self.register(skill);
        }
    }

    /// 注册 Box<dyn Skill>（用于动态注册，如专家插件中的技能）
    pub fn register_boxed(&mut self, skill: Box<dyn Skill>) {
        let skill_arc: Arc<dyn Skill> = skill.into();
        let name = skill_arc.name().to_string();

        // 1. 添加到名称索引
        self.skills_by_name.insert(name.clone(), skill_arc.clone());

        // 2. 添加到关键词索引
        for keyword in skill_arc.keywords() {
            self.keyword_index
                .entry(keyword)
                .or_default()
                .push(skill_arc.clone());
        }

        // 3. 添加到列表
        self.skills.push(skill_arc.clone());

        // 4. 按优先级排序
        self.skills.sort_by_key(|a| a.priority());

        tracing::info!(
            "Registered skill (boxed): {} (keywords: {})",
            name,
            skill_arc.keywords().len()
        );
    }

    /// 获取所有 Skill 信息
    pub fn get_skills(&self) -> Vec<SkillInfo> {
        self.skills
            .iter()
            .map(|s| SkillInfo {
                name: s.name().to_string(),
                description: s.description().to_string(),
                priority: s.priority(),
                flow_template: s.flow_template(),
                flow_templates: s.supported_templates(),
            })
            .collect()
    }

    /// 通过名称获取 Skill（O(1) HashMap 查找）
    pub fn get_skill_by_name(&self, name: &str) -> Option<SkillMatch> {
        self.skills_by_name.get(name).map(|s| SkillMatch {
            skill: s.clone(),
            confidence: 1.0,
            info: SkillInfo {
                name: s.name().to_string(),
                description: s.description().to_string(),
                priority: s.priority(),
                flow_template: s.flow_template(),
                flow_templates: s.supported_templates(),
            },
        })
    }

    /// 检查 Skill 是否存在（O(1) HashMap 查找）
    pub fn has_skill(&self, name: &str) -> bool {
        self.skills_by_name.contains_key(name)
    }

    /// 匹配 Skill（使用关键词索引优化）
    ///
    /// 优化流程：
    /// 1. 先通过关键词索引快速筛选候选 Skill（O(k)）
    /// 2. 再对候选 Skill 计算精确匹配度
    /// 3. 如果关键词索引无匹配，遍历所有 Skill（兜底）
    pub fn match_skill(&self, input: &str) -> Option<SkillMatch> {
        // 步骤1：通过关键词索引筛选候选 Skill
        let candidate_skills = self.get_candidate_skills(input);

        // 步骤2：对候选 Skill 计算精确匹配度
        let mut best_match: Option<(Arc<dyn Skill>, f32)> = None;

        for skill in candidate_skills {
            let confidence = skill.matches(input);

            if confidence >= self.match_threshold {
                match best_match {
                    None => {
                        best_match = Some((skill.clone(), confidence));
                    }
                    Some((_, current_confidence)) => {
                        if confidence > current_confidence {
                            best_match = Some((skill.clone(), confidence));
                        } else if confidence == current_confidence {
                            // 相同匹配度时比较优先级
                            if skill.priority() < best_match.as_ref().unwrap().0.priority() {
                                best_match = Some((skill.clone(), confidence));
                            }
                        }
                    }
                }
            }
        }

        best_match.map(|(skill, confidence)| {
            let info = SkillInfo {
                name: skill.name().to_string(),
                description: skill.description().to_string(),
                priority: skill.priority(),
                flow_template: skill.flow_template(),
                flow_templates: skill.supported_templates(),
            };
            SkillMatch {
                skill,
                confidence,
                info,
            }
        })
    }

    /// 通过关键词索引获取候选 Skill
    ///
    /// O(k) 查找，k 是输入中的关键词数量
    fn get_candidate_skills(&self, input: &str) -> Vec<Arc<dyn Skill>> {
        // 尝试通过关键词索引查找
        let mut candidates: Vec<Arc<dyn Skill>> = Vec::new();

        // 对输入进行分词（简单实现：按空格和常见分隔符）
        let words = self.tokenize_input(input);

        // 通过关键词索引查找
        for word in words {
            if let Some(skills) = self.keyword_index.get(&word) {
                for skill in skills {
                    // 避免重复添加
                    if !candidates.iter().any(|s| s.name() == skill.name()) {
                        candidates.push(skill.clone());
                    }
                }
            }
        }

        // 如果关键词索引有匹配，返回候选列表
        if !candidates.is_empty() {
            tracing::debug!(
                "Keyword index matched {} candidates for input: {}",
                candidates.len(),
                input
            );
            return candidates;
        }

        // 如果关键词索引无匹配，返回所有 Skill（兜底）
        tracing::debug!("No keyword match, using all skills as candidates");
        self.skills.clone()
    }

    /// 输入分词（简单实现）
    ///
    /// 将输入拆分为关键词，用于索引查找
    fn tokenize_input(&self, input: &str) -> Vec<String> {
        // 简单分词：提取中文词汇和英文单词
        let mut tokens = Vec::new();

        // 提取中文词汇（连续的中文字符）
        let chinese_chars: Vec<char> = input.chars().collect();
        let mut chinese_word = String::new();
        for ch in chinese_chars {
            if ch.is_ascii() {
                // 非中文字符，保存当前中文词汇
                if !chinese_word.is_empty() {
                    tokens.push(chinese_word.clone());
                    chinese_word.clear();
                }
                // 英文单词
                if ch.is_ascii_alphabetic() {
                    let mut english_word = String::new();
                    english_word.push(ch);
                    tokens.push(english_word);
                }
            } else {
                // 中文字符
                chinese_word.push(ch);
            }
        }
        if !chinese_word.is_empty() {
            tokens.push(chinese_word);
        }

        // 添加原始输入（用于精确匹配）
        tokens.push(input.to_string());

        tokens
    }

    /// 匹配所有符合条件的 Skill（使用关键词索引优化）
    pub fn match_all_skills(&self, input: &str) -> Vec<SkillMatch> {
        let candidate_skills = self.get_candidate_skills(input);

        let mut matches: Vec<SkillMatch> = candidate_skills
            .iter()
            .filter(|s| s.matches(input) >= self.match_threshold)
            .map(|s| SkillMatch {
                skill: s.clone(),
                confidence: s.matches(input),
                info: SkillInfo {
                    name: s.name().to_string(),
                    description: s.description().to_string(),
                    priority: s.priority(),
                    flow_template: s.flow_template(),
                    flow_templates: s.supported_templates(),
                },
            })
            .collect();

        // 按匹配度和优先级排序
        matches.sort_by(|a, b| {
            if a.confidence != b.confidence {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                a.info.priority.cmp(&b.info.priority)
            }
        });

        matches
    }

    /// 获取匹配的 Skill 及其流程模板
    ///
    /// 返回匹配的 Skill 和它的流程模板
    /// 如果没有匹配的 Skill，使用默认聊天 Skill
    pub fn get_matched_skill(&self, input: &str) -> Option<(SkillMatch, Option<FlowTemplate>)> {
        // 尝试匹配所有 Skill
        let matches = self.match_all_skills(input);

        tracing::debug!("Input: {}, Matched skills count: {}", input, matches.len());

        if let Some(skill_match) = matches.first() {
            // 有匹配的 Skill，使用第一个（匹配度最高）
            tracing::info!(
                "Using matched skill: {} (confidence: {:.2})",
                skill_match.info.name,
                skill_match.confidence
            );
            let template = skill_match.skill.flow_template();
            Some(((*skill_match).clone(), template))
        } else {
            // 没有匹配的 Skill，使用默认聊天 Skill
            tracing::debug!("No skill matched, checking for default_chat...");
            if let Some(default_skill) = self.get_skill_by_name("default_chat") {
                tracing::info!("Using default_chat skill");
                Some((default_skill, None))
            } else {
                // 没有默认聊天 Skill，返回 None
                tracing::warn!("No skill matched and no default_chat skill available");
                None
            }
        }
    }

    /// 执行匹配的 Skill
    ///
    /// 分层设计：
    /// - run_ctx: 请求级上下文（session、tokens、chain）
    /// - runtime: 全局运行时（LLM、工具）
    /// - memory: 全局记忆系统
    ///
    /// Token 统计通过 Arc 共享，从 run_ctx.tokens 获取
    pub async fn execute_skill(
        &self,
        skill_match: &SkillMatch,
        input: &str,
        run_ctx: &mut RunContext,
        runtime: &Runtime,
        memory: &Memory,
        flow_template: Option<FlowTemplate>,
    ) -> Result<String> {
        // 优先使用传入的模板，否则使用 Skill 默认模板
        let template = flow_template.or_else(|| skill_match.skill.flow_template());

        // 添加到调用链（先添加，避免借用冲突）
        run_ctx.add_to_chain(&skill_match.info.name);

        // 从 RunContext 创建 SkillContext（共享 tokens）
        let mut ctx = SkillContext::from_run_context(
            input,
            run_ctx,
            runtime,
            memory,
            skill_match.confidence,
            template,
        );

        // 执行 Skill 的纯代码实现
        skill_match.skill.execute(&mut ctx).await
    }

    /// 流式执行匹配的 Skill
    ///
    /// callback: 每收到一块数据时调用
    pub async fn execute_skill_streaming(
        &self,
        skill_match: &SkillMatch,
        input: &str,
        run_ctx: &mut RunContext,
        runtime: &Runtime,
        memory: &Memory,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<String> {
        // 添加到调用链
        run_ctx.add_to_chain(&skill_match.info.name);

        // 从 RunContext 创建 SkillContext
        let mut ctx = SkillContext::from_run_context(
            input,
            run_ctx,
            runtime,
            memory,
            skill_match.confidence,
            skill_match.skill.flow_template(),
        );

        // 执行 Skill 的流式实现
        skill_match
            .skill
            .execute_streaming(&mut ctx, callback)
            .await
    }

    /// 检查是否有匹配的 Skill
    pub fn has_match(&self, input: &str) -> bool {
        self.match_skill(input).is_some()
    }
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 内置示例 Skill：天气查询
///
/// 搜索长期记忆 Skill
///
/// 当用户提到之前聊过的内容、而当前对话里没有相关信息时调用
/// 这是"记忆迷宫"核心理念的体现：AI 主动检索历史，而不是框架无脑塞上下文
#[derive(Debug)]
pub struct SearchLongMemorySkill;

#[async_trait]
impl Skill for SearchLongMemorySkill {
    fn name(&self) -> &str {
        "search_long_memory"
    }

    fn description(&self) -> &str {
        "检索更早的历史对话记忆。当用户提到之前聊过的内容、而当前对话里没有相关信息时调用。"
    }

    fn matches(&self, input: &str) -> f32 {
        // 只有当用户明确提到"之前"、"以前"、"上次"、"曾经"等词时才触发
        let keywords = [
            "之前",
            "以前",
            "上次",
            "曾经",
            "那时候",
            "之前聊过",
            "刚才说",
        ];
        let matches: usize = keywords.iter().filter(|k| input.contains(*k)).count();

        if matches > 0 {
            0.7 + (matches as f32 - 1.0) * 0.1
        } else {
            0.0
        }
    }

    fn priority(&self) -> i32 {
        100 // 高优先级，确保在普通 Skill 之前检查
    }

    /// 使用预设 ReAct 流程模板
    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::ReAct)
    }

    /// 纯代码实现：检索长期记忆
    async fn execute(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // 步骤1：检索长期记忆（归档）
        let memory_results = ctx.memory.search_archive(ctx.input, 5);

        // 步骤2：构建上下文
        let memory_context = if memory_results.is_empty() {
            "没有找到相关历史记录".to_string()
        } else {
            memory_results
                .iter()
                .map(|r| r.item.content.clone())
                .collect::<Vec<_>>()
                .join("\n")
        };

        // 步骤3：调用 LLM 生成回复
        let response = ctx
            .call_llm(vec![crate::runtime::llm::Message::user(format!(
                "根据检索到的历史对话记录：\n{}\n\n回答用户的问题：{}",
                memory_context, ctx.input
            ))])
            .await?;

        Ok(response)
    }
}

/// 默认聊天 Skill
///
/// 这是一个兜底的 Skill，用于处理普通对话
/// 当没有其他 Skill 匹配时，自动使用此 Skill
#[derive(Debug)]
pub struct DefaultChatSkill;

#[async_trait]
impl Skill for DefaultChatSkill {
    fn name(&self) -> &str {
        "default_chat"
    }

    fn description(&self) -> &str {
        "默认聊天技能，用于处理普通对话"
    }

    fn matches(&self, input: &str) -> f32 {
        // 始终匹配，但优先级最低
        // 只有当没有其他 Skill 匹配时才会被使用
        // 返回一个低匹配度，确保其他 Skill 优先
        let _ = input; // 忽略输入
        0.05
    }

    fn priority(&self) -> i32 {
        // 最低优先级，确保其他 Skill 优先匹配
        i32::MAX
    }

    /// 不使用预设模板，完全自定义流程
    fn flow_template(&self) -> Option<FlowTemplate> {
        None
    }

    /// 纯代码实现：简单对话
    async fn execute(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // 步骤1：获取短期记忆上下文
        let memory_context = ctx.get_memory("recent");
        let context_hint = memory_context
            .map(|m| format!("最近对话：\n{}\n\n", m))
            .unwrap_or_default();

        // 步骤2：调用 LLM 生成回复
        let response = ctx
            .call_llm(vec![crate::runtime::llm::Message::user(format!(
                "{}{}",
                context_hint, ctx.input
            ))])
            .await?;

        Ok(response)
    }
}

/// 天气查询 Skill
///
/// 使用预设 Simple 流程模板
#[derive(Debug)]
pub struct WeatherSkill;

#[async_trait]
impl Skill for WeatherSkill {
    fn name(&self) -> &str {
        "weather"
    }

    fn description(&self) -> &str {
        "查询天气信息"
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = ["天气", "温度", "气温", "晴", "雨", "雪", "预报"];
        let matches: usize = keywords.iter().filter(|k| input.contains(*k)).count();

        if matches == 0 {
            0.0
        } else if matches == 1 {
            0.6
        } else {
            0.8 + (matches as f32 - 2.0) * 0.1
        }
    }

    /// 关键词列表（用于索引优化）
    fn keywords(&self) -> Vec<String> {
        vec![
            "天气".to_string(),
            "温度".to_string(),
            "气温".to_string(),
            "晴".to_string(),
            "雨".to_string(),
            "雪".to_string(),
            "预报".to_string(),
        ]
    }

    /// 使用预设 Simple 流程模板
    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::Simple)
    }

    /// Simple 流程实现：查询天气
    async fn execute_simple(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // 步骤1：从输入提取城市
        let city = extract_city(ctx.input).unwrap_or_else(|| "北京".to_string());

        // 步骤2：调用天气工具（如果有）
        let weather_data = match ctx
            .call_tool("get_weather", serde_json::json!({"city": city}))
            .await
        {
            Ok(data) => data,
            Err(_) => {
                // 如果工具不可用，从知识库查询
                let results = ctx.memory.search_knowledge(&format!("{}天气", city), 3);
                if results.is_empty() {
                    "暂无天气数据".to_string()
                } else {
                    results
                        .iter()
                        .map(|r| r.item.content.clone())
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
        };

        // 步骤3：调用 LLM 生成友好回复
        let response = ctx
            .call_llm(vec![crate::runtime::llm::Message::user(format!(
                "根据天气数据：\n{}\n\n用友好的语言回答用户的问题：{}",
                weather_data, ctx.input
            ))])
            .await?;

        Ok(response)
    }
}

/// 计算器 Skill
///
/// 使用预设 ReAct 流程模板（Reasoning + Acting）
/// ReAct 流程：思考 → 行动 → 观察 → 反思
#[derive(Debug)]
pub struct CalculatorSkill;

#[async_trait]
impl Skill for CalculatorSkill {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "执行数学计算。支持加减乘除等基本运算。"
    }

    fn matches(&self, input: &str) -> f32 {
        // 检查是否包含数学表达式
        let math_patterns = ["+", "-", "*", "/", "等于", "计算", "求和", "乘以", "除以"];
        let has_number = input.chars().any(|c| c.is_ascii_digit());
        let has_operator = math_patterns.iter().any(|p| input.contains(*p));

        if has_number && has_operator {
            0.85
        } else if has_operator {
            0.5
        } else {
            0.0
        }
    }

    /// 关键词列表（用于索引优化）
    fn keywords(&self) -> Vec<String> {
        vec![
            "计算".to_string(),
            "加".to_string(),
            "减".to_string(),
            "乘".to_string(),
            "除".to_string(),
            "等于".to_string(),
            "求和".to_string(),
            "乘以".to_string(),
            "除以".to_string(),
        ]
    }

    /// 使用预设 ReAct 流程模板
    /// ReAct = Reasoning + Acting，适合需要多轮思考和工具调用的场景
    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::ReAct)
    }

    /// ReAct 流程实现：思考 → 行动 → 观察 → 反思
    async fn execute_react(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // ========== ReAct Step 1: Reasoning（思考）==========
        // 分析用户输入，确定需要执行的操作
        tracing::info!(
            "[CalculatorSkill] ReAct Step 1 - Reasoning: 用户输入: {}",
            ctx.input
        );

        // 步骤1.1：使用 LLM 提取数学表达式
        tracing::info!("[CalculatorSkill] ReAct Step 1.1 - 提取数学表达式");
        let extract_prompt = format!(
            "从以下文本中提取纯数学表达式（只返回表达式本身，如 8+9 或 2*3，不要任何解释）：\n{}",
            ctx.input
        );

        let extracted_expr = ctx
            .call_llm(vec![crate::runtime::llm::Message::user(extract_prompt)])
            .await?;

        let expression = extracted_expr.trim().to_string();

        if expression.is_empty() {
            return Ok(
                "未能从输入中提取有效的数学表达式，请提供包含数字和运算符的问题。".to_string(),
            );
        }

        tracing::info!(
            "[CalculatorSkill] ReAct Step 1.2 - 提取结果: {}",
            expression
        );

        // ========== ReAct Step 2: Acting（行动）==========
        // 调用计算器工具执行计算
        tracing::info!(
            "[CalculatorSkill] ReAct Step 2 - Acting: 调用计算器工具，表达式: {}",
            expression
        );

        let tool_result = ctx
            .call_tool("calculate", serde_json::json!({"expression": expression}))
            .await?;

        // ========== ReAct Step 3: Observing（观察）==========
        // 检查工具返回结果
        tracing::info!(
            "[CalculatorSkill] ReAct Step 3 - Observing: 工具返回结果: {}",
            tool_result
        );

        // 解析工具返回结果
        let calculation_result = match serde_json::from_str::<serde_json::Value>(&tool_result) {
            Ok(value) => {
                if let Some(result) = value.get("result") {
                    result.to_string()
                } else if let Some(error) = value.get("error") {
                    return Ok(format!("计算出错: {}", error));
                } else {
                    tool_result.clone()
                }
            }
            Err(_) => {
                // 如果不是 JSON 格式，直接使用返回值
                tool_result.clone()
            }
        };

        // ========== ReAct Step 4: Reflecting（反思）==========
        // 总结结果，决定是否需要进一步处理
        tracing::info!("[CalculatorSkill] ReAct Step 4 - Reflecting: 总结结果");

        // 构建最终回复
        let final_response = format!(
            "计算完成！\n\n问题：{}\n表达式：{}\n结果：{}",
            ctx.input, expression, calculation_result
        );

        // 记录到记忆（可选）
        ctx.set_memory("last_calculation", &final_response);

        Ok(final_response)
    }
}

/// 示例：订单 Skill（完全自定义流程）
///
/// 展示如何使用纯代码实现复杂的业务逻辑
/// 不使用预设模板，完全自定义流程
#[derive(Debug)]
pub struct OrderSkill;

#[async_trait]
impl Skill for OrderSkill {
    fn name(&self) -> &str {
        "order"
    }

    fn description(&self) -> &str {
        "处理订单相关业务"
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = ["下单", "订单", "购买", "购买", "买东西"];
        let matches: usize = keywords.iter().filter(|k| input.contains(*k)).count();

        if matches > 0 {
            0.8 + (matches as f32 - 1.0) * 0.1
        } else {
            0.0
        }
    }

    /// 不使用预设模板，完全自定义
    fn flow_template(&self) -> Option<FlowTemplate> {
        None // None 表示完全自定义流程
    }

    /// 纯代码实现：复杂的订单业务逻辑
    async fn execute(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // 步骤1：解析订单信息（使用 LLM）
        let order_info = ctx
            .call_llm(vec![crate::runtime::llm::Message::user(format!(
                "从以下文本中提取订单信息（商品、数量、用户等），返回 JSON 格式：\n{}",
                ctx.input
            ))])
            .await?;

        // 步骤2：检查库存（调用工具）
        let inventory_result = ctx
            .call_tool("check_inventory", serde_json::json!({"order": order_info}))
            .await?;

        // 步骤3：业务逻辑判断
        if inventory_result.contains("库存不足") {
            // 库存不足，返回提示
            return Ok(format!("抱歉，{}", inventory_result));
        }

        // 步骤4：创建订单（调用工具）
        let order_result = ctx
            .call_tool("create_order", serde_json::json!({"order": order_info}))
            .await?;

        // 步骤5：发送通知（可选）
        if order_result.contains("成功") {
            // 记录到记忆
            ctx.set_memory("last_order", &order_result);
        }

        Ok(format!("订单处理结果：{}", order_result))
    }
}

/// 辅助函数：提取城市名
fn extract_city(input: &str) -> Option<String> {
    let cities = [
        "北京", "上海", "广州", "深圳", "杭州", "成都", "武汉", "南京",
    ];
    cities
        .iter()
        .find(|c| input.contains(*c))
        .cloned()
        .map(|s| s.to_string())
}

/// 辅助函数：提取数学表达式（预留方法）
#[allow(dead_code)]
fn extract_expression(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_digit() || "+-*/().".contains(*c))
        .collect()
}

/// 文件操作 Skill
///
/// 支持读取文件、列出目录、写入文件等操作
#[derive(Debug)]
pub struct FileOperationSkill;

#[async_trait]
impl Skill for FileOperationSkill {
    fn name(&self) -> &str {
        "file_operation"
    }

    fn description(&self) -> &str {
        "文件操作：读取文件、列出目录、查看文件内容"
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = [
            "读取文件",
            "查看文件",
            "列出目录",
            "文件内容",
            "读文件",
            "目录列表",
        ];
        let matches: usize = keywords.iter().filter(|k| input.contains(*k)).count();

        if matches == 0 {
            0.0
        } else if matches == 1 {
            0.7
        } else {
            0.9
        }
    }

    fn keywords(&self) -> Vec<String> {
        vec![
            "读取文件".to_string(),
            "查看文件".to_string(),
            "列出目录".to_string(),
            "文件内容".to_string(),
            "读文件".to_string(),
            "目录".to_string(),
        ]
    }

    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::Simple)
    }

    async fn execute_simple(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        use std::fs;
        use std::path::Path;

        let input = ctx.input;

        // 尝试提取路径
        let path = extract_path(input).unwrap_or_else(|| ".".to_string());
        let path = Path::new(&path);

        // 判断操作类型
        if input.contains("列出") || input.contains("列表") || input.contains("目录") {
            // 列出目录
            match fs::read_dir(path) {
                Ok(entries) => {
                    let mut files: Vec<String> = Vec::new();
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let file_type = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                            "[目录]"
                        } else {
                            "[文件]"
                        };
                        files.push(format!("{} {}", file_type, name));
                    }
                    files.sort();
                    Ok(format!(
                        "目录 {} 的内容：\n{}",
                        path.display(),
                        files.join("\n")
                    ))
                }
                Err(e) => Ok(format!("读取目录失败：{}", e)),
            }
        } else {
            // 读取文件
            match fs::read_to_string(path) {
                Ok(content) => {
                    let preview = if content.len() > 2000 {
                        format!(
                            "{}\n... (文件过长，已截断，共 {} 字符)",
                            &content[..2000],
                            content.len()
                        )
                    } else {
                        content
                    };
                    Ok(format!("文件 {} 内容：\n{}", path.display(), preview))
                }
                Err(e) => Ok(format!("读取文件失败：{}", e)),
            }
        }
    }
}

fn extract_path(input: &str) -> Option<String> {
    // 简单的路径提取：查找常见路径模式
    let patterns = ["./", "../", "/", "~/"];
    for pattern in patterns {
        if let Some(idx) = input.find(pattern) {
            let rest = &input[idx..];
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '，' || c == '。' || c == '？')
                .unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// 网络搜索 Skill
///
/// 调用搜索引擎查询信息
#[derive(Debug)]
pub struct WebSearchSkill;

#[async_trait]
impl Skill for WebSearchSkill {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "网络搜索：搜索最新资讯、查询资料"
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = [
            "搜索",
            "查询",
            "百度",
            "谷歌",
            "最新",
            "资讯",
            "新闻",
            "怎么",
            "如何",
            "什么是",
        ];
        let matches: usize = keywords.iter().filter(|k| input.contains(*k)).count();

        if matches == 0 {
            0.0
        } else if matches == 1 {
            0.5
        } else {
            0.8
        }
    }

    fn keywords(&self) -> Vec<String> {
        vec![
            "搜索".to_string(),
            "查询".to_string(),
            "百度".to_string(),
            "谷歌".to_string(),
            "最新".to_string(),
            "资讯".to_string(),
        ]
    }

    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::Simple)
    }

    async fn execute_simple(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // 尝试调用搜索工具
        match ctx
            .call_tool("web_search", serde_json::json!({"query": ctx.input}))
            .await
        {
            Ok(result) => Ok(result.to_string()),
            Err(_) => {
                // 如果工具不可用，从记忆中搜索
                let results = ctx.memory.search_archive(ctx.input, 5);
                if results.is_empty() {
                    Ok("暂未配置网络搜索功能，且知识库中未找到相关信息。".to_string())
                } else {
                    let formatted: Vec<String> = results
                        .iter()
                        .map(|r| format!("• {}", r.item.content))
                        .collect();
                    Ok(format!(
                        "从知识库中找到相关信息：\n{}",
                        formatted.join("\n")
                    ))
                }
            }
        }
    }
}

/// 代码执行 Skill
///
/// 执行简单的代码片段（支持 Python、JavaScript 等）
#[derive(Debug)]
pub struct CodeExecutionSkill;

#[async_trait]
impl Skill for CodeExecutionSkill {
    fn name(&self) -> &str {
        "code_execution"
    }

    fn description(&self) -> &str {
        "代码执行：运行代码、计算结果"
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = [
            "运行代码",
            "执行代码",
            "跑一下",
            "代码",
            "python",
            "javascript",
            "js ",
        ];
        let has_code_block = input.contains("```");
        let matches: usize = keywords
            .iter()
            .filter(|k| input.to_lowercase().contains(*k))
            .count();

        let mut score = 0.0;
        if has_code_block {
            score += 0.5;
        }
        if matches > 0 {
            score += 0.3 + (matches as f32 * 0.1);
        }
        score.min(0.9)
    }

    fn keywords(&self) -> Vec<String> {
        vec![
            "运行代码".to_string(),
            "执行代码".to_string(),
            "代码".to_string(),
            "python".to_string(),
            "javascript".to_string(),
        ]
    }

    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::Simple)
    }

    async fn execute_simple(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        // 尝试调用代码执行工具
        match ctx
            .call_tool("execute_code", serde_json::json!({"code": ctx.input}))
            .await
        {
            Ok(result) => Ok(result.to_string()),
            Err(_) => Ok(
                "代码执行功能暂未配置。如需执行代码，请配置代码执行工具（如 Docker 沙箱）。"
                    .to_string(),
            ),
        }
    }
}

/// 定时提醒 Skill
///
/// 设置提醒、倒计时、定时任务
#[derive(Debug)]
pub struct ReminderSkill;

#[async_trait]
impl Skill for ReminderSkill {
    fn name(&self) -> &str {
        "reminder"
    }

    fn description(&self) -> &str {
        "定时提醒：设置提醒、倒计时、待办提醒"
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = [
            "提醒",
            "闹钟",
            "定时",
            "倒计时",
            "待办",
            "记得",
            "到点",
            "分钟后",
            "小时后",
        ];
        let matches: usize = keywords.iter().filter(|k| input.contains(*k)).count();

        if matches == 0 {
            0.0
        } else if matches == 1 {
            0.7
        } else {
            0.9
        }
    }

    fn keywords(&self) -> Vec<String> {
        vec![
            "提醒".to_string(),
            "闹钟".to_string(),
            "定时".to_string(),
            "倒计时".to_string(),
            "待办".to_string(),
            "记得".to_string(),
        ]
    }

    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::Simple)
    }

    async fn execute_simple(&self, ctx: &mut SkillContext<'_>) -> Result<String> {
        let input = ctx.input;

        // 简单提取时间（分钟）
        let minutes = extract_minutes(input);
        let content = extract_reminder_content(input);

        if let Some(mins) = minutes {
            let msg = if mins > 0 {
                format!("好的！我会在 {} 分钟后提醒你：{}", mins, content)
            } else {
                format!("好的！已记录提醒：{}", content)
            };

            // 尝试调用提醒工具
            if let Ok(result) = ctx
                .call_tool(
                    "set_reminder",
                    serde_json::json!({
                        "minutes": mins,
                        "content": content
                    }),
                )
                .await
            {
                return Ok(result.to_string());
            }

            // 如果没有工具，模拟设置（仅记录到记忆）
            let reminder_text = format!("提醒：{}（{} 分钟后）", content, mins);
            let _ = ctx.memory.write_short_term(reminder_text, "reminder");

            Ok(msg)
        } else {
            Ok("已收到你的提醒请求。请告诉我具体时间，例如：'30分钟后提醒我开会'。".to_string())
        }
    }
}

fn extract_minutes(input: &str) -> Option<u64> {
    // 简单提取分钟数
    let patterns = [("分钟后", 1), ("分钟", 1), ("小时后", 60), ("小时", 60)];

    for (pattern, multiplier) in patterns {
        if let Some(idx) = input.find(pattern) {
            let before = &input[..idx];
            let num_str: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();

            if let Ok(num) = num_str.parse::<u64>() {
                return Some(num * multiplier);
            }
        }
    }

    None
}

fn extract_reminder_content(input: &str) -> String {
    // 简单提取提醒内容
    let separators = ["提醒我", "提醒", "叫我", "告诉我"];
    for sep in separators {
        if let Some(idx) = input.find(sep) {
            let content = &input[idx + sep.len()..];
            return content
                .trim_end_matches(['。', '！', '？'])
                .trim()
                .to_string();
        }
    }
    input.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weather_skill_matches() {
        let skill = WeatherSkill;
        assert!(skill.matches("今天天气怎么样") > 0.5);
        assert!(skill.matches("北京温度") > 0.5);
        assert_eq!(skill.matches("你好"), 0.0);
    }

    #[test]
    fn test_calculator_skill_matches() {
        let skill = CalculatorSkill;
        assert!(skill.matches("2 + 3") > 0.5);
        assert!(skill.matches("计算10乘以5") > 0.5);
        assert_eq!(skill.matches("你好"), 0.0);
    }

    #[test]
    fn test_skill_manager() {
        let mut manager = SkillManager::new();
        manager.register(WeatherSkill);
        manager.register(CalculatorSkill);

        assert_eq!(manager.get_skills().len(), 2);

        let matches = manager.match_all_skills("北京天气");
        assert!(!matches.is_empty());
        assert_eq!(matches[0].info.name, "weather");
    }
}
