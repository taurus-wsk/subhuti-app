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
    /// 技能参数（可以是字符串或对象，统一转为字符串；LLM 常漏掉此字段，缺省为空串）
    #[serde(default, deserialize_with = "deserialize_params")]
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

/// 主管编排计划步骤（**主管层专用**，与专家内部 `PlanStep.skill_id` 语义区分）
///
/// 字段用 `expert_id` 而非 `skill_id`：主管层计划的一项 = **调度一个已注册专家**
/// （黑盒），不是调用专家内部的某个技能。此前复用 `SkillPlan`/`PlanStep` ，
/// 把专家 id 塞进名为 `skill_id` 的字段里，命名词不副实（实测易误导）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertStep {
    /// 步骤序号（从 1 开始）
    pub order: u32,
    /// 被调度的专家 id（必须来自已注册专家）
    pub expert_id: String,
    /// 步骤描述（给该专家看的任务说明）
    pub description: String,
    /// 专家参数（LLM 常漏掉此字段，缺省为空串）
    #[serde(default, deserialize_with = "deserialize_params")]
    pub params: String,
}

/// 主管编排计划：把多领域请求按顺序拆给多个专家串行执行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertPlan {
    /// 计划描述
    pub description: String,
    /// 专家步骤（严格按顺序执行，上一步输出喂给下一步）
    pub steps: Vec<ExpertStep>,
}

impl ExpertPlan {
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

/// 计划执行的汇总结果：产物 + 步骤成败统计。
///
/// 存在的意义：**整体成败必须由步骤成败聚合得出**。执行链不再"永远返回 Ok"——
/// 上层（专家 → 框架 → 编排 → 入站适配器）据此把失败判为整体失败，
/// 而不是把一个通篇写着 `[失败]` 的产物当成成功结果交给调用方。
pub struct PlanExecution {
    /// 汇总后的 markdown 产物（含每步结果与失败原因）
    pub output: String,
    /// 计划中的步骤总数
    pub total_steps: usize,
    /// 执行成功的步骤数
    pub succeeded_steps: usize,
}

impl PlanExecution {
    /// 失败的步骤数
    pub fn failed_steps(&self) -> usize {
        self.total_steps.saturating_sub(self.succeeded_steps)
    }

    /// 整体是否失败 —— **产品规则（2026-09-13 定稿）：只要有任意一步失败即判整体失败**。
    ///
    /// 为什么取严（而不是"全部步骤均失败才算失败"）：
    /// 1. 与框架多专家路径（`Orchestrator::dispatch_with_plan` 的 `all_ok`）语义一致，
    ///    全局只有一条成败口径；
    /// 2. **不随 planner 拆步粒度抖动**——同一句请求可能被 LLM 拆成 1/2/3 步，
    ///    "全败才算失败"会让同一请求时而成功时而失败；
    /// 3. 失败必须让调用方**可程序化判别**（`isError=true`），不能只埋在产物正文里。
    ///
    /// 代价（已知并接受）：`[生成代码][编译验证]` 这类计划里，编译失败会把
    /// "已生成代码"的部分交付一并判为整体失败。产物本身仍完整返回，
    /// 调用方可从 `## 步骤 N … [失败]` 读到细节。
    ///
    /// 空计划（0 步）不算失败——「无事可做」不是「失败」。
    pub fn has_failed_steps(&self) -> bool {
        self.failed_steps() > 0
    }
}

impl std::fmt::Debug for PlanExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanExecution")
            .field("total_steps", &self.total_steps)
            .field("succeeded_steps", &self.succeeded_steps)
            .field("failed_steps", &self.failed_steps())
            .field("output_len", &self.output.len())
            .finish()
    }
}

/// 解析 LLM 输出的执行计划
///
/// 兼容纯 JSON 与 markdown 代码块（```` ```json ... ``` ````）包裹两种形式。
pub fn parse_plan(output: &str) -> Result<SkillPlan> {
    let json_str = extract_json(output);
    serde_json::from_str(&json_str).map_err(|e| {
        crate::Error::Expert(format!(
            "解析执行计划失败: {}, 原始输出: {}",
            e,
            output_preview(output)
        ))
    })
}

/// 错误文案里的 LLM 原始输出**预览**：最多 200 字符。
///
/// 不截断的话，解析失败会把几千字乱码原样塞进用户可见的报错（早期实测出现过），
/// 既不可读也淹没真正的错误原因。完整输出仍可从日志里查。
const OUTPUT_PREVIEW_CHARS: usize = 200;

fn output_preview(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.chars().count() <= OUTPUT_PREVIEW_CHARS {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(OUTPUT_PREVIEW_CHARS).collect();
    format!("{}…(已截断)", head)
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
        crate::Error::Expert(format!(
            "解析规划结果失败: {}, 原始输出: {}",
            e,
            output_preview(output)
        ))
    })?;

    if value.get("question").is_some() && value.get("options").is_some() {
        let ask: AskRequest = serde_json::from_value(value).map_err(|e| {
            crate::Error::Expert(format!(
                "解析提问请求失败: {}, 原始输出: {}",
                e,
                output_preview(output)
            ))
        })?;
        Ok(PlanOrAsk::Ask(ask))
    } else {
        let plan: SkillPlan = serde_json::from_value(value).map_err(|e| {
            crate::Error::Expert(format!(
                "解析执行计划失败: {}, 原始输出: {}",
                e,
                output_preview(output)
            ))
        })?;
        Ok(PlanOrAsk::Plan(plan))
    }
}

/// 主管编排计划（或提问）产出。
///
/// 与 `PlanOrAsk` 分离：`PlanOrAsk` 的 `Plan` 承载专家内部技能计划（`SkillPlan`），
/// 主管层这里承载**专家调度计划**（`ExpertPlan`，字段为 `expert_id`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpertPlanOrAsk {
    /// 可执行的专家编排计划
    Plan(ExpertPlan),
    /// 信息不足，需要先向用户提问
    Ask(AskRequest),
}

/// 解析主管 LLM 输出的专家编排计划（计划 或 提问）。
///
/// 与 `parse_plan_or_ask` 共用 `question`/`options` 判定与 JSON 抽取逻辑，
/// 唯一区别是 `Plan` 分支落到 `ExpertPlan`（而非 `SkillPlan`）。
pub fn parse_expert_plan_or_ask(output: &str) -> Result<ExpertPlanOrAsk> {
    let json_str = extract_json(output);
    let value: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
        crate::Error::Expert(format!(
            "解析主管规划结果失败: {}, 原始输出: {}",
            e,
            output_preview(output)
        ))
    })?;

    if value.get("question").is_some() && value.get("options").is_some() {
        let ask: AskRequest = serde_json::from_value(value).map_err(|e| {
            crate::Error::Expert(format!(
                "解析提问请求失败: {}, 原始输出: {}",
                e,
                output_preview(output)
            ))
        })?;
        Ok(ExpertPlanOrAsk::Ask(ask))
    } else {
        let plan: ExpertPlan = serde_json::from_value(value).map_err(|e| {
            crate::Error::Expert(format!(
                "解析专家编排计划失败: {}, 原始输出: {}",
                e,
                output_preview(output)
            ))
        })?;
        Ok(ExpertPlanOrAsk::Plan(plan))
    }
}

/// 规划结果解析失败时的**纠错重试指令**。
///
/// 实测（2026-09-13）：LLM 偶发把 JSON 截断（如只吐到 `{"desc`）或在 JSON 前后
/// 夹带大段解释，导致 `serde_json` 报 `expected ',' or '}'`。这类失败**与用户输入
/// 无关**，纯属输出格式抖动，重试一次即可恢复——若不重试，任意请求都可能因一次
/// 格式抖动而整体失败（实测「帮我写首诗赞美大海」即被此问题击穿）。
const PLAN_RETRY_INSTRUCTION: &str = "你上一次的输出不是合法 JSON，无法解析。\n\
请**只输出一个完整、闭合的 JSON 对象**：\n\
- 不要任何解释、寒暄、前后缀文字；\n\
- 不要用 markdown 代码块包裹；\n\
- 确保括号配对、无尾随逗号；\n\
- 步骤数不超过 3 个；\n\
- 若需要提问，也必须是一个合法 JSON 对象（含 question 与 options）。";

/// 「只提问、不产出」的占位步骤特征词。
///
/// **为什么在代码层再兜一道**：即便 system prompt 已明确禁止，LLM 仍会偶发把
/// 「与用户沟通」「询问用户所在位置」这类**没有交付物**的动作排成步骤。实测
/// （2026-09-13 多轮 MCP 探针）后果被放大得很明显：
///   - 「用 rust 写一个函数并说明模块划分」→ **226.6 s**
///   - 「再补充一下错误处理」→ **95.7 s**
/// 而这些耗时换来的产物只有一句「抱歉，我无法获取您当前的地理位置」。
/// 提示词约束不可靠，故在此加一层**确定性**过滤。
///
/// 词表收得很窄：只匹配语义明确等于「停下来问用户」的短语，
/// 避免误伤「确认目标目录是否存在」这类确实有产出的步骤。
const PLACEHOLDER_STEP_MARKERS: &[&str] = &[
    "与用户沟通",
    "和用户沟通",
    "与用户交流",
    "询问用户",
    "向用户询问",
    "咨询用户",
    "与用户确认",
    "向用户确认",
    "确认需求",
    "澄清需求",
    "澄清用户",
    "收集信息",
    "获取更多信息",
    "了解用户意图",
    "了解用户需求",
];

/// 该步骤是否为「只提问、不产出」的占位步骤
pub fn is_placeholder_step(step: &PlanStep) -> bool {
    PLACEHOLDER_STEP_MARKERS
        .iter()
        .any(|m| step.description.contains(m))
}

/// 过滤掉占位步骤，返回被剔除的条数，并把剩余步骤的 `order` 重新编号为连续值
/// （执行链与前端展示都依赖 order 连续）。
///
/// 过滤后若计划为空，上层会退化为「直接对话回答」
/// （见 `DomainExpert::plan_and_execute` 的空计划分支）——这比执行一个
/// 没有交付物的提问步骤更符合预期，也更省一轮 LLM 编排。
pub fn strip_placeholder_steps(plan: &mut SkillPlan) -> usize {
    let before = plan.steps.len();
    plan.steps.retain(|s| !is_placeholder_step(s));
    for (i, s) in plan.steps.iter_mut().enumerate() {
        s.order = (i + 1) as u32;
    }
    before - plan.steps.len()
}

/// 规划结果出厂前的收尾：把「只提问不产出」的占位步骤剔掉（仅对 `Plan` 生效）。
fn finalize_plan(mut parsed: PlanOrAsk, label: &str) -> PlanOrAsk {
    if let PlanOrAsk::Plan(ref mut plan) = parsed {
        let removed = strip_placeholder_steps(plan);
        if removed > 0 {
            tracing::warn!(
                "[planner] {} 剔除 {} 个「只提问不产出」的占位步骤，剩余 {} 步",
                label,
                removed,
                plan.steps.len()
            );
        }
    }
    parsed
}

/// 两套规划 prompt（专家内部 `generate_plan` / 主管 `generate_expert_plan`）
/// 共享的**公共约束段**。抽成常量：这三条规则对「选技能」与「选专家」同等成立，
/// 此前在两端各写一份，改一块容易漏另一块。
///
/// ① 每步必须产出交付物；② 严禁「只提问、不产出」的占位步骤（实测 226s / 95.7s
/// 耗时的元凶，既无产物又把整体耗时拖长一个数量级）；③ 提问是 planner 层的**返回**
/// （question + options），不是 steps 里的一步。
const PLAN_COMMON_RULES: &str = "\
4. **每个步骤都必须产出实际交付物**（代码 / 分析 / 文件 / 结论）。\n\
   ⚠️ 严禁出现「与用户沟通」「询问用户」「确认需求」「澄清需求」「收集信息」\n\
   「了解用户意图」这类**只提问、不产出**的占位步骤——这类步骤既无交付物，\n\
   又会把整体耗时拖长一个数量级。需要什么信息就从用户需求中合理推断，\n\
   或直接在步骤描述里写明采用的假设。\n\
5. 仅当用户需求缺失完成它所必需的关键信息且无从推断（而不是含糊）时，\n\
   才允许返回一个提问（question + options）征询用户；通常不要提问。\n\
   （注意：**提问是 planner 层的返回，不是计划里的一个步骤**——不要在 steps 里造提问步骤）";

/// 计划 JSON 输出模板（两套规划共用的逐字格式）。
///
/// 注意：本常量作为 `format!` 的**参数**传入外层模板，其内部 `{}`/`{}` 的花括号
/// 不会被外层 `format!` 解析，安全。
const PLAN_OUTPUT_EXAMPLE: &str = "```json\n\
{\n  \"description\": \"计划描述\",\n  \"steps\": [\n\
    {\n      \"order\": 1, \"skill_id\": \"<技能或专家id>\", \"description\": \"步骤描述\", \"params\": \"参数\" }\n\
  ]\n}\n```";

/// 提问 JSON 输出模板（两套规划共用）。
const ASK_OUTPUT_EXAMPLE: &str = "```json\n\
{\n  \"question\": \"问题\", \"options\": [\"选项1\", \"选项2\"], \"context\": \"为何询问的说明\"\n}\n```";

/// 主管编排计划 JSON 输出模板（主管层专用，字段为 `expert_id`）。
const EXPERT_PLAN_OUTPUT_EXAMPLE: &str = "```json\n\
{\n  \"description\": \"计划描述\",\n  \"steps\": [\n\
    {\n      \"order\": 1, \"expert_id\": \"<专家id>\", \"description\": \"步骤描述\", \"params\": \"参数\" }\n\
  ]\n}\n```";

/// 调用 LLM 生成规划结果，并在**解析失败时自动纠错重试一次**。
///
/// 为什么在框架层做：`generate_plan`（专家内部规划）与 `generate_expert_plan`
/// （主管多专家编排）共享同一种"LLM 输出格式抖动"故障，收敛到一处避免各修一遍。
///
/// 重试策略：把上一次的**原始输出**作为 assistant 消息回喂，再补一条强约束的
/// 用户指令（`PLAN_RETRY_INSTRUCTION`），要求只产出合法 JSON。一次仍失败则放弃，
/// 抛出**精简后**的错误（不含原始输出全文，避免把几千字乱码甩给用户）。
async fn chat_and_parse_plan(
    llm: &Arc<dyn LLM>,
    messages: Vec<Message>,
    label: &str,
) -> Result<PlanOrAsk> {
    let llm_output = llm.chat(messages.clone()).await?;
    let first_err = match parse_plan_or_ask(&llm_output) {
        Ok(parsed) => return Ok(finalize_plan(parsed, label)),
        Err(e) => e,
    };

    tracing::warn!(
        "[planner] {} 首次输出无法解析，带纠错提示重试一次: {}",
        label,
        first_err
    );

    let mut retry_messages = messages;
    retry_messages.push(Message {
        role: Role::Assistant,
        content: llm_output,
        tool_call_id: None,
    });
    retry_messages.push(Message {
        role: Role::User,
        content: PLAN_RETRY_INSTRUCTION.to_string(),
        tool_call_id: None,
    });

    let retry_output = llm.chat(retry_messages).await?;
    parse_plan_or_ask(&retry_output)
        .map(|parsed| finalize_plan(parsed, label))
        .map_err(|second| {
            // 只带第二次（重试后）的解析错误：它已足够定位问题，
            // 且避免把两份原始输出拼成超长报错。
            crate::Error::Expert(format!(
                "规划结果解析失败（已自动重试一次仍不合法）: {}",
                second
            ))
        })
}

/// 主管版规划生成：与 `chat_and_parse_plan` 同款的「解析失败自动纠错重试一次」机制，
/// 仅落到 `ExpertPlanOrAsk`（专家编排计划）而非 `PlanOrAsk`（专家内部技能计划）。
async fn chat_and_parse_expert_plan(
    llm: &Arc<dyn LLM>,
    messages: Vec<Message>,
    label: &str,
) -> Result<ExpertPlanOrAsk> {
    let llm_output = llm.chat(messages.clone()).await?;
    let first_err = match parse_expert_plan_or_ask(&llm_output) {
        Ok(parsed) => return Ok(parsed),
        Err(e) => e,
    };

    tracing::warn!(
        "[planner] {} 首次输出无法解析，带纠错提示重试一次: {}",
        label,
        first_err
    );

    let mut retry_messages = messages;
    retry_messages.push(Message {
        role: Role::Assistant,
        content: llm_output,
        tool_call_id: None,
    });
    retry_messages.push(Message {
        role: Role::User,
        content: PLAN_RETRY_INSTRUCTION.to_string(),
        tool_call_id: None,
    });

    let retry_output = llm.chat(retry_messages).await?;
    parse_expert_plan_or_ask(&retry_output).map_err(|second| {
        crate::Error::Expert(format!(
            "主管编排计划解析失败（已自动重试一次仍不合法）: {}",
            second
        ))
    })
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
2. 当用户需求已明确给出（如目标目录、语言、项目名、具体任务）时，直接规划并执行
3. 如果用户只是闲聊或询问信息，只需使用 chat 技能
{}
6. 以 JSON 格式返回执行计划（除非规则 5 需要提问）"#,
        expert_name, skills_desc, PLAN_COMMON_RULES
    );

    let user_prompt = format!(
        "用户需求：\n{}\n\n请制定执行计划，以以下 JSON 格式返回：\n{}\n\n仅当确实缺失关键信息、且无法从上下文中推断时，才改为返回提问：\n{}",
        input, PLAN_OUTPUT_EXAMPLE, ASK_OUTPUT_EXAMPLE
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

    chat_and_parse_plan(llm, messages, &format!("专家 {} 内部规划", expert_name)).await
}

/// 框架主管（Planner/ReAct 主管）专用的「专家编排计划」生成器。
///
/// 与专家内部的 `generate_plan` 不同：这里的"技能"就是已注册专家本身，
/// 计划步骤的字段 `expert_id` 直接是专家 id。主管据此把请求拆给一个或多个专家
/// （单领域排一个，多领域排多个）串行执行，上一步专家的输出会作为下一步专家的输入
/// （黑盒专家之间的上下文传递）。
pub async fn generate_expert_plan(
    llm: &Arc<dyn LLM>,
    input: &str,
    experts: &[SkillInfo],
    supervisor_name: &str,
) -> Result<ExpertPlanOrAsk> {
    let experts_desc: String = experts
        .iter()
        .map(|s| format!("- **{}** (id=`{}`): {}", s.name, s.id, s.description))
        .collect::<Vec<_>>()
        .join("\n");

    let system_prompt = format!(
        r#"你是 {}，负责把用户请求拆给合适的专家处理（可单专家或多专家）。

可用的专家（expert_id 必须严格从下列 id 中选择，不要编造）：
{}

规则：
1. 先判断请求涉及几个领域；只涉及单个领域时，只排那一个专家即可
2. 涉及多个领域时，按最自然的执行顺序排多个专家；前一步专家的输出会作为后一步专家的输入
3. expert_id 必须精确等于上面列出的某个专家 id
{}
6. 以 JSON 格式返回专家编排计划（除非规则 5 需要提问）"#,
        supervisor_name, experts_desc, PLAN_COMMON_RULES
    );

    let user_prompt = format!(
        "用户需求：\n{}\n\n请制定专家编排计划，以以下 JSON 格式返回（expert_id 字段对应专家 id）：\n{}\n\n仅当确实缺失关键信息、且无法从上下文中推断时，才改为返回提问：\n{}",
        input, EXPERT_PLAN_OUTPUT_EXAMPLE, ASK_OUTPUT_EXAMPLE
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

    chat_and_parse_expert_plan(
        llm,
        messages,
        &format!("主管 {} 多专家编排", supervisor_name),
    )
    .await
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
/// - 汇总后的执行结果（产物 + 步骤成败统计），由上层据 `has_failed_steps()` 判整体成败
pub async fn execute_plan<F, Fut>(
    expert_name: &str,
    plan: &SkillPlan,
    mut on_progress: impl FnMut(&str),
    mut run_step: F,
) -> Result<PlanExecution>
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
    let mut succeeded_steps: usize = 0;

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
                succeeded_steps += 1;
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

    Ok(PlanExecution {
        output: format!(
            "# {} 执行结果\n\n{}\n\n---\n共执行 {} 个步骤",
            expert_name,
            results.join("\n\n---\n\n"),
            total_steps
        ),
        total_steps,
        succeeded_steps,
    })
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
    fn test_parse_expert_plan_or_ask_detects_plan() {
        // 主管层 JSON：字段为 expert_id，而非技能层的 skill_id
        let json = r#"{
            "description": "跨领域协作",
            "steps": [
                {"order": 1, "expert_id": "blender", "description": "建模", "params": "角色"},
                {"order": 2, "expert_id": "rust-expert", "description": "写工具", "params": "脚本"}
            ]
        }"#;
        let parsed = parse_expert_plan_or_ask(json).unwrap();
        match parsed {
            ExpertPlanOrAsk::Plan(plan) => {
                assert_eq!(plan.step_count(), 2);
                assert_eq!(plan.steps[0].expert_id, "blender");
                assert_eq!(plan.steps[1].expert_id, "rust-expert");
            }
            ExpertPlanOrAsk::Ask(_) => panic!("应识别为专家编排计划"),
        }
    }

    #[test]
    fn test_parse_expert_plan_or_ask_detects_ask() {
        // 主管层也应能交出提问（信息不足时）
        let json = r#"{
            "question": "需要先确认目标平台",
            "options": ["macOS", "Linux"],
            "context": "不同平台的命令不同"
        }"#;
        let parsed = parse_expert_plan_or_ask(json).unwrap();
        match parsed {
            ExpertPlanOrAsk::Ask(ask) => assert_eq!(ask.question, "需要先确认目标平台"),
            ExpertPlanOrAsk::Plan(_) => panic!("应识别为提问"),
        }
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
        assert!(result.output.contains("测试专家 执行结果"));
        assert!(result.output.contains("第一步"));
        assert!(result.output.contains("第二步"));
        assert!(result.output.contains("共执行 2 个步骤"));
        // 两步全成功 → 不算整体失败
        assert_eq!(result.total_steps, 2);
        assert_eq!(result.succeeded_steps, 2);
        assert!(!result.has_failed_steps());
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
        assert!(result.output.contains("[失败]"));
        assert!(result.output.contains("成功步"));
        assert!(result.output.contains("失败步"));
        // 失败步的错误被汇总
        assert!(result.output.contains("步骤失败"));
        // 上一步输出被注入到失败步的输入（prev=ok 输出）
        assert!(result.output.contains("prev=ok 输出"));
        // 部分失败 → 严格口径下即判整体失败（失败信息必须可程序化判别）
        assert_eq!(result.succeeded_steps, 1);
        assert_eq!(result.failed_steps(), 1);
        assert!(
            result.has_failed_steps(),
            "任一步骤失败即判整体失败（严格口径）"
        );
    }

    #[tokio::test]
    async fn test_execute_plan_any_failed_step_flags_overall_failure() {
        // 规则：**任一步骤失败 → 整体失败**（与框架多专家路径 all_ok 同口径）
        let plan = SkillPlan::new("全败").add_step(PlanStep {
            order: 1,
            skill_id: "bad-a".into(),
            description: "失败步 A".into(),
            params: "".into(),
        });
        let plan = plan.add_step(PlanStep {
            order: 2,
            skill_id: "bad-b".into(),
            description: "失败步 B".into(),
            params: "".into(),
        });

        let result = execute_plan(
            "测试专家",
            &plan,
            |_| {},
            |skill_id, _, _| {
                let sid = skill_id.to_string();
                async move { Err(crate::Error::Expert(format!("{} 失败", sid))) }
            },
        )
        .await
        .unwrap();

        assert_eq!(result.total_steps, 2);
        assert_eq!(result.succeeded_steps, 0);
        assert_eq!(result.failed_steps(), 2);
        assert!(result.has_failed_steps(), "所有步骤均失败必须判整体失败");
        // 失败原因仍完整保留在产物里（用于给调用方可操作的反馈）
        assert!(result.output.contains("bad-a 失败"));
        assert!(result.output.contains("bad-b 失败"));
    }

    #[tokio::test]
    async fn test_execute_plan_empty_plan_is_not_overall_failure() {
        // 空计划（0 步）不算失败——「无事可做」不是「失败」
        let plan = SkillPlan::new("空计划");
        let result = execute_plan(
            "测试专家",
            &plan,
            |_| {},
            |_, _, _| async move { Ok("never".to_string()) },
        )
        .await
        .unwrap();
        assert_eq!(result.total_steps, 0);
        assert!(!result.has_failed_steps());
    }

    // ── Mock LLM：generate_plan 决策路径测试 ──────────

    struct MockLlm {
        config: crate::runtime::llm::LLMConfig,
        reply: Arc<std::sync::Mutex<String>>,
    }

    #[async_trait::async_trait]
    impl crate::runtime::llm::LLM for MockLlm {
        fn provider(&self) -> crate::runtime::llm::LLMProvider {
            crate::runtime::llm::LLMProvider::Custom
        }
        fn config(&self) -> &crate::runtime::llm::LLMConfig {
            &self.config
        }
        async fn chat(&self, _m: Vec<Message>) -> crate::Result<String> {
            Ok(self.reply.lock().unwrap().clone())
        }
        async fn chat_with_tools(
            &self,
            _m: Vec<Message>,
            _tools: Vec<crate::runtime::llm::ToolInfo>,
        ) -> crate::Result<crate::runtime::llm::LLMResponse> {
            Ok(crate::runtime::llm::LLMResponse {
                content: self.reply.lock().unwrap().clone(),
                tool_call: None,
                model: None,
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
            })
        }
        async fn chat_streaming(
            &self,
            _m: Vec<Message>,
            _callback: Box<dyn Fn(String) + Send>,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn health_check(&self) -> crate::Result<bool> {
            Ok(true)
        }
    }

    fn mock_llm(reply: &str) -> Arc<dyn crate::runtime::llm::LLM> {
        Arc::new(MockLlm {
            config: crate::runtime::llm::LLMConfig::default(),
            reply: Arc::new(std::sync::Mutex::new(reply.to_string())),
        })
    }

    fn skills() -> Vec<SkillInfo> {
        vec![SkillInfo {
            id: "rust-chat".into(),
            name: "rust-chat".into(),
            description: "闲聊对话".into(),
            parameters: vec!["query".into()],
        }]
    }

    #[tokio::test]
    async fn test_generate_plan_llm_returns_plan() {
        let llm = mock_llm(
            r#"{"description":"编码计划","steps":[{"order":1,"skill_id":"rust-chat","description":"回复","params":"你好"}]}"#,
        );
        let out = generate_plan(&llm, "你好", &skills(), "测试专家")
            .await
            .unwrap();
        match out {
            PlanOrAsk::Plan(p) => {
                assert_eq!(p.description, "编码计划");
                assert_eq!(p.step_count(), 1);
                assert_eq!(p.steps[0].skill_id, "rust-chat");
            }
            PlanOrAsk::Ask(_) => panic!("应识别为执行计划"),
        }
    }

    #[tokio::test]
    async fn test_generate_plan_llm_returns_ask() {
        let llm = mock_llm(
            r#"{"question":"请选择目标目录","options":["/tmp/a","/tmp/b"],"context":"编码需要目录"}"#,
        );
        let out = generate_plan(&llm, "帮我创建项目", &skills(), "测试专家")
            .await
            .unwrap();
        match out {
            PlanOrAsk::Ask(a) => {
                assert_eq!(a.question, "请选择目标目录");
                assert_eq!(a.options.len(), 2);
            }
            PlanOrAsk::Plan(_) => panic!("应识别为提问"),
        }
    }

    #[tokio::test]
    async fn test_generate_plan_llm_error_propagates() {
        let llm: Arc<dyn crate::runtime::llm::LLM> = Arc::new(MockLlm {
            config: crate::runtime::llm::LLMConfig::default(),
            reply: Arc::new(std::sync::Mutex::new(String::new())),
        });
        let result = generate_plan(&llm, "", &skills(), "测试专家").await;
        assert!(result.is_err(), "空输出应被解析为错误");
    }

    // ─── 解析失败 → 纠错重试（2026-09-13 实测故障的回归用例）──────────────

    /// 按顺序返回预设回复的 LLM，并记录每次 `chat` 收到的消息内容。
    struct ScriptedLlm {
        config: crate::runtime::llm::LLMConfig,
        replies: std::sync::Mutex<std::collections::VecDeque<String>>,
        /// 每次调用的消息内容快照（用于断言重试时带了纠错指令）
        seen: std::sync::Mutex<Vec<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl crate::runtime::llm::LLM for ScriptedLlm {
        fn provider(&self) -> crate::runtime::llm::LLMProvider {
            crate::runtime::llm::LLMProvider::Custom
        }
        fn config(&self) -> &crate::runtime::llm::LLMConfig {
            &self.config
        }
        async fn chat(&self, m: Vec<Message>) -> crate::Result<String> {
            self.seen
                .lock()
                .unwrap()
                .push(m.iter().map(|x| x.content.clone()).collect());
            Ok(self.replies.lock().unwrap().pop_front().unwrap_or_default())
        }
        async fn chat_with_tools(
            &self,
            _m: Vec<Message>,
            _tools: Vec<crate::runtime::llm::ToolInfo>,
        ) -> crate::Result<crate::runtime::llm::LLMResponse> {
            Ok(crate::runtime::llm::LLMResponse {
                content: self.replies.lock().unwrap().pop_front().unwrap_or_default(),
                tool_call: None,
                model: None,
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
            })
        }
        async fn chat_streaming(
            &self,
            _m: Vec<Message>,
            _callback: Box<dyn Fn(String) + Send>,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn health_check(&self) -> crate::Result<bool> {
            Ok(true)
        }
    }

    fn scripted(replies: Vec<&str>) -> Arc<ScriptedLlm> {
        Arc::new(ScriptedLlm {
            config: crate::runtime::llm::LLMConfig::default(),
            replies: std::sync::Mutex::new(replies.into_iter().map(String::from).collect()),
            seen: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn good_plan_json() -> &'static str {
        r#"{"description":"直答","steps":[{"order":1,"skill_id":"rust-chat","description":"回答","params":"x"}]}"#
    }

    /// 首次输出被截断（实测故障形态：只吐到 `{"desc`）→ 自动带纠错提示重试一次并恢复。
    #[tokio::test]
    async fn plan_parse_failure_retries_once_and_recovers() {
        let llm = scripted(vec!["```json\n{\n  \"desc", good_plan_json()]);
        let dyn_llm: Arc<dyn crate::runtime::llm::LLM> = llm.clone();
        let out = generate_plan(&dyn_llm, "帮我写首诗赞美大海", &skills(), "测试专家")
            .await
            .expect("首次坏 JSON 应被重试救回，而不是整体失败");
        match out {
            PlanOrAsk::Plan(p) => assert_eq!(p.step_count(), 1),
            PlanOrAsk::Ask(_) => panic!("不应识别为提问"),
        }

        let seen = llm.seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "应恰好两次 LLM 调用（1 次原始 + 1 次重试）");
        let retry = &seen[1];
        assert!(
            retry
                .iter()
                .any(|c| c.contains("只输出一个完整、闭合的 JSON 对象")),
            "重试消息必须带上纠错指令"
        );
        assert!(
            retry.iter().any(|c| c.contains("\"desc")),
            "重试消息必须回喂上一次的坏输出"
        );
    }

    /// 两次都不合法 → 报错，但**错误文案必须精简**（不含原始输出全文）。
    #[tokio::test]
    async fn plan_parse_failure_twice_yields_concise_error() {
        let junk = "抱歉，我无法完成这个请求。".repeat(50);
        let llm = scripted(vec![junk.as_str(), junk.as_str()]);
        let dyn_llm: Arc<dyn crate::runtime::llm::LLM> = llm.clone();
        let err = generate_plan(&dyn_llm, "x", &skills(), "测试专家")
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("已自动重试一次仍不合法"),
            "文案应说明已重试过: {msg}"
        );
        assert!(
            !msg.contains(&junk),
            "不应把原始输出全文塞进错误文案（应截断为预览）"
        );
        assert!(
            msg.chars().count() < 500,
            "错误文案应精简，实际 {} 字: {msg}",
            msg.chars().count()
        );
        assert_eq!(llm.seen.lock().unwrap().len(), 2, "应重试过恰好一次");
    }

    /// 首次即合法 → 不做多余重试（不白花一次 LLM 调用）。
    #[tokio::test]
    async fn plan_parse_success_does_not_retry() {
        let llm = scripted(vec![good_plan_json(), "不该被调用"]);
        let dyn_llm: Arc<dyn crate::runtime::llm::LLM> = llm.clone();
        generate_plan(&dyn_llm, "x", &skills(), "测试专家")
            .await
            .unwrap();
        assert_eq!(llm.seen.lock().unwrap().len(), 1, "解析成功时不应触发重试");
    }

    /// 输出预览：超长输出必须被截断，短输出原样保留。
    #[test]
    fn output_preview_truncates_long_text() {
        let long = "a".repeat(5000);
        let p = output_preview(&long);
        assert!(p.chars().count() <= OUTPUT_PREVIEW_CHARS + 10);
        assert!(p.ends_with("…(已截断)"));
        assert_eq!(output_preview("  hello  "), "hello");
    }

    // ─── 占位步骤过滤（实测 226s / 95.7s 耗时的元凶）──────────────

    /// 「只提问不产出」的步骤必须被剔除，且 order 重新编号连续。
    #[test]
    fn placeholder_steps_are_stripped_and_reordered() {
        let mut plan = SkillPlan::new("含占位步")
            .add_step(PlanStep {
                order: 1,
                skill_id: "rust-chat".into(),
                description: "与用户沟通以获取函数的具体用途".into(),
                params: "".into(),
            })
            .add_step(PlanStep {
                order: 2,
                skill_id: "rust-generate".into(),
                description: "生成斐波那契函数实现".into(),
                params: "".into(),
            })
            .add_step(PlanStep {
                order: 3,
                skill_id: "rust-chat".into(),
                description: "询问用户希望使用哪种错误处理风格".into(),
                params: "".into(),
            });

        let removed = strip_placeholder_steps(&mut plan);
        assert_eq!(removed, 2, "两个占位步骤都应被剔除");
        assert_eq!(plan.step_count(), 1);
        assert_eq!(plan.steps[0].skill_id, "rust-generate");
        assert_eq!(plan.steps[0].order, 1, "剔除后 order 应重排为连续值");
    }

    /// 有实际产出的步骤不能被误伤（词表必须收得足够窄）。
    #[test]
    fn productive_steps_are_kept() {
        let mut plan = SkillPlan::new("正常计划")
            .add_step(PlanStep {
                order: 1,
                skill_id: "rust-coding".into(),
                description: "检查目标目录是否存在并初始化 cargo 工程".into(),
                params: "".into(),
            })
            .add_step(PlanStep {
                order: 2,
                skill_id: "rust-generate".into(),
                description: "编写 fib 函数并运行 cargo check 验证".into(),
                params: "".into(),
            });
        assert_eq!(strip_placeholder_steps(&mut plan), 0, "正常步骤不应被剔除");
        assert_eq!(plan.step_count(), 2);
    }

    /// 全是占位步骤 → 过滤后为空计划（上层据此退化为「直接对话回答」）。
    #[tokio::test]
    async fn all_placeholder_plan_becomes_empty_via_generate_plan() {
        let json = r#"{"description":"全是占位步","steps":[
            {"order":1,"skill_id":"rust-chat","description":"与用户沟通以确认需求","params":""}
        ]}"#;
        let llm = scripted(vec![json]);
        let dyn_llm: Arc<dyn crate::runtime::llm::LLM> = llm.clone();
        let out = generate_plan(&dyn_llm, "写个函数", &skills(), "测试专家")
            .await
            .unwrap();
        match out {
            PlanOrAsk::Plan(p) => assert_eq!(
                p.step_count(),
                0,
                "占位步骤应被剔空，交由上层退化为直接回答"
            ),
            PlanOrAsk::Ask(_) => panic!("不应识别为提问"),
        }
    }

    /// 端到端：占位步骤过滤在 `generate_plan` 里生效（混入 1 个占位 + 1 个真步骤）。
    #[tokio::test]
    async fn generate_plan_strips_placeholder_but_keeps_real_step() {
        let json = r#"{"description":"混合","steps":[
            {"order":1,"skill_id":"rust-chat","description":"询问用户希望实现什么功能","params":""},
            {"order":2,"skill_id":"rust-generate","description":"生成并写入实现代码","params":"fib"}
        ]}"#;
        let llm = scripted(vec![json]);
        let dyn_llm: Arc<dyn crate::runtime::llm::LLM> = llm.clone();
        let out = generate_plan(&dyn_llm, "写个函数", &skills(), "测试专家")
            .await
            .unwrap();
        match out {
            PlanOrAsk::Plan(p) => {
                assert_eq!(p.step_count(), 1);
                assert_eq!(p.steps[0].skill_id, "rust-generate");
                assert_eq!(p.steps[0].order, 1);
            }
            PlanOrAsk::Ask(_) => panic!("不应识别为提问"),
        }
    }
}
