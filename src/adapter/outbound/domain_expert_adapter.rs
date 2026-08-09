//! # 领域专家适配器
//!
//! 将领域层定义的 `DomainExpert` 接口转换为 Subhuti 框架的 `ExpertAgent` 接口。
//!
//! 六边形架构：
//! - 领域层定义接口（DomainExpert）
//! - 出站适配层实现适配器（DomainExpertAdapter）
//! - 适配器内部持有 DomainExpert 实例，实现 ExpertAgent 接口
//! - Subhuti 框架通过 ExpertAgent 接口调用专家

use async_trait::async_trait;
use std::sync::Arc;

use subhuti::{
    orchestrator::{AgentContext, ExpertAgent, ExpertState, FromState, Llm, SkillInfo},
    Result,
};

use crate::domain::traits::{
    DomainContext, DomainError, DomainExecutionContext, DomainExpert, DomainLlm, DomainMessage,
    DomainRepository, DomainResult, DomainRole,
};

/// 领域专家适配器
///
/// 将领域层的 DomainExpert 转换为 Subhuti 框架的 ExpertAgent。
pub struct DomainExpertAdapter<D: ?Sized>
where
    D: DomainExpert,
{
    domain_expert: Arc<D>,
    /// 缓存的技能信息（在创建时预计算，避免重复转换）
    skills: Vec<SkillInfo>,
    /// 数据仓库（通过依赖注入传入，供领域专家使用）
    repository: Arc<dyn DomainRepository>,
}

impl<D: ?Sized> DomainExpertAdapter<D>
where
    D: DomainExpert,
{
    /// 创建新的适配器实例
    pub fn new(domain_expert: Arc<D>, repository: Arc<dyn DomainRepository>) -> Self {
        // 预计算并缓存技能信息，避免每次调用 skills() 时重复转换
        let skills = domain_expert
            .skills()
            .iter()
            .map(|s| SkillInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.parameters.clone(),
            })
            .collect();

        Self {
            domain_expert,
            skills,
            repository,
        }
    }

    /// 获取底层领域专家实例
    pub fn inner(&self) -> &Arc<D> {
        &self.domain_expert
    }

    /// 获取数据仓库实例
    pub fn repository(&self) -> &Arc<dyn DomainRepository> {
        &self.repository
    }
}

#[async_trait]
impl<D: ?Sized> ExpertAgent for DomainExpertAdapter<D>
where
    D: DomainExpert,
{
    fn id(&self) -> &str {
        self.domain_expert.id()
    }

    fn name(&self) -> &str {
        self.domain_expert.name()
    }

    fn tags(&self) -> &[String] {
        self.domain_expert.tags()
    }

    fn skills(&self) -> &[SkillInfo] {
        // 返回预计算并缓存的技能信息
        &self.skills
    }

    async fn run(&self, ctx: &mut AgentContext, state: &ExpertState) -> Result<String> {
        // 1. 从框架状态中提取 LLM
        let Llm(llm) = Llm::from_state(state)?;

        // 2. 创建领域 LLM 适配器
        let domain_llm = Arc::new(SubhutiLlmAdapter { llm: llm.clone() });

        // 3. 构建领域上下文
        let domain_ctx = DomainContext {
            input: ctx.input.clone(),
            session_id: Some(ctx.session.id().to_string()),
            user_id: None,
        };

        // 4. 检查是否指定了技能执行
        // 从上下文中提取 skill_id 和 params（通过 metadata 传递）
        let skill_id = ctx
            .metadata
            .get("skill_id")
            .unwrap_or(&"".to_string())
            .clone();
        let params = ctx
            .metadata
            .get("skill_params")
            .unwrap_or(&"".to_string())
            .clone();

        // 5. 构建领域执行上下文（封装所有依赖，避免参数膨胀）
        let exec_ctx = DomainExecutionContext {
            ctx: domain_ctx,
            llm: domain_llm,
            repository: self.repository.clone(),
            skill_id: if skill_id.is_empty() {
                None
            } else {
                Some(skill_id.clone())
            },
            skill_params: if params.is_empty() {
                None
            } else {
                Some(params.clone())
            },
        };

        // 6. 根据是否有技能ID选择执行方式
        let result = if !skill_id.is_empty() {
            // 执行指定技能
            tracing::debug!("执行技能: {}，参数: {}", skill_id, params);
            self.domain_expert
                .execute_skill(&skill_id, &params, exec_ctx)
                .await
        } else {
            // 执行通用逻辑
            self.domain_expert.run(exec_ctx).await
        };

        // 7. 转换结果（在适配器中手动转换领域错误为框架错误）
        match result {
            Ok(output) => Ok(output),
            Err(e) => Err(subhuti::Error::Any(anyhow::anyhow!(e))),
        }
    }
}

/// Subhuti LLM 适配器
///
/// 将 Subhuti 框架的 LLM 转换为领域层的 DomainLlm 接口。
struct SubhutiLlmAdapter {
    llm: Arc<dyn subhuti::runtime::llm::LLM>,
}

#[async_trait]
impl DomainLlm for SubhutiLlmAdapter {
    async fn chat(&self, messages: Vec<DomainMessage>) -> DomainResult<String> {
        // 将领域消息转换为框架消息
        let framework_messages: Vec<subhuti::runtime::llm::Message> = messages
            .into_iter()
            .map(|m| {
                let role = match m.role {
                    DomainRole::System => subhuti::runtime::llm::Role::System,
                    DomainRole::User => subhuti::runtime::llm::Role::User,
                    DomainRole::Assistant => subhuti::runtime::llm::Role::Assistant,
                };
                subhuti::runtime::llm::Message {
                    role,
                    content: m.content,
                    tool_call_id: None,
                }
            })
            .collect();

        // 调用框架 LLM
        let response = self.llm.chat(framework_messages).await;

        // 转换结果
        match response {
            Ok(output) => Ok(output),
            Err(e) => Err(DomainError::LlmError(e.to_string())),
        }
    }

    fn model_name(&self) -> &str {
        self.llm.config().model.as_str()
    }
}
