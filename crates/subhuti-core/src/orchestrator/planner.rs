//! # LLM 规划器（Planner）
//!
//! 规划能力属于框架核心：使用 LLM 为用户需求生成技能执行计划。
//!
//! 设计原则：
//! - 规划是**纯机制**（用技能列表 + LLM 产出计划），不含任何专家业务
//! - 技能信息以 `SkillInfo` 传入，不感知具体专家
//! - 生成计划后由上层（专家/编排层）按顺序执行 `execute_skill`
//!
//! 迁移说明：早期规划逻辑位于应用层领域 `DomainExpert::plan_and_execute`，
//! 现已收敛到框架引擎，保持行为等价。

use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

use crate::orchestrator::SkillInfo;
use crate::runtime::llm::{Message, Role, LLM};
use crate::Result;

/// 执行计划步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// 步骤序号（从 1 开始）
    pub order: u32,
    /// 使用的技能 ID
    pub skill_id: String,
    /// 步骤描述
    pub description: String,
    /// 技能参数（可以是字符串或对象，统一转为字符串）
    #[serde(deserialize_with = "deserialize_params")]
    pub params: String,
}

/// 自定义反序列化：支持 params 为字符串或对象
fn deserialize_params<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        other => Ok(other.to_string()),
    }
}

/// 执行计划
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPlan {
    /// 计划描述
    pub description: String,
    /// 步骤列表（按顺序执行）
    pub steps: Vec<PlanStep>,
}

impl SkillPlan {
    pub fn new(description: &str) -> Self {
        Self {
            description: description.to_string(),
            steps: Vec::new(),
        }
    }

    pub fn add_step(mut self, step: PlanStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

/// 解析 LLM 输出的执行计划
///
/// 兼容纯 JSON 与 markdown 代码块（```` ```json ... ``` ````）包裹两种形式。
pub fn parse_plan(output: &str) -> Result<SkillPlan> {
    let json_str = extract_json(output);
    serde_json::from_str(&json_str)
        .map_err(|e| crate::Error::Expert(format!("解析执行计划失败: {}, 原始输出: {}", e, output)))
}

/// 从 LLM 输出中抽取 JSON 文本（兼容纯 JSON 与 markdown 代码块包裹）
fn extract_json(output: &str) -> String {
    if let Some(start) = output.find("```json") {
        let start = start + 7;
        if let Some(end) = output[start..].find("```") {
            output[start..start + end].trim().to_string()
        } else {
            output.trim().to_string()
        }
    } else if let Some(start) = output.find("```") {
        let start = start + 3;
        if let Some(end) = output[start..].find("```") {
            output[start..start + end].trim().to_string()
        } else {
            output.trim().to_string()
        }
    } else {
        output.trim().to_string()
    }
}

/// 主动提问请求：规划阶段信息不足时，专家向用户发起单选提问
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskRequest {
    /// 问题描述
    pub question: String,
    /// 候选选项（前端渲染单选卡片）
    pub options: Vec<String>,
    /// 补充上下文（向用户说明为何提问，可选）
    #[serde(default)]
    pub context: Option<String>,
}

/// 规划器的产出：可能是执行计划，也可能是需要向用户提问
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlanOrAsk {
    /// 可执行的技能执行计划
    Plan(SkillPlan),
    /// 信息不足，需要先向用户提问
    Ask(AskRequest),
}

/// 解析 LLM 输出的规划结果（计划 或 提问）
///
/// 当输出包含 `question`/`options` 字段时判定为提问，否则视为执行计划。
pub fn parse_plan_or_ask(output: &str) -> Result<PlanOrAsk> {
    let json_str = extract_json(output);
    let value: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
        crate::Error::Expert(format!("解析规划结果失败: {}, 原始输出: {}", e, output))
    })?;

    if value.get("question").is_some() && value.get("options").is_some() {
        let ask: AskRequest = serde_json::from_value(value).map_err(|e| {
            crate::Error::Expert(format!("解析提问请求失败: {}, 原始输出: {}", e, output))
        })?;
        Ok(PlanOrAsk::Ask(ask))
    } else {
        let plan: SkillPlan = serde_json::from_value(value).map_err(|e| {
            crate::Error::Expert(format!("解析执行计划失败: {}, 原始输出: {}", e, output))
        })?;
        Ok(PlanOrAsk::Plan(plan))
    }
}

/// 使用 LLM 生成执行计划
///
/// # 参数
/// - `llm`: 引擎的 LLM 客户端
/// - `input`: 用户需求
/// - `skills`: 可用技能列表（来自专家）
/// - `expert_name`: 专家名称（用于 system prompt 角色设定）
///
/// # 返回
/// - 解析后的 `PlanOrAsk`：信息不足时为 `Ask`（需先向用户提问），否则为 `Plan`
pub async fn generate_plan(
    llm: &Arc<dyn LLM>,
    input: &str,
    skills: &[SkillInfo],
    expert_name: &str,
) -> Result<PlanOrAsk> {
    let skills_desc: String = skills
        .iter()
        .map(|s| {
            format!(
                "- **{}**: {} (参数: {})",
                s.id,
                s.description,
                s.parameters.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let system_prompt = format!(
        r#"你是 {}，擅长规划任务执行。

请分析用户需求，从可用技能中选择最合适的技能组合，制定执行计划。

可用技能：
{}

规则：
1. 根据用户需求选择 1-3 个技能组合执行，技能按顺序执行，前一步输出作为后一步输入
2. 尽量直接、具体地展开执行计划，不要用「确认需求」这类占位步骤充当第一步
3. 当用户需求已明确给出（如目标目录、语言、项目名、具体任务）时，直接规划并执行
4. 如果用户只是闲聊或询问信息，只需使用 chat 技能
5. 如果用户需求缺失完成它所必需的关键信息且无从推断（而不是含糊），
   才允许返回一个提问（question + options）征询用户；通常不要提问
6. 以 JSON 格式返回执行计划（除非规则 5 需要提问）"#,
        expert_name, skills_desc
    );

    let user_prompt = format!(
        "用户需求：\n{}\n\n请制定执行计划，以以下 JSON 格式返回：\n```json\n{{\n  \"description\": \"计划描述\",\n  \"steps\": [\n    {{\n      \"order\": 1,\n      \"skill_id\": \"rust-chat\",\n      \"description\": \"步骤描述\",\n      \"params\": \"技能参数\"\n    }}\n  ]\n}}\n```\n\n仅当确实缺失关键信息、且无法从上下文中推断时，才改为返回提问：\n```json\n{{\n  \"question\": \"问题\",\n  \"options\": [\"选项1\", \"选项2\"],\n  \"context\": \"为何询问的说明\"\n}}\n```",
        input
    );

    let messages = vec![
        Message {
            role: Role::System,
            content: system_prompt,
            tool_call_id: None,
        },
        Message {
            role: Role::User,
            content: user_prompt,
            tool_call_id: None,
        },
    ];

    let llm_output = llm.chat(messages).await?;

    Ok(parse_plan_or_ask(&llm_output)?)
}

/// 按顺序执行计划（引擎侧的循环驱动机制）
///
/// 引擎只负责"驱动步骤顺序 + 结果注入 + 进度回传 + 结果汇总"这些纯机制，
/// 不感知任何技能的具体业务。单个步骤如何执行交由 `run_step` 回调提供
/// （通常来自领域专家，把 `skill_id`/`params`/上一步输出组装成自己的执行上下文）。
///
/// # 参数
/// - `expert_name`: 专家名称（用于结果汇总文案）
/// - `plan`: 已生成的执行计划
/// - `on_progress`: 进度回调（SSE 推送等）
/// - `run_step`: 单步执行器 `FnMut(skill_id, params, prev_output) -> Future`
///
/// # 返回
/// - 汇总后的执行结果 markdown
pub async fn execute_plan<F, Fut>(
    expert_name: &str,
    plan: &SkillPlan,
    mut on_progress: impl FnMut(&str),
    mut run_step: F,
) -> Result<String>
where
    F: FnMut(&str, &str, &str) -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let total_steps = plan.steps.len();

    on_progress(&format!(
        "📋 执行计划：{}\n共 {} 个步骤",
        plan.description,
        plan.step_count()
    ));

    let mut results: Vec<String> = Vec::new();
    // 上一步输出作为下一步输入
    let mut prev_output: Option<String> = None;

    for (idx, step) in plan.steps.iter().enumerate() {
        let step_num = idx + 1;
        let input = prev_output.clone().unwrap_or_default();

        on_progress(&format!(
            "⚡ 步骤 {}/{}: [{}] {} - 执行中...",
            step_num, total_steps, step.skill_id, step.description
        ));

        let step_result = run_step(&step.skill_id, &step.params, &input).await;

        match &step_result {
            Ok(output) => {
                on_progress(&format!(
                    "✅ 步骤 {}/{} 完成: {}",
                    step_num, total_steps, step.description
                ));
                results.push(format!(
                    "## 步骤 {}: {}\n\n{}",
                    step_num, step.description, output
                ));
                prev_output = Some(output.clone());
            }
            Err(e) => {
                on_progress(&format!(
                    "❌ 步骤 {}/{} 失败: {} - 错误: {}",
                    step_num, total_steps, step.description, e
                ));
                results.push(format!(
                    "## 步骤 {}: {} [失败]\n\n错误: {}",
                    step_num, step.description, e
                ));
                prev_output = Some(format!("上一步失败: {}", e));
            }
        }
    }

    Ok(format!(
        "# {} 执行结果\n\n{}\n\n---\n共执行 {} 个步骤",
        expert_name,
        results.join("\n\n---\n\n"),
        total_steps
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_plan_creation() {
        let plan = SkillPlan::new("测试计划")
            .add_step(PlanStep {
                order: 1,
                skill_id: "rust-chat".to_string(),
                description: "闲聊".to_string(),
                params: "你好".to_string(),
            })
            .add_step(PlanStep {
                order: 2,
                skill_id: "rust-coding".to_string(),
                description: "编码".to_string(),
                params: "创建项目".to_string(),
            });

        assert_eq!(plan.description, "测试计划");
        assert_eq!(plan.step_count(), 2);
        assert_eq!(plan.steps[0].skill_id, "rust-chat");
        assert_eq!(plan.steps[1].skill_id, "rust-coding");
    }

    #[test]
    fn test_parse_plan_from_json() {
        let json = r#"{
            "description": "简单计划",
            "steps": [
                {
                    "order": 1,
                    "skill_id": "rust-chat",
                    "description": "回复问候",
                    "params": "你好"
                }
            ]
        }"#;

        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.description, "简单计划");
        assert_eq!(plan.step_count(), 1);
        assert_eq!(plan.steps[0].skill_id, "rust-chat");
    }

    #[test]
    fn test_parse_plan_from_markdown_code_block() {
        let input = r#"以下是执行计划：

```json
{
  "description": "编码计划",
  "steps": [
    {
      "order": 1,
      "skill_id": "rust-chat",
      "description": "确认需求",
      "params": "用户要求创建Rust项目"
    },
    {
      "order": 2,
      "skill_id": "rust-coding",
      "description": "执行编码",
      "params": "创建项目并编写代码"
    }
  ]
}
```"#;

        let plan = parse_plan(input).unwrap();
        assert_eq!(plan.description, "编码计划");
        assert_eq!(plan.step_count(), 2);
    }

    #[test]
    fn test_parse_plan_invalid_json() {
        let input = "这不是有效的JSON";
        assert!(parse_plan(input).is_err());
    }

    #[test]
    fn test_parse_plan_or_ask_detects_ask() {
        let ask_json = r#"{
            "question": "请选择目标目录",
            "options": ["/tmp/a", "/tmp/b"],
            "context": "编码技能需要目标目录"
        }"#;
        match parse_plan_or_ask(ask_json).unwrap() {
            PlanOrAsk::Ask(ask) => {
                assert_eq!(ask.question, "请选择目标目录");
                assert_eq!(ask.options.len(), 2);
                assert!(ask.context.is_some());
            }
            PlanOrAsk::Plan(_) => panic!("应识别为提问"),
        }
    }

    #[test]
    fn test_parse_plan_or_ask_detects_plan() {
        let plan_json = r#"{
            "description": "编码计划",
            "steps": [
                { "order": 1, "skill_id": "rust-coding", "description": "编码", "params": "x" }
            ]
        }"#;
        match parse_plan_or_ask(plan_json).unwrap() {
            PlanOrAsk::Plan(plan) => assert_eq!(plan.step_count(), 1),
            PlanOrAsk::Ask(_) => panic!("应识别为执行计划"),
        }
    }

    #[test]
    fn test_plan_step_serialization() {
        let step = PlanStep {
            order: 1,
            skill_id: "rust-chat".to_string(),
            description: "测试步骤".to_string(),
            params: "参数".to_string(),
        };

        let json = serde_json::to_string(&step).unwrap();
        let deserialized: PlanStep = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.order, 1);
        assert_eq!(deserialized.skill_id, "rust-chat");
        assert_eq!(deserialized.description, "测试步骤");
    }

    #[test]
    fn test_parse_plan_with_object_params() {
        // 测试 params 为对象的情况（LLM 可能生成这种格式）
        let json = r#"{
            "description": "测试对象参数",
            "steps": [
                {
                    "order": 1,
                    "skill_id": "rust-chat",
                    "description": "测试步骤",
                    "params": {
                        "question": "你好",
                        "context": "测试"
                    }
                }
            ]
        }"#;

        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.description, "测试对象参数");
        assert_eq!(plan.step_count(), 1);
        assert_eq!(plan.steps[0].skill_id, "rust-chat");
        assert!(plan.steps[0].params.contains("question"));
    }

    #[tokio::test]
    async fn test_execute_plan_drives_steps_in_order() {
        let plan = SkillPlan::new("顺序执行")
            .add_step(PlanStep {
                order: 1,
                skill_id: "skill-a".into(),
                description: "第一步".into(),
                params: "A".into(),
            })
            .add_step(PlanStep {
                order: 2,
                skill_id: "skill-b".into(),
                description: "第二步".into(),
                params: "B".into(),
            });

        let mut progress_log: Vec<String> = Vec::new();
        let mut call_order: Vec<String> = Vec::new();

        // 模拟：每步输出 = skill_id + 上一步输入
        let result = execute_plan(
            "测试专家",
            &plan,
            |msg: &str| progress_log.push(msg.to_string()),
            |skill_id, params, prev: &str| {
                let sid = skill_id.to_string();
                let out = format!("{}:{}(prev={})", sid, params, prev);
                call_order.push(sid);
                async move { Ok(out) }
            },
        )
        .await
        .unwrap();

        // 步骤按序调用
        assert_eq!(call_order, vec!["skill-a", "skill-b"]);
        // 汇总包含两步结果
        assert!(result.contains("测试专家 执行结果"));
        assert!(result.contains("第一步"));
        assert!(result.contains("第二步"));
        assert!(result.contains("共执行 2 个步骤"));
        // 会有进度推送
        assert!(!progress_log.is_empty());
    }

    #[tokio::test]
    async fn test_execute_plan_handles_step_failure_and_continues() {
        let plan = SkillPlan::new("失败处理")
            .add_step(PlanStep {
                order: 1,
                skill_id: "ok".into(),
                description: "成功步".into(),
                params: "".into(),
            })
            .add_step(PlanStep {
                order: 2,
                skill_id: "bad".into(),
                description: "失败步".into(),
                params: "".into(),
            });

        let result = execute_plan(
            "测试专家",
            &plan,
            |_| {},
            |skill_id, _, prev: &str| {
                let sid = skill_id.to_string();
                let prev = prev.to_string();
                async move {
                    if sid == "bad" {
                        Err(crate::Error::Expert(format!("步骤失败, prev={}", prev)))
                    } else {
                        Ok(format!("{} 输出", sid))
                    }
                }
            },
        )
        .await
        .unwrap();

        // 失败标记仍汇入结果，且流程继续
        assert!(result.contains("[失败]"));
        assert!(result.contains("成功步"));
        assert!(result.contains("失败步"));
        // 失败步的错误被汇总
        assert!(result.contains("步骤失败"));
        // 上一步输出被注入到失败步的输入（prev=ok 输出）
        assert!(result.contains("prev=ok 输出"));
    }
}
