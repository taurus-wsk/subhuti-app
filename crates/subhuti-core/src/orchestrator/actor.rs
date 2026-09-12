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
use std::sync::{Arc, RwLock};

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
///
/// 内部使用 `Arc<RwLock<..>>`：注册走写锁、读取走读锁，
/// 使注册不再需要 `&mut self`，从而让 Orchestrator 可以被并发共享
/// （避免整个编排过程被一把大锁串行化）。
#[derive(Clone, Default)]
pub struct ActorRegistry {
    actors: Arc<RwLock<Vec<Arc<dyn Actor>>>>,
}

impl ActorRegistry {
    pub fn new() -> Self {
        Self {
            actors: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 注册 Actor（内部写锁，无需 `&mut self`）
    pub fn register(&self, actor: Arc<dyn Actor>) {
        match self.actors.write() {
            Ok(mut actors) => actors.push(actor),
            Err(e) => tracing::error!("ActorRegistry 写锁中毒，Actor 注册失败: {}", e),
        }
    }

    /// 获取所有 Actor（快照，避免长期持有读锁）
    pub fn list(&self) -> Vec<Arc<dyn Actor>> {
        self.actors.read().map(|a| a.clone()).unwrap_or_default()
    }

    /// 获取 Actor 数量
    pub fn count(&self) -> usize {
        self.actors.read().map(|a| a.len()).unwrap_or(0)
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
        let actors = self.list();
        for actor in actors.iter() {
            let score = actor.score(task_tags).await;
            if score >= min_score {
                candidates.push((actor.clone(), score));
            }
        }
        candidates.sort_by_key(|c| std::cmp::Reverse(c.1));
        candidates.truncate(max);
        candidates
    }

    /// 竞标（带领域上下文）：同时看节点流程标签与用户输入的领域
    ///
    /// `find_top_candidates` 只能回答「谁能干这个流程步骤」——节点标签是图定义时
    /// 写死的（如 `["analysis","rust"]`），与用户实际问什么无关，因此只带领域标签的
    /// 专家（如 Blender）在 Rust 图里永远 0 分、结构性不可见。
    ///
    /// 这里把用户输入作为**领域上下文**参与加权：
    ///
    /// - 流程分：节点标签匹配度 × `FLOW_WEIGHT`，表示能否胜任该步骤
    /// - 领域分：输入命中专家标签的程度 × `DOMAIN_WEIGHT`，表示是否懂该领域
    ///
    /// 领域权重更高，于是「Blender 问题误入 rust 图」时 Blender 专家仍能胜出。
    pub async fn find_top_candidates_with_input(
        &self,
        task_tags: &[String],
        input: &str,
        max: usize,
    ) -> Vec<(Arc<dyn Actor>, u32)> {
        const FLOW_WEIGHT: f32 = 0.4;
        const DOMAIN_WEIGHT: f32 = 0.6;

        let mut candidates: Vec<(Arc<dyn Actor>, u32)> = Vec::new();
        let actors = self.list();
        let has_input = !input.trim().is_empty();
        let domain_tags: Vec<String> = if has_input {
            vec![input.to_string()]
        } else {
            Vec::new()
        };

        for actor in actors.iter() {
            let flow = actor.score(task_tags).await;
            let domain = if has_input {
                actor.score(&domain_tags).await
            } else {
                0
            };
            let total = (flow as f32 * FLOW_WEIGHT + domain as f32 * DOMAIN_WEIGHT).round() as u32;
            if total > 0 {
                candidates.push((actor.clone(), total));
            }
        }

        candidates.sort_by_key(|c| std::cmp::Reverse(c.1));
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
        let actors = self.list();
        for actor in actors.iter() {
            let score = actor.score(task_tags).await;
            if score > 0 {
                scores.push(ActorScore {
                    actor_id: actor.id().to_string(),
                    score,
                    reason: None,
                });
            }
        }
        scores.sort_by_key(|s| std::cmp::Reverse(s.score));
        scores
    }

    /// 通过 ID 查找 Actor
    pub fn get_by_id(&self, id: &str) -> Option<Arc<dyn Actor>> {
        self.actors
            .read()
            .ok()
            .and_then(|actors| actors.iter().find(|a| a.id() == id).cloned())
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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockActor {
        id: String,
        tags: Vec<String>,
    }

    #[async_trait]
    impl Actor for MockActor {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> &str {
            &self.id
        }
        fn tags(&self) -> &[String] {
            &self.tags
        }
        async fn perform(
            &self,
            _ctx: &mut AgentContext,
            _state: &ExpertState,
        ) -> crate::Result<String> {
            Ok(self.id.clone())
        }
    }

    fn mock(id: &str, tags: &[&str]) -> Arc<dyn Actor> {
        Arc::new(MockActor {
            id: id.to_string(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        })
    }

    /// 注册不再需要 `&mut self`（回归防护：这里用的是不可变绑定）
    #[test]
    fn register_without_exclusive_access() {
        let registry = ActorRegistry::new();
        registry.register(mock("a", &["rust"]));
        registry.register(mock("b", &["blender"]));

        assert_eq!(registry.count(), 2);
        assert_eq!(registry.list().len(), 2);
        assert_eq!(registry.get_by_id("a").unwrap().id(), "a");
        assert!(registry.get_by_id("missing").is_none());
    }

    /// 并发读（竞标）与并发写（注册）同时进行，验证注册表不再阻塞并发
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_read_and_register() {
        let registry = Arc::new(ActorRegistry::new());
        registry.register(mock("initial", &["rust"]));

        let mut handles = Vec::new();

        // 8 个并发读者：反复执行竞标
        for _ in 0..8 {
            let r = registry.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..50 {
                    let candidates = r.find_top_candidates(&["rust".to_string()], 3).await;
                    assert!(!candidates.is_empty(), "竞标应至少命中初始 Actor");
                }
            }));
        }

        // 同时 8 个并发写者：注册新 Actor
        for i in 0..8 {
            let r = registry.clone();
            handles.push(tokio::spawn(async move {
                r.register(mock(&format!("a{}", i), &["rust"]));
            }));
        }

        for h in handles {
            h.await.expect("并发任务不应 panic");
        }

        assert_eq!(registry.count(), 9, "1 个初始 + 8 个并发注册");
    }

    fn tags_of(s: &[&str]) -> Vec<String> {
        s.iter().map(|t| t.to_string()).collect()
    }

    /// 回归：Blender 问题误入 rust 图时，Blender 专家应靠领域上下文胜出
    ///
    /// 节点标签是图定义写死的 `["analysis","rust"]`，与用户输入无关。
    /// 若只看流程标签，只带领域标签的 Blender 专家永远 0 分、结构性不可见。
    #[tokio::test]
    async fn domain_context_beats_hardcoded_flow_tags() {
        let registry = ActorRegistry::new();
        registry.register(mock(
            "rust",
            &["rust", "code", "analysis", "planning", "file_io"],
        ));
        registry.register(mock("blender", &["blender", "3d", "建模", "渲染"]));

        let flow_tags = tags_of(&["analysis", "rust"]);
        let input = "教我怎么用Blender做阵列修改器循环建模";

        let best = registry
            .find_top_candidates_with_input(&flow_tags, input, 1)
            .await
            .into_iter()
            .next()
            .expect("应有候选");

        assert_eq!(
            best.0.id(),
            "blender",
            "Blender 问题即使节点标签是 rust 相关，也应由 Blender 专家中标，实际: {}",
            best.0.id()
        );
    }

    /// 回归：真实 Rust 代码任务仍应由 Rust 专家中标（不能改坏正常路径）
    #[tokio::test]
    async fn rust_task_still_selects_rust_expert() {
        let registry = ActorRegistry::new();
        registry.register(mock(
            "rust",
            &["rust", "code", "analysis", "planning", "file_io"],
        ));
        registry.register(mock("blender", &["blender", "3d", "建模", "渲染"]));

        let flow_tags = tags_of(&["analysis", "rust"]);
        let input = "帮我重构 src/main.rs 里的错误处理逻辑";

        let best = registry
            .find_top_candidates_with_input(&flow_tags, input, 1)
            .await
            .into_iter()
            .next()
            .expect("应有候选");

        assert_eq!(best.0.id(), "rust", "Rust 任务应由 Rust 专家中标");
    }

    /// 无输入时退化为纯流程标签竞标（保持原有语义）
    #[tokio::test]
    async fn empty_input_falls_back_to_flow_tags() {
        let registry = ActorRegistry::new();
        registry.register(mock("rust", &["rust", "analysis"]));
        registry.register(mock("blender", &["blender", "建模"]));

        let flow_tags = tags_of(&["analysis", "rust"]);
        let best = registry
            .find_top_candidates_with_input(&flow_tags, "", 1)
            .await
            .into_iter()
            .next()
            .expect("应有候选");

        assert_eq!(best.0.id(), "rust", "无输入时按流程标签选 Rust");
    }
}
