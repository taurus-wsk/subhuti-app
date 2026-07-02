//! # 多 Agent 协作层 (Orchestrator)
//!
//! ## 核心架构
//! 注册式预定义链路架构（类似 Rust 的函数签名）：
//! - 每个链路是显式注册的，步骤和条件都是写死的
//! - 约束层是纯 predicate（输入校验、输出校验）
//! - AI 只在新增链路时介入：生成符合规范的新链代码
//!
//! ## 调度流程（完全确定性，零 AI 调用）
//! match_condition(input) → 找到 Chain → validate(input) → execute chain steps → validate_output(result) → return

use crate::context::TokenStats;
use crate::observe::Trace;
use crate::runtime::{Message, Role, Runtime};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

// ─── 类型别名 ──────────────────────────────────────────────

pub type AgentId = String;
pub type CtxId = String;
pub type ChainName = String;

// ─── 上下文存储（享元模式）──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextData {
    pub content: String,
    pub metadata: HashMap<String, String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ContextStore {
    store: HashMap<CtxId, ContextData>,
}

impl ContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, data: ContextData) -> CtxId {
        let ctx_id = Uuid::new_v4().to_string();
        self.store.insert(ctx_id.clone(), data);
        ctx_id
    }

    pub fn get(&self, ctx_id: &str) -> Option<&ContextData> {
        self.store.get(ctx_id)
    }

    pub fn remove(&mut self, ctx_id: &str) -> Option<ContextData> {
        self.store.remove(ctx_id)
    }
}

// ─── 预定义链路步骤 ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ChainStep {
    pub agent_id: AgentId,
    pub pass_full_context: bool,
}

// ─── 链路匹配条件 ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MatchCondition {
    pub keywords: Vec<String>,
    pub domain_tags: Vec<String>,
    pub exact_match: Option<String>,
    pub priority: u32,
}

impl MatchCondition {
    pub fn matches(&self, input: &str, domain_tags: &[String]) -> bool {
        let input_lower = input.to_lowercase();

        if let Some(exact) = &self.exact_match {
            if input_lower == exact.to_lowercase() {
                return true;
            }
        }

        for keyword in &self.keywords {
            if input_lower.contains(&keyword.to_lowercase()) {
                return true;
            }
        }

        for tag in &self.domain_tags {
            if domain_tags.contains(tag) {
                return true;
            }
        }

        false
    }
}

// ─── 链路约束（纯 predicate）───────────────────────────────

pub type InputValidator = Box<dyn Fn(&str) -> bool + Send + Sync>;
pub type OutputValidator = Box<dyn Fn(&str) -> bool + Send + Sync>;

pub struct ChainConstraint {
    pub input_validator: Option<InputValidator>,
    pub output_validator: Option<OutputValidator>,
    pub max_input_length: usize,
    pub max_output_length: usize,
    pub timeout_ms: u64,
}

impl Clone for ChainConstraint {
    fn clone(&self) -> Self {
        Self {
            input_validator: None,
            output_validator: None,
            max_input_length: self.max_input_length,
            max_output_length: self.max_output_length,
            timeout_ms: self.timeout_ms,
        }
    }
}

impl Default for ChainConstraint {
    fn default() -> Self {
        Self {
            input_validator: None,
            output_validator: None,
            max_input_length: 10000,
            max_output_length: 50000,
            timeout_ms: 300000,
        }
    }
}

impl ChainConstraint {
    pub fn validate_input(&self, input: &str) -> Result<()> {
        if input.len() > self.max_input_length {
            return Err(anyhow!(
                "输入长度超过限制: {} > {}",
                input.len(),
                self.max_input_length
            ));
        }

        if let Some(validator) = &self.input_validator {
            if !validator(input) {
                return Err(anyhow!("输入不符合约束条件"));
            }
        }

        Ok(())
    }

    pub fn validate_output(&self, output: &str) -> Result<()> {
        if output.len() > self.max_output_length {
            return Err(anyhow!(
                "输出长度超过限制: {} > {}",
                output.len(),
                self.max_output_length
            ));
        }

        if let Some(validator) = &self.output_validator {
            if !validator(output) {
                return Err(anyhow!("输出不符合约束条件"));
            }
        }

        Ok(())
    }
}

// ─── 预定义链路 ────────────────────────────────────────────

#[derive(Clone)]
pub struct PredefinedChain {
    pub name: ChainName,
    pub description: String,
    pub condition: MatchCondition,
    pub steps: Vec<ChainStep>,
    pub constraint: ChainConstraint,
}

// ─── 链路注册中心 ──────────────────────────────────────────

#[derive(Default)]
pub struct ChainRegistry {
    chains: HashMap<ChainName, PredefinedChain>,
    default_chain: Option<ChainName>,
}

impl ChainRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_chain(&mut self, chain: PredefinedChain) {
        self.chains.insert(chain.name.clone(), chain);
    }

    pub fn set_default_chain(&mut self, name: &str) {
        if self.chains.contains_key(name) {
            self.default_chain = Some(name.to_string());
        }
    }

    pub fn find_matching_chain(
        &self,
        input: &str,
        domain_tags: &[String],
    ) -> Option<&PredefinedChain> {
        let mut matched: Vec<&PredefinedChain> = self
            .chains
            .values()
            .filter(|chain| chain.condition.matches(input, domain_tags))
            .collect();

        matched.sort_by_key(|b| std::cmp::Reverse(b.condition.priority));

        matched.first().copied()
    }

    pub fn get_chain(&self, name: &str) -> Option<&PredefinedChain> {
        self.chains.get(name)
    }

    pub fn get_default_chain(&self) -> Option<&PredefinedChain> {
        self.default_chain
            .as_ref()
            .and_then(|name| self.chains.get(name))
    }

    pub fn list_chains(&self) -> Vec<ChainName> {
        self.chains.keys().cloned().collect()
    }
}

// ─── 专家 Agent 接口 ──────────────────────────────────────

#[async_trait::async_trait]
pub trait ExpertAgent: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn tags(&self) -> &[String];
    fn priority(&self) -> u32 {
        0
    }
    async fn run(&self, input_ctx_id: &str, context_store: &mut ContextStore) -> Result<CtxId>;
}

// ─── 专家注册中心 ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AgentMeta {
    pub id: AgentId,
    pub name: String,
    pub tags: Vec<String>,
    pub priority: u32,
}

pub struct AgentRegistry {
    agents: HashMap<AgentId, Arc<dyn ExpertAgent>>,
    meta: HashMap<AgentId, AgentMeta>,
}

#[allow(clippy::derivable_impls)]
impl Default for AgentRegistry {
    fn default() -> Self {
        Self {
            agents: HashMap::new(),
            meta: HashMap::new(),
        }
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, agent: Arc<dyn ExpertAgent>) {
        let id = agent.id().to_string();
        let meta = AgentMeta {
            id: id.clone(),
            name: agent.name().to_string(),
            tags: agent.tags().to_vec(),
            priority: 10,
        };
        self.agents.insert(id, agent);
        self.meta.insert(meta.id.clone(), meta);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ExpertAgent>> {
        self.agents.get(id).cloned()
    }

    pub fn get_meta(&self, id: &str) -> Option<&AgentMeta> {
        self.meta.get(id)
    }

    pub fn list(&self) -> Vec<&AgentMeta> {
        self.meta.values().collect()
    }

    pub fn find_matching_experts(
        &self,
        input: &str,
        domain_tags: &[String],
    ) -> Vec<Arc<dyn ExpertAgent>> {
        let input_lower = input.to_lowercase();
        let mut matched: Vec<(Arc<dyn ExpertAgent>, u32)> = Vec::new();

        for agent in self.agents.values() {
            let mut score = 0;

            for tag in agent.tags() {
                if input_lower.contains(&tag.to_lowercase()) {
                    score += 1;
                }
            }

            for tag in domain_tags {
                if agent
                    .tags()
                    .iter()
                    .any(|t| t.to_lowercase() == tag.to_lowercase())
                {
                    score += 2;
                }
            }

            if score > 0 {
                matched.push((agent.clone(), score));
            }
        }

        matched.sort_by_key(|b| std::cmp::Reverse(b.1));
        matched.into_iter().map(|(a, _)| a).collect()
    }
}

// ─── 内部通用专家（兜底专家）─────────────────────────────────

/// 内部通用专家，当没有匹配到任何插件专家时使用
///
/// 这是一个纯 LLM 调用的兜底实现，不注入任何领域知识，作为最后的 fallback
#[derive(Debug, Clone)]
pub struct DefaultExpert {
    runtime: Arc<Runtime>,
}

impl DefaultExpert {
    pub fn new(runtime: Arc<Runtime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl ExpertAgent for DefaultExpert {
    fn id(&self) -> &str {
        "default"
    }

    fn name(&self) -> &str {
        "通用专家"
    }

    fn tags(&self) -> &[String] {
        &[]
    }

    async fn run(&self, ctx_id: &str, store: &mut ContextStore) -> Result<CtxId> {
        let span = tracing::info_span!(
            "default_expert_run",
            expert_id = %self.id(),
            expert_name = %self.name(),
        );
        let _enter = span.enter();

        let context = store
            .get(ctx_id)
            .ok_or_else(|| anyhow!("Context not found: {}", ctx_id))?;

        tracing::debug!(
            "DefaultExpert: 处理请求, content_len={}",
            context.content.len()
        );

        let messages = vec![
            Message {
                role: Role::System,
                content: "你是一个友好的 AI 助手，可以回答各种问题。请直接给出简洁、准确的回答。"
                    .to_string(),
                tool_call_id: None,
            },
            Message {
                role: Role::User,
                content: context.content.clone(),
                tool_call_id: None,
            },
        ];

        let response = self.runtime.call_llm(messages).await?;

        tracing::debug!(
            "DefaultExpert: LLM 调用完成, response_len={}",
            response.len()
        );

        Ok(store.put(ContextData {
            content: response,
            metadata: HashMap::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }))
    }
}

// ─── 领域关键词提取（纯规则，无 AI）─────────────────────────

pub struct DomainExtractor;

impl DomainExtractor {
    const DOMAIN_KEYWORDS: &'static [(&'static str, &'static str)] = &[
        ("编程", "tech"),
        ("代码", "tech"),
        ("开发", "tech"),
        ("bug", "tech"),
        ("rust", "tech"),
        ("python", "tech"),
        ("java", "tech"),
        ("javascript", "tech"),
        ("天气", "weather"),
        ("温度", "weather"),
        ("下雨", "weather"),
        ("气象", "weather"),
        ("心情", "psychology"),
        ("心理", "psychology"),
        ("咨询", "psychology"),
        ("情绪", "psychology"),
        ("情感", "psychology"),
        ("健康", "health"),
        ("疾病", "health"),
        ("医生", "health"),
        ("健身", "health"),
        ("学习", "education"),
        ("课程", "education"),
        ("教学", "education"),
        ("考试", "education"),
        ("写文章", "writing"),
        ("写报告", "writing"),
        ("总结", "writing"),
        ("翻译", "writing"),
        ("设计", "design"),
        ("画图", "design"),
        ("UI", "design"),
        ("界面", "design"),
        ("数据分析", "data"),
        ("统计", "data"),
        ("可视化", "data"),
        ("报表", "data"),
    ];

    pub fn extract_domain_tags(input: &str) -> Vec<String> {
        let input_lower = input.to_lowercase();
        let mut tags = Vec::new();
        for (keyword, tag) in Self::DOMAIN_KEYWORDS {
            if input_lower.contains(keyword) {
                tags.push(tag.to_string());
            }
        }
        tags
    }
}

// ─── 调度结果 ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub output: String,
    pub agent_chain: Vec<AgentId>,
    pub tokens: TokenStats,
    pub duration_ms: u64,
    pub chain_name: ChainName,
}

#[derive(Debug, Clone)]
pub struct OrchestrationResult {
    pub success: bool,
    pub output: String,
    pub session_id: String,
    pub trace_id: String,
    pub strategy: String,
    pub expert_chain: Vec<String>,
    pub expert_outputs: HashMap<String, String>,
    pub duration_ms: u64,
    pub critique_rounds: usize,
    pub critique_records: Vec<serde_json::Value>,
    pub tokens: TokenStats,
}

// ─── Orchestrator 调度器 ──────────────────────────────────

pub struct Orchestrator {
    agent_registry: AgentRegistry,
    chain_registry: ChainRegistry,
    context_store: ContextStore,
    global_constraint: ChainConstraint,
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("agent_count", &self.agent_registry.list().len())
            .finish()
    }
}

impl Orchestrator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self {
            agent_registry: AgentRegistry::new(),
            chain_registry: ChainRegistry::new(),
            context_store: ContextStore::new(),
            global_constraint: ChainConstraint {
                max_input_length: 10000,
                max_output_length: 50000,
                timeout_ms: 300000,
                input_validator: Some(Box::new(|input| {
                    let blacklist = ["暴力", "攻击", "色情"];
                    !blacklist.iter().any(|kw| input.contains(kw))
                })),
                output_validator: None,
            },
        }
    }
}

impl Orchestrator {
    pub fn register_agent(&mut self, agent: Arc<dyn ExpertAgent>) {
        self.agent_registry.register(agent);
    }

    pub fn list_agents(&self) -> Vec<&AgentMeta> {
        self.agent_registry.list()
    }

    pub fn register_chain(&mut self, chain: PredefinedChain) {
        self.chain_registry.register_chain(chain);
    }

    pub fn set_default_chain(&mut self, name: &str) {
        self.chain_registry.set_default_chain(name);
    }

    pub fn put_context(&mut self, content: String) -> CtxId {
        self.context_store.put(ContextData {
            content,
            metadata: HashMap::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        })
    }

    pub async fn execute_chain(
        &mut self,
        input: &str,
        chain_name: &str,
        trace: &mut Trace,
    ) -> Result<ExecutionResult> {
        let start = Instant::now();

        let chain = self
            .chain_registry
            .get_chain(chain_name)
            .cloned()
            .ok_or_else(|| anyhow!("链路不存在: {}", chain_name))?;

        let span = tracing::info_span!(
            "execute_chain",
            trace_id = %trace.id.as_str(),
            chain_name = %chain_name,
            chain_description = %chain.description,
            steps_count = chain.steps.len(),
        );
        let _enter = span.enter();

        tracing::debug!("开始执行链路: {}", chain_name);

        chain.constraint.validate_input(input)?;
        self.global_constraint.validate_input(input)?;

        let material_ctx_id = self.put_context(input.to_string());
        let mut current_ctx_id: Option<String> = None;
        let mut agent_chain = Vec::new();
        let mut outputs = Vec::new();

        for (step_idx, step) in chain.steps.iter().enumerate() {
            let agent = self
                .agent_registry
                .get(&step.agent_id)
                .ok_or_else(|| anyhow!("专家不存在: {}", step.agent_id))?;

            let input_ctx_id = if step.pass_full_context {
                &material_ctx_id
            } else {
                current_ctx_id.as_deref().unwrap_or(&material_ctx_id)
            };

            let expert_span = tracing::debug_span!(
                "expert_run",
                trace_id = %trace.id.as_str(),
                expert_id = %agent.id(),
                expert_name = %agent.name(),
                step = step_idx + 1,
                pass_full_context = step.pass_full_context,
            );
            let _expert_enter = expert_span.enter();

            let step_start = Instant::now();
            let ctx_id = agent.run(input_ctx_id, &mut self.context_store).await?;
            let step_duration = step_start.elapsed().as_millis() as u64;

            tracing::info!("专家 [{}] 执行完成, 耗时: {}ms", agent.id(), step_duration);

            let output = self
                .context_store
                .get(&ctx_id)
                .map(|d| d.content.clone())
                .unwrap_or_default();
            outputs.push(output.clone());
            current_ctx_id = Some(ctx_id);
            agent_chain.push(step.agent_id.clone());
        }

        let final_output = outputs.last().cloned().unwrap_or_default();
        chain.constraint.validate_output(&final_output)?;

        let duration_ms = start.elapsed().as_millis() as u64;

        tracing::info!("链路执行完成, 总耗时: {}ms", duration_ms);

        trace.expert_chain = agent_chain.clone();

        Ok(ExecutionResult {
            output: final_output,
            agent_chain,
            tokens: TokenStats::default(),
            duration_ms,
            chain_name: chain_name.to_string(),
        })
    }

    pub async fn dispatch(&mut self, input: &str, trace: &mut Trace) -> Result<ExecutionResult> {
        let domain_tags = DomainExtractor::extract_domain_tags(input);

        let span = tracing::info_span!(
            "dispatch",
            trace_id = %trace.id.as_str(),
            input_len = input.len(),
            domain_tags_len = domain_tags.len(),
        );
        let _enter = span.enter();

        tracing::debug!("提取到领域标签: {:?}", domain_tags);

        let matched_experts = self
            .agent_registry
            .find_matching_experts(input, &domain_tags);

        tracing::debug!("匹配到 {} 个专家", matched_experts.len());

        if matched_experts.len() == 1 {
            let agent = matched_experts[0].clone();

            tracing::info!("模式: direct, 匹配专家: {} ({})", agent.id(), agent.name());

            return self.execute_direct(agent, input, trace).await;
        }

        if matched_experts.is_empty() {
            let default_agent = self
                .agent_registry
                .get("default")
                .ok_or_else(|| anyhow!("内部通用专家未注册"))?;

            tracing::warn!(
                "模式: fallback, 未匹配到任何专家，使用内部通用专家: {} ({})",
                default_agent.id(),
                default_agent.name()
            );

            return self.execute_direct(default_agent, input, trace).await;
        }

        let chain_name = {
            let chain = self
                .chain_registry
                .find_matching_chain(input, &domain_tags)
                .or_else(|| self.chain_registry.get_default_chain())
                .ok_or_else(|| anyhow!("未找到匹配的链路"))?;

            tracing::info!(
                "模式: chain, 匹配链路: {} ({})",
                chain.name,
                chain.description
            );

            chain.name.clone()
        };

        trace.chain_name = Some(chain_name.clone());
        self.execute_chain(input, &chain_name, trace).await
    }

    async fn execute_direct(
        &mut self,
        agent: Arc<dyn ExpertAgent>,
        input: &str,
        trace: &mut Trace,
    ) -> Result<ExecutionResult> {
        let start = Instant::now();

        let span = tracing::info_span!(
            "execute_direct",
            trace_id = %trace.id.as_str(),
            expert_id = %agent.id(),
            expert_name = %agent.name(),
        );
        let _enter = span.enter();

        tracing::debug!("开始直接执行专家");

        self.global_constraint.validate_input(input)?;

        let material_ctx_id = self.put_context(input.to_string());

        let expert_span = tracing::debug_span!(
            "expert_run",
            trace_id = %trace.id.as_str(),
            expert_id = %agent.id(),
            step = 1,
        );
        let _expert_enter = expert_span.enter();

        let step_start = Instant::now();
        let ctx_id = agent.run(&material_ctx_id, &mut self.context_store).await?;
        let step_duration = step_start.elapsed().as_millis() as u64;

        tracing::info!("专家执行完成, 耗时: {}ms", step_duration);

        let output = self
            .context_store
            .get(&ctx_id)
            .map(|d| d.content.clone())
            .unwrap_or_default();

        self.global_constraint.validate_output(&output)?;

        let duration_ms = start.elapsed().as_millis() as u64;

        trace.expert_chain = vec![agent.id().to_string()];

        Ok(ExecutionResult {
            output,
            agent_chain: vec![agent.id().to_string()],
            tokens: TokenStats::default(),
            duration_ms,
            chain_name: "direct".to_string(),
        })
    }

    pub async fn run_orchestrated_with_trace(
        &mut self,
        input: &str,
        _user_id: &str,
        trace: &mut Trace,
    ) -> Result<OrchestrationResult> {
        let start = Instant::now();

        let result = self.dispatch(input, trace).await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(OrchestrationResult {
            success: true,
            output: result.output,
            session_id: "".to_string(),
            trace_id: trace.id.as_str().to_string(),
            strategy: result.chain_name,
            expert_chain: result.agent_chain,
            expert_outputs: HashMap::new(),
            duration_ms,
            critique_rounds: 0,
            critique_records: Vec::new(),
            tokens: result.tokens,
        })
    }

    pub fn list_chains(&self) -> Vec<ChainName> {
        self.chain_registry.list_chains()
    }

    pub fn get_chain(&self, name: &str) -> Option<&PredefinedChain> {
        self.chain_registry.get_chain(name)
    }
}

// ─── AI 扩展生成器 ──────────────────────────────────────────

pub struct ChainCodeGenerator;

impl ChainCodeGenerator {
    pub fn generate_new_chain_code(
        chain_name: &str,
        description: &str,
        keywords: &[&str],
        domain_tags: &[&str],
        steps: &[(&str, bool)],
        max_input_length: usize,
        max_output_length: usize,
    ) -> String {
        let keywords_str = keywords
            .iter()
            .map(|k| format!("\"{}\"", k))
            .collect::<Vec<_>>()
            .join(", ");

        let domain_tags_str = domain_tags
            .iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>()
            .join(", ");

        let steps_str = steps
            .iter()
            .map(|(agent_id, pass_full)| {
                format!(
                    "        ChainStep {{\n            agent_id: \"{}\".to_string(),\n            pass_full_context: {},\n        }},",
                    agent_id, pass_full
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"// ─── {} ──────────────────────────────────────────

// {}
pub fn register_{}_chain(orchestrator: &mut Orchestrator) {{
    orchestrator.register_chain(PredefinedChain {{
        name: "{}".to_string(),
        description: "{}".to_string(),
        condition: MatchCondition {{
            keywords: vec![{}],
            domain_tags: vec![{}],
            exact_match: None,
            priority: 10,
        }},
        steps: vec![
{}
        ],
        constraint: ChainConstraint {{
            input_validator: None,
            output_validator: None,
            max_input_length: {},
            max_output_length: {},
            timeout_ms: 300000,
        }},
    }});
}}
"#,
            chain_name,
            description,
            chain_name.replace("-", "_").replace(" ", "_"),
            chain_name,
            description,
            keywords_str,
            domain_tags_str,
            steps_str,
            max_input_length,
            max_output_length,
        )
    }
}
