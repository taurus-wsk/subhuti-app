//! # Actor 竞标系统
//!
//! 舞台隐喻的核心：专家注册后成为 Actor，通过竞标机制竞争图节点任务。
//!
//! ## 设计哲学
//!
//! - **Actor = 演员**：每个专家注册后自动成为 Actor，拥有独立评分能力
//! - **竞标制**：图节点发布任务要求，所有 Actor 自评分数，最高分上台
//! - **双向奔赴**：节点定义"需要什么"，Actor 自评"我能做什么"，匹配成功才执行
//!
//! ## 竞标流程
//!
//! ```text
//! 图节点需要执行
//!   → 调度器发布 ActorTaskRequested（含任务标签/描述/状态）
//!   → 所有 Actor 收到 → 各自评分（使用 RuleEngine 或自定义逻辑）
//!   → 调度器收集竞标 → 选最高分
//!   → 调度器发布 NodeTaskAssigned（含中标 Actor + 状态）
//!   → 中标 Actor 执行 → 发布 NodeCompleted/NodeFailed
//! ```

use async_trait::async_trait;
use std::sync::Arc;

use crate::orchestrator::{AgentContext, ExpertAgent, ExpertState, SkillInfo};

/// Actor 评分结果
#[derive(Debug, Clone)]
pub struct ActorScore {
    pub actor_id: String,
    pub score: u32,
    pub reason: Option<String>,
}

/// Actor trait - 舞台上可执行任务的演员
///
/// 每个 Actor 代表一个已注册的专家，具备：
/// - 自我认知：id, name, tags, skills
/// - 评分能力：score() 根据任务要求自评匹配度
/// - 执行能力：perform() 上台执行任务
#[async_trait]
pub trait Actor: Send + Sync {
    /// 演员唯一标识
    fn id(&self) -> &str;
    /// 演员名称
    fn name(&self) -> &str;
    /// 标签（领域能力标识）
    fn tags(&self) -> &[String];
    /// 技能列表
    fn skills(&self) -> &[SkillInfo] {
        &[]
    }

    /// 自评：对给定任务标签的匹配分数
    ///
    /// 返回 0-100 的分数，0 表示完全不匹配，100 表示完美匹配。
    /// 默认实现：按标签匹配数 × 20 分（满分 100）。
    async fn score(&self, task_tags: &[String]) -> u32 {
        if task_tags.is_empty() {
            return 50; // 无要求时给中性分
        }
        let my_tags: Vec<String> = self.tags().iter().map(|t| t.to_lowercase()).collect();
        let task_tags_lower: Vec<String> = task_tags.iter().map(|t| t.to_lowercase()).collect();

        // 子串匹配：检查 actor 的标签是否出现在 task tag 中（消息内容作为标签时有用）
        let matched = task_tags_lower
            .iter()
            .filter(|tt| my_tags.iter().any(|mt| tt.contains(mt)))
            .count();

        if matched == 0 {
            0
        } else {
            (matched as u32 * 100 / task_tags.len() as u32).min(100)
        }
    }

    /// 上台执行任务
    async fn perform(&self, ctx: &mut AgentContext, state: &ExpertState) -> crate::Result<String>;
}

/// Actor 注册表（全局演员池）
///
/// 管理所有已注册的 Actor，提供竞标能力。
#[derive(Clone, Default)]
pub struct ActorRegistry {
    actors: Vec<Arc<dyn Actor>>,
}

impl ActorRegistry {
    pub fn new() -> Self {
        Self { actors: Vec::new() }
    }

    /// 注册 Actor
    pub fn register(&mut self, actor: Arc<dyn Actor>) {
        self.actors.push(actor);
    }

    /// 获取所有 Actor
    pub fn list(&self) -> &[Arc<dyn Actor>] {
        &self.actors
    }

    /// 获取 Actor 数量
    pub fn count(&self) -> usize {
        self.actors.len()
    }

    /// 竞标：取前 N 名候补演员（分数 >= 60 才入候选池）
    ///
    /// 返回按分数降序排列，最多 N 个 Actor。不够 N 个也行。
    /// 一个节点有多个候补时，如果第一个执行失败，自动按顺序重试下一个。
    /// 无任务标签时（空标签），任何 Actor 只要分数 > 0 即可入选。
    pub async fn find_top_candidates(
        &self,
        task_tags: &[String],
        max: usize,
    ) -> Vec<(Arc<dyn Actor>, u32)> {
        let mut candidates: Vec<(Arc<dyn Actor>, u32)> = Vec::new();
        let min_score = if task_tags.is_empty() { 1 } else { 60 };
        for actor in &self.actors {
            let score = actor.score(task_tags).await;
            if score >= min_score {
                candidates.push((actor.clone(), score));
            }
        }
        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates.truncate(max);
        candidates
    }

    /// 竞标：找到最匹配的 Actor（向下兼容）
    pub async fn find_best(&self, task_tags: &[String]) -> Option<Arc<dyn Actor>> {
        self.find_top_candidates(task_tags, 1)
            .await
            .into_iter()
            .next()
            .map(|(a, _)| a)
    }

    /// 竞标：返回所有 Actor 的评分列表
    pub async fn collect_scores(&self, task_tags: &[String]) -> Vec<ActorScore> {
        let mut scores = Vec::new();
        for actor in &self.actors {
            let score = actor.score(task_tags).await;
            if score > 0 {
                scores.push(ActorScore {
                    actor_id: actor.id().to_string(),
                    score,
                    reason: None,
                });
            }
        }
        scores.sort_by(|a, b| b.score.cmp(&a.score));
        scores
    }

    /// 通过 ID 查找 Actor
    pub fn get_by_id(&self, id: &str) -> Option<Arc<dyn Actor>> {
        self.actors.iter().find(|a| a.id() == id).cloned()
    }
}

/// 将 ExpertAgent 适配为 Actor
///
/// 专家注册时自动创建此适配器，使专家能以 Actor 身份参与竞标。
pub struct ExpertAgentActorAdapter {
    agent: Arc<dyn ExpertAgent>,
    tags: Vec<String>,
    skills: Vec<SkillInfo>,
}

impl ExpertAgentActorAdapter {
    pub fn new(agent: Arc<dyn ExpertAgent>) -> Self {
        let tags = agent.tags().to_vec();
        let skills = agent.skills().to_vec();
        Self {
            agent,
            tags,
            skills,
        }
    }
}

#[async_trait]
impl Actor for ExpertAgentActorAdapter {
    fn id(&self) -> &str {
        self.agent.id()
    }

    fn name(&self) -> &str {
        self.agent.name()
    }

    fn tags(&self) -> &[String] {
        &self.tags
    }

    fn skills(&self) -> &[SkillInfo] {
        &self.skills
    }

    async fn perform(&self, ctx: &mut AgentContext, state: &ExpertState) -> crate::Result<String> {
        self.agent.run(ctx, state).await
    }
}
