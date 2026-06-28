//! # 心理咨询专家插件
//!
//! 一个示例专家插件，演示如何基于 Subhuti 框架构建领域专家。
//!
//! 包含：
//! - 心理咨询师角色定义
//! - 心理疏导技能
//! - 心理学知识库
//! - 完整的 Manifest 声明、权限、沙箱配置、钩子注册

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use subhuti::{
    expert::{
        Author, HookContext, HookPoint, HookResult, PluginCategory, PluginManifest,
        PluginPermissions, SandboxConfig,
    },
    skill::{FlowTemplate, Skill, SkillContext},
    soul::{BigFive, EmotionalTendency, ToneStyle},
    ExpertPersona, ExpertPlugin, KnowledgeEntry,
};

/// 心理咨询专家
pub struct PsychologyExpert;

impl PsychologyExpert {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PsychologyExpert {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpertPlugin for PsychologyExpert {
    /// 获取插件清单
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "psychology".into(),
            name: "心理咨询专家".into(),
            description: "专业的心理咨询专家，擅长情绪疏导、压力管理、人际关系和自我成长".into(),
            version: "0.1.0".into(),
            author: Some(Author {
                name: "Subhuti Team".into(),
                email: None,
                url: None,
            }),
            category: PluginCategory::Psychology,
            keywords: vec![
                "心理".into(),
                "咨询".into(),
                "情绪".into(),
                "心情".into(),
                "压力".into(),
                "焦虑".into(),
                "抑郁".into(),
                "心理疏导".into(),
                "心理咨询".into(),
            ],
            // 不需要任何特殊权限
            permissions: PluginPermissions::default(),
            // 启用沙箱，限制资源使用
            sandbox: SandboxConfig {
                enabled: true,
                memory_limit_mb: 256,
                max_execution_time_secs: 30,
                max_tokens_per_request: 2048,
                isolate_plugins: true,
                daily_request_limit: Some(500),
                used_requests_today: 0,
            },
            // 注册钩子：在响应前检查是否需要心理危机干预
            hooks: vec![HookPoint::BeforeResponse],
            dependencies: vec![],
            min_framework_version: Some("0.1.0".into()),
            homepage: None,
            license: Some("MIT".into()),
        }
    }

    fn persona(&self) -> ExpertPersona {
        ExpertPersona {
            name: "暖心心理咨询师".into(),
            description: "一位温暖、专业的心理咨询师，善于倾听和共情，\
                帮助用户探索内心世界，提供专业的心理支持和成长建议。"
                .into(),
            tone: ToneStyle::Friendly,
            emotional_tendency: EmotionalTendency::Neutral,
            big_five: BigFive {
                openness: 0.85,
                conscientiousness: 0.75,
                extraversion: 0.4,
                agreeableness: 0.9,
                neuroticism: 0.3,
            },
            traits: vec![
                "温暖".into(),
                "共情".into(),
                "耐心".into(),
                "专业".into(),
                "包容".into(),
                "善于倾听".into(),
            ],
            expertise_areas: {
                let mut map = HashMap::new();
                map.insert("情绪管理".into(), 0.95);
                map.insert("压力应对".into(), 0.9);
                map.insert("人际关系".into(), 0.85);
                map.insert("自我成长".into(), 0.9);
                map.insert("焦虑缓解".into(), 0.88);
                map
            },
            system_prompt: "你是一位温暖、专业的心理咨询师。你的名字是'暖心'。\
                你擅长倾听、共情和理解来访者的感受。\
                你会用温和、支持性的语气与来访者交流。\
                你不会轻易给出诊断，而是引导来访者自我探索。\
                你尊重每一位来访者的感受，保持无条件的积极关注。\
                当遇到严重心理问题时，你会建议寻求专业医疗机构的帮助。"
                .into(),
        }
    }

    fn skills(&self) -> Vec<Box<dyn Skill>> {
        vec![Box::new(MoodCheckSkill), Box::new(StressReliefSkill)]
    }

    fn knowledge(&self) -> Vec<KnowledgeEntry> {
        vec![
            KnowledgeEntry {
                content: "情绪ABC理论：情绪不是由事件本身引起的，而是由我们对事件的认知和信念引起的。\
                    A（Activating event）是诱发性事件，B（Belief）是我们的信念，C（Consequence）是情绪结果。\
                    改变不合理的信念可以改变情绪体验。".into(),
                metadata: None,
            },
            KnowledgeEntry {
                content: "正念呼吸法：找一个安静的地方，舒适地坐下。\
                    将注意力集中在呼吸上，感受气息进出身体的感觉。\
                    当思绪飘走时，温和地将注意力带回呼吸。\
                    每天练习5-10分钟，可以有效缓解焦虑和压力。".into(),
                metadata: None,
            },
            KnowledgeEntry {
                content: "情绪释放的健康方式：1)运动释放-跑步、游泳等有氧运动；\
                    2)表达性书写-把感受写下来；3)艺术表达-绘画、音乐；\
                    4)社会支持-和信任的人倾诉；5)专业帮助-心理咨询。".into(),
                metadata: None,
            },
        ]
    }

    fn on_activate(&self) -> Result<(), String> {
        tracing::info!("心理咨询专家已激活");
        Ok(())
    }

    fn on_deactivate(&self) -> Result<(), String> {
        tracing::info!("心理咨询专家已停用");
        Ok(())
    }

    /// 处理钩子：在响应前检查是否需要危机干预
    fn handle_hook(&self, point: HookPoint, ctx: HookContext) -> HookResult {
        if point == HookPoint::BeforeResponse {
            let input_lower = ctx.input.to_lowercase();

            // 检测心理危机关键词
            let crisis_keywords = ["自杀", "不想活", "死了算了", "结束生命", "自残"];
            for keyword in &crisis_keywords {
                if input_lower.contains(keyword) {
                    // 返回阻止信息，提示用户寻求专业帮助
                    tracing::warn!("检测到心理危机关键词: {}", ctx.input);
                    return HookResult::modify_response(
                        "我注意到你可能正在经历非常困难的时刻。\
                        如果你有任何关于自我伤害的想法，我真的很担心你。\
                        请你现在联系身边信任的人，或者拨打心理危机干预热线：\
                        全国心理援助热线：400-161-9995\
                        北京心理危机研究与干预中心：010-82951332\
                        如果你有紧急危险，请立即拨打120或110。\
                        你不是一个人，有人关心你，也有人可以帮助你。"
                            .to_string(),
                    );
                }
            }
        }
        HookResult::continue_()
    }
}

// ── 专家技能 ──────────────────────────────────────────

/// 心情检测技能
pub struct MoodCheckSkill;

#[async_trait]
impl Skill for MoodCheckSkill {
    fn name(&self) -> &str {
        "mood_check"
    }

    fn description(&self) -> &str {
        "帮助用户了解当前的情绪状态，提供情绪调节建议"
    }

    fn keywords(&self) -> Vec<String> {
        vec![
            "心情".to_string(),
            "情绪".to_string(),
            "不开心".to_string(),
            "难过".to_string(),
            "烦躁".to_string(),
        ]
    }

    fn priority(&self) -> i32 {
        10
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = self.keywords();
        let mut score = 0.0f32;
        for kw in &keywords {
            if input.contains(kw) {
                score += 1.0;
            }
        }
        (score / keywords.len() as f32).min(1.0)
    }

    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::Simple)
    }

    async fn execute_simple(&self, _ctx: &mut SkillContext<'_>) -> Result<String> {
        Ok(
            "我感受到你可能正在经历一些情绪波动。这很正常，每个人都会有这样的时候。\
            你愿意多和我说说，是什么事情让你有这样的感受吗？\
            有时候，把心里的话说出来，本身就是一种疗愈。"
                .to_string(),
        )
    }
}

/// 压力缓解技能
pub struct StressReliefSkill;

#[async_trait]
impl Skill for StressReliefSkill {
    fn name(&self) -> &str {
        "stress_relief"
    }

    fn description(&self) -> &str {
        "提供压力管理和放松技巧"
    }

    fn keywords(&self) -> Vec<String> {
        vec![
            "压力".to_string(),
            "焦虑".to_string(),
            "紧张".to_string(),
            "放松".to_string(),
            "减压".to_string(),
            "失眠".to_string(),
        ]
    }

    fn priority(&self) -> i32 {
        10
    }

    fn matches(&self, input: &str) -> f32 {
        let keywords = self.keywords();
        let mut score = 0.0f32;
        for kw in &keywords {
            if input.contains(kw) {
                score += 1.0;
            }
        }
        (score / keywords.len() as f32).min(1.0)
    }

    fn flow_template(&self) -> Option<FlowTemplate> {
        Some(FlowTemplate::Simple)
    }

    async fn execute_simple(&self, _ctx: &mut SkillContext<'_>) -> Result<String> {
        Ok(
            "压力是生活中很常见的体验，你并不孤单。让我分享一个简单的放松练习：\n\n\
            🌿 **4-7-8 呼吸法**\n\n\
            1. 用鼻子安静地吸气，心里数 4 秒\n\
            2. 屏住呼吸，数 7 秒\n\
            3. 用嘴巴慢慢呼气，数 8 秒\n\n\
            重复 3-4 次，你可能会感到平静一些。\n\n\
            同时也想提醒你，压力本身不是问题，\
            关键是我们如何看待和应对它。\n\
            你愿意和我说说，最近是什么让你感到有压力吗？"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expert_info() {
        let expert = PsychologyExpert::new();
        let info = expert.info();
        assert_eq!(info.id, "psychology");
        assert_eq!(info.name, "心理咨询专家");
        assert!(!info.keywords.is_empty());
    }

    #[test]
    fn test_expert_persona() {
        let expert = PsychologyExpert::new();
        let persona = expert.persona();
        assert_eq!(persona.name, "暖心心理咨询师");
        assert!(persona.big_five.agreeableness > 0.8);
        assert!(!persona.traits.is_empty());
    }

    #[test]
    fn test_expert_skills() {
        let expert = PsychologyExpert::new();
        let skills = expert.skills();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name(), "mood_check");
        assert_eq!(skills[1].name(), "stress_relief");
    }

    #[test]
    fn test_expert_knowledge() {
        let expert = PsychologyExpert::new();
        let knowledge = expert.knowledge();
        assert_eq!(knowledge.len(), 3);
    }

    #[test]
    fn test_expert_matches() {
        let expert = PsychologyExpert::new();
        assert!(expert.matches("我最近心情很不好") > 0.0);
        assert!(expert.matches("压力很大怎么办") > 0.0);
        assert_eq!(expert.matches("今天天气怎么样"), 0.0);
    }
}
