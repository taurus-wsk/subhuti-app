//! Subhuti HTTP Server - 统一网关架构
//!
//! 运行: cargo run --bin http_server
//!
//! 设计理念：
//! - **统一网关**：单一入口，智能路由到对应 Skill
//! - **Skill 路由**：AI 自动判断调用哪个 Skill
//! - **三层 Flow 概念**：框架核心 / Skill 模板 / 组合 Skill
//! - **流式输出**：除工具调用外，其他场景使用 SSE 流式输出
//!
//! API 路径设计：
//! - POST /api/v1/chat                    # 统一入口，AI 自动判断
//! - POST /api/v1/skills                 # 列出所有 Skill
//! - POST /api/v1/skills/{name}         # 执行指定 Skill
//! - POST /api/v1/skills/{name}/stream  # 流式执行
//! - GET  /api/v1/health                 # 健康检查

use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::io::Error as IoError;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

// 中间件模块
mod middleware;
use middleware::{RequestLogLayer, TraceIdLayer};

// 配置模块
mod config;
use config::AppConfig;

// 导入 subhuti 核心（使用框架统一配置）
use async_trait::async_trait;
use chrono::Local;
use serde_json::Value;
use subhuti::{
    runtime::tools::{Tool, ToolInfo, ToolResult},
    skill::{CalculatorSkill, DefaultChatSkill, FlowTemplate, SearchLongMemorySkill, WeatherSkill},
    FlowConfig, LLMConfig, LLMProvider, MemoryConfig, RuntimeConfig, Subhuti, SubhutiConfig,
};

// ============================================================
// 第二部分：会话级依赖（对应 HTTP 的 Scoped）
// ============================================================

/// 会话上下文（会话级，每次对话创建一次）
pub struct SessionContext {
    /// 会话 ID
    pub session_id: String,
    /// 用户 ID
    pub user_id: String,
    /// 创建时间
    pub created_at: String,
}

impl SessionContext {
    pub fn new(user_id: String) -> Self {
        Self {
            session_id: uuid_v4(),
            user_id,
            created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}

// ============================================================
// 第三部分：手工 DI 容器（最地道 Rust 方案）
// ============================================================

/*
三层 Flow 概念（必须分清）：

1. 框架核心主流程（ReAct Loop）
   - 位置：Flow 层内置
   - 注册：不用注册，框架内置
   - 职责：Agent 的调度大脑

2. Skill 内流程模板（FlowStep）
   - 位置：Skill 的 flow_steps() 方法
   - 注册：不用注册，Skill 自己定义
   - 职责：代码复用骨架

3. 跨 Skill 业务工作流
   - 位置：包装成「组合 Skill」
   - 注册：像普通 Skill 一样注册
   - 职责：复杂业务编排
*/

// ============================================================
// 第三部分：Skill 层（手工 DI）
// ============================================================

/// 创建配置好的 Subhuti 实例（使用框架统一配置）
pub fn create_agent(config: SubhutiConfig) -> Subhuti {
    let subhuti = Subhuti::with_config(config);

    // 注册内置 Skill
    subhuti.register_skill(WeatherSkill);
    subhuti.register_skill(CalculatorSkill);
    subhuti.register_skill(SearchLongMemorySkill); // 长期记忆检索
    subhuti.register_skill(DefaultChatSkill); // 默认聊天 Skill

    // 注册工具
    subhuti.runtime().register_tool(CalculatorTool);

    // 注册专家插件
    subhuti.register_expert(subhuti_expert_psychology::PsychologyExpert::new());

    subhuti
}

// ============================================================
// 第四部分：三层 Flow 概念落地
// ============================================================

/*
三层 Flow 概念（必须分清）：

1. 框架核心主流程（ReAct Loop）
   - 位置：Flow 层内置
   - 注册：不用注册，框架内置
   - 职责：Agent 的调度大脑

2. Skill 内流程模板（FlowStep）
   - 位置：Skill 的 flow_steps() 方法
   - 注册：不用注册，Skill 自己定义
   - 职责：代码复用骨架

3. 跨 Skill 业务工作流
   - 位置：包装成「组合 Skill」
   - 注册：像普通 Skill 一样注册
   - 职责：复杂业务编排
*/

// ============================================================
// 第五部分：内置工具实现
// ============================================================

struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "calculate".to_string(),
            description: "执行数学计算".to_string(),
            parameters: serde_json::json!({
                "expression": "string"
            }),
        }
    }

    async fn run(&self, params: Value) -> Result<ToolResult> {
        let expr = params
            .get("expression")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let result = simple_calculate(expr);

        Ok(ToolResult {
            success: true,
            content: result,
            error: None,
        })
    }
}

fn simple_calculate(expr: &str) -> String {
    let expr = expr.replace(" ", "");

    if let Some(pos) = expr.find('+') {
        if let (Ok(a), Ok(b)) = (expr[..pos].parse::<f64>(), expr[pos + 1..].parse::<f64>()) {
            return (a + b).to_string();
        }
    }
    if let Some(pos) = expr.find('-') {
        if let (Ok(a), Ok(b)) = (expr[..pos].parse::<f64>(), expr[pos + 1..].parse::<f64>()) {
            return (a - b).to_string();
        }
    }
    if let Some(pos) = expr.find('*') {
        if let (Ok(a), Ok(b)) = (expr[..pos].parse::<f64>(), expr[pos + 1..].parse::<f64>()) {
            return (a * b).to_string();
        }
    }
    if let Some(pos) = expr.find('/') {
        if let (Ok(a), Ok(b)) = (expr[..pos].parse::<f64>(), expr[pos + 1..].parse::<f64>()) {
            if b != 0.0 {
                return (a / b).to_string();
            }
        }
    }

    format!("无法计算: {}", expr)
}

// ============================================================
// 第六部分：HTTP API 定义
// ============================================================

/// Chat 请求
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    /// 直接指定使用的 Skill 名称（可选）
    /// 如果指定，则直接使用该 Skill，不进行智能匹配
    pub skill: Option<String>,
    /// 流程模板（可选）
    /// - 当有 Skill 匹配时：作为 Skill 的流程模板
    /// - 当没有 Skill 匹配时：作为框架的 Flow 类型
    pub flow_template: Option<String>,
}

/// Chat 响应
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub response: String,
    pub session_id: String,
    /// Trace ID（用于追踪请求）
    pub trace_id: String,
    pub skill_used: Option<String>,
    /// 技能调用链（展示 AI 判断过程）
    pub chain: Vec<String>,
    /// 请求处理耗时（毫秒）
    pub duration_ms: u64,
    /// 使用的模型
    pub model: Option<String>,
    /// Prompt Token 数量
    pub prompt_tokens: u32,
    /// Completion Token 数量
    pub completion_tokens: u32,
    /// 总 Token 数量
    pub total_tokens: u32,
}

/// Skill 执行请求
#[derive(Debug, Deserialize)]
pub struct SkillExecuteRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    /// 流程模板（可选，覆盖 Skill 默认模板）
    pub flow_template: Option<String>,
}

/// Skill 列表响应
#[derive(Debug, Serialize)]
pub struct SkillListResponse {
    pub skills: Vec<SkillInfoItem>,
}

/// Skill 信息项
#[derive(Debug, Serialize)]
pub struct SkillInfoItem {
    pub name: String,
    pub description: String,
    /// 使用的流程模板
    pub flow_template: Option<String>,
    /// 所有实现的流程模板版本
    pub flow_templates: Vec<String>,
    /// 优先级
    pub priority: i32,
}

/// 会话历史响应
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub session_id: String,
    pub messages: Vec<MessageItem>,
}

/// 消息项
#[derive(Debug, Serialize)]
pub struct MessageItem {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// 性格快照响应
#[derive(Debug, Serialize)]
pub struct PersonaResponse {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub tone: String,
    pub emotional_tendency: String,
    pub traits: Vec<String>,
    pub big_five: BigFiveResponse,
    pub skill_proficiency: std::collections::HashMap<String, f32>,
    pub expertise_areas: std::collections::HashMap<String, f32>,
    pub skill_affinity: std::collections::HashMap<String, f32>,
    pub interaction_stats: InteractionStatsResponse,
    pub interactions_since_last_evolve: u32,
    pub updated_at: String,
}

/// 性格五维响应
#[derive(Debug, Serialize)]
pub struct BigFiveResponse {
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

/// 互动统计响应
#[derive(Debug, Serialize)]
pub struct InteractionStatsResponse {
    pub total_interactions: u32,
    pub skill_usage: std::collections::HashMap<String, u32>,
    pub avg_response_time_ms: u64,
    pub likes: u32,
    pub dislikes: u32,
}

/// 演化结果响应
#[derive(Debug, Serialize)]
pub struct EvolveResponse {
    pub success: bool,
    pub old_version: u32,
    pub new_version: u32,
    pub message: String,
}

/// 反馈请求
#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub feedback_type: String,
    pub content: String,
    pub skill_name: String,
}

/// 反馈响应
#[derive(Debug, Serialize)]
pub struct FeedbackResponse {
    pub success: bool,
    pub likes: u32,
    pub dislikes: u32,
    pub message: String,
}

/// 错误响应
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

// ============================================================
// 第七部分：Axum Handler 实现
// ============================================================

/// 应用状态（包含 Subhuti 实例）
#[derive(Clone)]
struct AppState {
    subhuti: Arc<Subhuti>,
    trace_observer: Arc<subhuti::observe::TraceObserver>,
}

/// 统一响应枚举
#[derive(Debug)]
enum ApiResponse {
    Success(ChatResponse),
    Error(ErrorResponse),
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiResponse::Success(r) => (StatusCode::OK, Json(r)).into_response(),
            ApiResponse::Error(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
        }
    }
}

/// 解析 flow_template 字符串为 FlowTemplate 枚举
fn parse_flow_template(template_str: &str) -> Option<FlowTemplate> {
    match template_str.to_lowercase().as_str() {
        "simple" => Some(FlowTemplate::Simple),
        "react" => Some(FlowTemplate::ReAct),
        "plan_act" => Some(FlowTemplate::PlanAct),
        "chain_of_thought" => Some(FlowTemplate::ChainOfThought),
        _ => None,
    }
}

/// Chat 处理函数 - 统一网关入口
///
/// AI 自动判断调用哪个 Skill，返回调用链信息
/// 已集成 Trace 追踪和 LLM 重试机制
async fn chat_handler(State(state): State<AppState>, Json(req): Json<ChatRequest>) -> ApiResponse {
    let start_time = std::time::Instant::now();

    let user_id = req.user_id.unwrap_or_else(|| "anonymous".to_string());
    let session_id = req.session_id.unwrap_or_else(uuid_v4);

    tracing::info!(
        "Chat request: user={}, session={}, message={}, skill={:?}, flow_template={:?}",
        user_id,
        session_id,
        req.message,
        req.skill,
        req.flow_template
    );

    // 创建 Trace
    let mut trace = state
        .trace_observer
        .create_trace(&user_id, &session_id, &req.message);
    let trace_id = trace.id.0.clone();

    // 解析 flow_template（统一参数）
    let flow_template = req.flow_template.as_deref().and_then(parse_flow_template);

    // 记录当前激活的专家
    if let Some(expert_id) = state.subhuti.active_expert_id() {
        trace.record_expert(&expert_id);
    }

    // 调用 Agent（支持显式 Skill 指定和流程模板选择）
    let skill_name = req.skill.as_deref();
    match state
        .subhuti
        .run_simple_with_template(&user_id, &req.message, skill_name, flow_template)
        .await
    {
        Ok((response, skill_used, tokens)) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            // 记录 Skill 匹配
            if let Some(ref skill) = skill_used {
                trace.record_skill_match(skill, 1.0);
            }

            // 记录 LLM Token 消耗
            trace.record_llm_call(tokens.prompt_tokens as u64, tokens.completion_tokens as u64);

            // 完成 Trace
            trace.complete(response.clone());

            // 存储 Trace
            state.trace_observer.store_trace(trace);

            tracing::info!(
                "Chat response: session={}, skill_used={:?}, duration={}ms, tokens={}, trace_id={}",
                session_id,
                skill_used,
                duration_ms,
                tokens.total_tokens,
                trace_id
            );

            // 构建技能调用链
            let mut chain = Vec::new();
            if let Some(ref skill) = skill_used {
                chain.push(skill.clone());
            }

            ApiResponse::Success(ChatResponse {
                response,
                session_id,
                trace_id,
                skill_used,
                chain,
                duration_ms,
                model: tokens.model,
                prompt_tokens: tokens.prompt_tokens,
                completion_tokens: tokens.completion_tokens,
                total_tokens: tokens.total_tokens,
            })
        }
        Err(e) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            // 失败的 Span
            let span_id = trace.create_span(
                subhuti::observe::SpanKind::Response,
                "error_response".to_string(),
            );
            trace.fail_span(&span_id, e.to_string());

            // 存储 Trace（失败也保存）
            state.trace_observer.store_trace(trace);

            tracing::error!(
                "Chat error: {}, duration={}ms, trace_id={}",
                e,
                duration_ms,
                trace_id
            );
            ApiResponse::Error(ErrorResponse {
                error: e.to_string(),
                code: 500,
            })
        }
    }
}

/// Chat 流式处理函数（SSE）
///
/// 使用 Server-Sent Events 实现流式输出
/// 除工具调用外，其他场景使用流式输出
async fn chat_stream_handler(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let user_id = req.user_id.unwrap_or_else(|| "anonymous".to_string());
    let session_id = req.session_id.unwrap_or_else(uuid_v4);

    tracing::info!(
        "Chat stream request: user={}, session={}, message={}",
        user_id,
        session_id,
        req.message
    );

    // 创建 channel 用于流式输出
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let tx_clone = Arc::new(tx);

    // 启动后台任务执行 Agent
    let subhuti_clone = state.subhuti.clone();
    let message_clone = req.message.clone();
    let user_id_clone = user_id.clone();
    let tx_arc = tx_clone.clone();

    tokio::spawn(async move {
        let tx_for_callback = tx_arc.clone();
        let callback = move |chunk: String| {
            let _ = tx_for_callback.blocking_send(chunk);
        };

        let _ = subhuti_clone
            .run_simple_streaming(&user_id_clone, &message_clone, Box::new(callback))
            .await;
        let _ = tx_arc.blocking_send("[DONE]".to_string());
    });

    // 将 mpsc receiver 转换为 TryStream
    let stream = ReceiverStream::new(rx).map(|chunk| -> Result<Bytes, IoError> {
        let data = if chunk == "[DONE]" {
            Bytes::from("event: done\ndata: true\n\n")
        } else if chunk.starts_with("[ERROR]") {
            Bytes::from(format!(
                "event: error\ndata: {}\n\n",
                chunk.strip_prefix("[ERROR] ").unwrap_or(&chunk)
            ))
        } else {
            Bytes::from(format!("event: message\ndata: {}\n\n", chunk))
        };
        Ok(data)
    });

    // 发送初始响应（包含 session_id）
    let session_id_clone = session_id.clone();
    let initial_stream = futures::stream::once(async move {
        Ok::<Bytes, IoError>(Bytes::from(format!(
            "event: session_id\ndata: {}\n\n",
            session_id_clone
        )))
    });

    // 创建组合流：初始数据 + 后续流式数据
    let stream = initial_stream.chain(stream);

    // 使用流式响应
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// 健康检查
/// 获取当前性格快照
async fn persona_handler(State(state): State<AppState>) -> impl IntoResponse {
    let profile = state.subhuti.soul_profile();
    let since_evolve = state.subhuti.interactions_since_last_evolve();

    let resp = PersonaResponse {
        version: profile.version,
        name: profile.name,
        description: profile.description,
        tone: match profile.tone {
            subhuti::ToneStyle::Friendly => "友好",
            subhuti::ToneStyle::Formal => "正式",
            subhuti::ToneStyle::Casual => "随意",
            subhuti::ToneStyle::Enthusiastic => "热情",
            subhuti::ToneStyle::Calm => "冷静",
            subhuti::ToneStyle::Witty => "机智",
        }
        .to_string(),
        emotional_tendency: match profile.emotional_tendency {
            subhuti::EmotionalTendency::Optimistic => "乐观",
            subhuti::EmotionalTendency::Neutral => "中性",
            subhuti::EmotionalTendency::Cautious => "谨慎",
            subhuti::EmotionalTendency::Humorous => "幽默",
            subhuti::EmotionalTendency::Professional => "专业",
        }
        .to_string(),
        traits: profile.traits,
        big_five: BigFiveResponse {
            openness: profile.big_five.openness,
            conscientiousness: profile.big_five.conscientiousness,
            extraversion: profile.big_five.extraversion,
            agreeableness: profile.big_five.agreeableness,
            neuroticism: profile.big_five.neuroticism,
        },
        skill_proficiency: profile.skill_proficiency,
        expertise_areas: profile.expertise_areas,
        skill_affinity: profile.skill_affinity,
        interaction_stats: InteractionStatsResponse {
            total_interactions: profile.interaction_stats.total_interactions,
            skill_usage: profile.interaction_stats.skill_usage,
            avg_response_time_ms: profile.interaction_stats.avg_response_time_ms,
            likes: profile.interaction_stats.likes,
            dislikes: profile.interaction_stats.dislikes,
        },
        interactions_since_last_evolve: since_evolve,
        updated_at: profile.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    (StatusCode::OK, Json(resp))
}

/// 手动触发挥化
async fn persona_evolve_handler(State(state): State<AppState>) -> Json<EvolveResponse> {
    let old_version = state.subhuti.soul_profile().version;

    match state.subhuti.evolve_persona().await {
        Ok(_) => {
            let new_version = state.subhuti.soul_profile().version;
            Json(EvolveResponse {
                success: true,
                old_version,
                new_version,
                message: format!("性格演化成功！版本 v{} → v{}", old_version, new_version),
            })
        }
        Err(e) => Json(EvolveResponse {
            success: false,
            old_version,
            new_version: old_version,
            message: format!("演化失败: {}", e),
        }),
    }
}

/// 用户反馈（点赞/踩/评论）
async fn persona_feedback_handler(
    State(state): State<AppState>,
    Json(req): Json<FeedbackRequest>,
) -> Json<FeedbackResponse> {
    let feedback_type = match req.feedback_type.as_str() {
        "like" => subhuti::FeedbackType::Like,
        "dislike" => subhuti::FeedbackType::Dislike,
        "comment" => subhuti::FeedbackType::Comment,
        _ => {
            return Json(FeedbackResponse {
                success: false,
                likes: 0,
                dislikes: 0,
                message: "无效的反馈类型".to_string(),
            });
        }
    };

    state
        .subhuti
        .record_feedback(feedback_type.clone(), &req.content, &req.skill_name);
    let (likes, dislikes) = state.subhuti.feedback_stats();

    Json(FeedbackResponse {
        success: true,
        likes,
        dislikes,
        message: match feedback_type {
            subhuti::FeedbackType::Like => "点赞成功！".to_string(),
            subhuti::FeedbackType::Dislike => "感谢反馈，我们会改进！".to_string(),
            subhuti::FeedbackType::Comment => "评论已记录！".to_string(),
        },
    })
}

// ── 心灵宫殿 API ──────────────────────────────────────────

/// 心灵宫殿统计信息
async fn palace_stats_handler(State(state): State<AppState>) -> impl IntoResponse {
    let stats = state.subhuti.palace_stats();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": {
                "total_count": stats.total_count,
                "zone_counts": stats.zone_counts.iter()
                    .map(|(k, v)| (format!("{:?}", k), v))
                    .collect::<std::collections::HashMap<String, &usize>>(),
                "importance_counts": stats.importance_counts,
                "avg_strength": stats.avg_strength,
                "base_stats": {
                    "short_term_count": stats.base_stats.short_term_count,
                    "archive_count": stats.base_stats.archive_count,
                    "knowledge_count": stats.base_stats.knowledge_count,
                },
            },
        })),
    )
}

/// 执行遗忘周期
async fn palace_forget_handler(State(state): State<AppState>) -> impl IntoResponse {
    let forgotten = state.subhuti.run_forget_cycle();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "forgotten_count": forgotten,
            "message": format!("遗忘清理完成，共清理 {} 条记忆", forgotten),
        })),
    )
}

/// 心灵宫殿搜索
#[derive(Debug, Deserialize)]
struct PalaceSearchRequest {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    use_persona_bias: bool,
}

fn default_limit() -> usize {
    10
}

async fn palace_search_handler(
    State(state): State<AppState>,
    Json(req): Json<PalaceSearchRequest>,
) -> impl IntoResponse {
    let zone_bias = if req.use_persona_bias {
        Some(state.subhuti.persona_zone_bias())
    } else {
        None
    };

    let results = state
        .subhuti
        .memory_palace()
        .search(&req.query, req.limit, zone_bias.as_ref());

    let response: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.memory.base.id,
                "content": r.memory.base.content,
                "zone": format!("{:?}", r.memory.zone),
                "zone_name": r.memory.zone.name(),
                "importance": r.memory.importance as u32,
                "strength": r.memory.strength,
                "relevance_score": r.relevance_score,
                "final_score": r.final_score,
                "activation_count": r.memory.activation_count,
                "created_at": r.memory.base.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": response,
            "total": response.len(),
        })),
    )
}

// ── 专家插件 API ──────────────────────────────────────────

/// 专家列表
async fn experts_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    let experts = state.subhuti.list_experts();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": experts,
            "total": experts.len(),
        })),
    )
}

/// 当前激活的专家
async fn experts_active_handler(State(state): State<AppState>) -> impl IntoResponse {
    let active = state.subhuti.active_expert_info();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": active,
        })),
    )
}

async fn experts_activate_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.subhuti.activate_expert(&id) {
        Ok(_) => {
            let info = state.subhuti.active_expert_info();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "message": "专家激活成功",
                    "data": info,
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "message": format!("激活失败: {}", e),
            })),
        ),
    }
}

/// 停用专家
async fn experts_deactivate_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.subhuti.deactivate_expert() {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message": "专家已停用，恢复默认状态",
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "message": format!("停用失败: {}", e),
            })),
        ),
    }
}

/// 匹配专家请求
#[derive(Debug, Deserialize)]
struct MatchExpertRequest {
    input: String,
}

async fn experts_match_handler(
    State(state): State<AppState>,
    Json(req): Json<MatchExpertRequest>,
) -> impl IntoResponse {
    let matched = state.subhuti.match_expert(&req.input);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": matched,
        })),
    )
}

/// 获取所有插件的详细信息（包括状态）
async fn experts_plugins_handler(State(state): State<AppState>) -> impl IntoResponse {
    let plugins = state.subhuti.list_plugins();
    let plugins_json: Vec<serde_json::Value> = plugins
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.manifest.id,
                "name": p.manifest.name,
                "version": p.manifest.version,
                "description": p.manifest.description,
                "category": p.manifest.category.to_string(),
                "keywords": p.manifest.keywords,
                "author": p.manifest.author.map(|a| a.name),
                "state": p.state.to_string(),
                "enabled_at": p.enabled_at,
                "activated_at": p.activated_at,
                "disabled_at": p.disabled_at,
                "permissions": {
                    "file_read": p.manifest.permissions.file_read,
                    "file_write": p.manifest.permissions.file_write,
                    "network": p.manifest.permissions.network,
                    "database": p.manifest.permissions.database,
                    "code_execution": p.manifest.permissions.code_execution,
                },
                "sandbox": {
                    "enabled": p.manifest.sandbox.enabled,
                    "memory_limit_mb": p.manifest.sandbox.memory_limit_mb,
                    "max_execution_time_secs": p.manifest.sandbox.max_execution_time_secs,
                    "daily_request_limit": p.manifest.sandbox.daily_request_limit,
                },
                "hooks": p.manifest.hooks.iter().map(|h| h.to_string()).collect::<Vec<_>>(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": plugins_json,
            "total": plugins_json.len(),
        })),
    )
}

/// 启用插件
async fn experts_enable_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.subhuti.enable_plugin(&id) {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message": "插件启用成功",
                "plugin_id": id,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "message": format!("启用失败: {}", e),
            })),
        ),
    }
}

/// 停用插件
async fn experts_disable_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.subhuti.disable_plugin(&id) {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message": "插件已停用",
                "plugin_id": id,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "message": format!("停用失败: {}", e),
            })),
        ),
    }
}

// ── Trace 追踪 API ──────────────────────────────────────

/// 获取 Trace 列表（摘要）
async fn traces_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    let summaries = state.trace_observer.list_summaries();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": summaries,
            "total": summaries.len(),
        })),
    )
}

/// 获取单个 Trace 详情
async fn traces_get_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.trace_observer.get_trace(&id) {
        Some(trace) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "data": trace,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "message": "Trace not found",
            })),
        ),
    }
}

/// 获取 Trace 的 Span 树（可视化）
async fn traces_tree_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.trace_observer.get_span_tree(&id) {
        Some(tree) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "data": tree,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "message": "Trace not found",
            })),
        ),
    }
}

async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "timestamp": Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
        })),
    )
}

/// 详细健康检查（包含系统各组件状态）
async fn health_detailed_handler(State(state): State<AppState>) -> impl IntoResponse {
    let report = state.subhuti.health_check();

    let components: Vec<serde_json::Value> = report
        .components
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "healthy": c.healthy,
                "details": c.details
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": if report.overall_healthy { "healthy" } else { "unhealthy" },
            "overall_healthy": report.overall_healthy,
            "timestamp": report.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "components": components
        })),
    )
}

/// 日志查询参数
#[derive(Debug, Deserialize)]
struct LogQueryParams {
    trace_id: Option<String>,
    level: Option<String>,
    target: Option<String>,
    keyword: Option<String>,
    start: Option<String>, // 新增：开始时间 ISO 8601
    end: Option<String>,   // 新增：结束时间 ISO 8601
    page: Option<usize>,
    page_size: Option<usize>,
}

/// 日志条目
#[derive(Debug, Clone, Serialize)]
struct LogEntry {
    timestamp: String,
    level: String,
    target: String,
    message: String,
    fields: serde_json::Value,
    filename: Option<String>,
    line_number: Option<u32>,
}

/// 日志列表响应
#[derive(Debug, Serialize)]
struct LogListResponse {
    total: usize,
    page: usize,
    page_size: usize,
    logs: Vec<LogEntry>,
}

/// 日志查询处理函数
async fn logs_handler(
    axum::extract::Query(params): axum::extract::Query<LogQueryParams>,
) -> impl IntoResponse {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).min(500);

    let filter = LogFilterParams {
        trace_id: params.trace_id,
        level: params.level,
        target: params.target,
        keyword: params.keyword,
        start_time: params.start,
        end_time: params.end,
    };

    match read_logs(&filter, page, page_size) {
        Ok((logs, total)) => {
            let resp = LogListResponse {
                total,
                page,
                page_size,
                logs,
            };
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to read logs: {}", e)
            })),
        ),
    }
}

/// 读取并过滤日志文件
/// 日志过滤参数
#[derive(Debug, Clone)]
struct LogFilterParams {
    trace_id: Option<String>,
    level: Option<String>,
    target: Option<String>,
    keyword: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
}

fn read_logs(
    filter: &LogFilterParams,
    page: usize,
    page_size: usize,
) -> Result<(Vec<LogEntry>, usize), Box<dyn std::error::Error>> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let log_dir = "./logs";
    let mut all_logs: Vec<LogEntry> = Vec::new();

    // 读取日志目录下的所有文件
    let entries = std::fs::read_dir(log_dir)?;
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();
    // 按文件名排序，最新的文件优先
    files.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    for entry in files {
        let path = entry.path();
        let file = File::open(&path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            // 解析 JSON
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                let ts = value["timestamp"].as_str().unwrap_or("").to_string();
                let lv = value["level"].as_str().unwrap_or("").to_string();
                let tgt = value["target"].as_str().unwrap_or("").to_string();
                let fields = value["fields"].clone();
                let msg = fields["message"].as_str().unwrap_or("").to_string();
                let filename = value["filename"].as_str().map(|s| s.to_string());
                let line_number = value["line_number"].as_u64().map(|n| n as u32);

                // 过滤 trace_id
                if let Some(ref tid) = filter.trace_id {
                    let field_tid = fields["trace_id"].as_str().unwrap_or("");
                    if !field_tid.contains(tid) {
                        continue;
                    }
                }

                // 过滤 level
                if let Some(ref lv_filter) = filter.level {
                    if !lv.eq_ignore_ascii_case(lv_filter) {
                        continue;
                    }
                }

                // 过滤 target
                if let Some(ref tgt_filter) = filter.target {
                    if !tgt.contains(tgt_filter) {
                        continue;
                    }
                }

                // 关键词搜索（在 message 和所有 fields 中搜索）
                if let Some(ref kw) = filter.keyword {
                    let kw_lower = kw.to_lowercase();
                    let haystack = format!("{} {}", msg, fields).to_lowercase();
                    if !haystack.contains(&kw_lower) {
                        continue;
                    }
                }

                // 时间范围过滤
                if let Some(ref start) = filter.start_time {
                    if ts.as_str() < start.as_str() {
                        continue;
                    }
                }
                if let Some(ref end) = filter.end_time {
                    if ts.as_str() > end.as_str() {
                        continue;
                    }
                }

                all_logs.push(LogEntry {
                    timestamp: ts,
                    level: lv,
                    target: tgt,
                    message: msg,
                    fields,
                    filename,
                    line_number,
                });
            }
        }
    }

    // 按时间倒序（最新的在前面）
    all_logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let total = all_logs.len();
    let start = (page - 1) * page_size;
    let end = (start + page_size).min(total);
    let paged_logs = if start < total {
        all_logs[start..end].to_vec()
    } else {
        Vec::new()
    };

    Ok((paged_logs, total))
}

/// Skill 列表处理函数
async fn skill_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    let skills = state.subhuti.list_skills();

    let skill_infos: Vec<SkillInfoItem> = skills
        .iter()
        .map(|s| SkillInfoItem {
            name: s.name.clone(),
            description: s.description.clone(),
            flow_template: s.flow_template.map(|t| format!("{:?}", t)),
            flow_templates: s
                .flow_templates
                .iter()
                .map(|t| format!("{:?}", t))
                .collect(),
            priority: s.priority,
        })
        .collect();

    (
        StatusCode::OK,
        Json(SkillListResponse {
            skills: skill_infos,
        }),
    )
}

/// Skill 执行处理函数
async fn skill_execute_handler(
    State(state): State<AppState>,
    Path(skill_name): Path<String>,
    Json(req): Json<SkillExecuteRequest>,
) -> ApiResponse {
    let start_time = std::time::Instant::now();

    let user_id = req.user_id.unwrap_or_else(|| "anonymous".to_string());
    let session_id = req.session_id.unwrap_or_else(uuid_v4);

    tracing::info!(
        "Skill execute request: skill={}, session={}, message={}, flow_template={:?}",
        skill_name,
        session_id,
        req.message,
        req.flow_template
    );

    // 解析 flow_template（统一参数）
    let flow_template = req.flow_template.as_deref().and_then(parse_flow_template);

    // 调用指定 Skill（支持动态流程模板选择）
    // 创建 Trace
    let mut trace = state
        .trace_observer
        .create_trace(&user_id, &session_id, &req.message);
    let trace_id = trace.id.0.clone();
    trace.record_skill_match(&skill_name, 1.0);

    match state
        .subhuti
        .run_simple_with_template(&user_id, &req.message, Some(&skill_name), flow_template)
        .await
    {
        Ok((response, skill_used, tokens)) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            // 记录 LLM Token 消耗
            trace.record_llm_call(tokens.prompt_tokens as u64, tokens.completion_tokens as u64);
            trace.complete(response.clone());
            state.trace_observer.store_trace(trace);

            tracing::info!(
                "Skill execute response: skill={}, duration={}ms, tokens={}, trace_id={}",
                skill_name,
                duration_ms,
                tokens.total_tokens,
                trace_id
            );

            ApiResponse::Success(ChatResponse {
                response,
                session_id,
                trace_id,
                skill_used,
                chain: vec![skill_name],
                duration_ms,
                model: tokens.model,
                prompt_tokens: tokens.prompt_tokens,
                completion_tokens: tokens.completion_tokens,
                total_tokens: tokens.total_tokens,
            })
        }
        Err(e) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            let span_id = trace.create_span(
                subhuti::observe::SpanKind::SkillExecute,
                "skill_error".to_string(),
            );
            trace.fail_span(&span_id, e.to_string());
            state.trace_observer.store_trace(trace);

            tracing::error!(
                "Skill execute error: {}, duration={}ms, trace_id={}",
                e,
                duration_ms,
                trace_id
            );
            ApiResponse::Error(ErrorResponse {
                error: e.to_string(),
                code: 500,
            })
        }
    }
}

/// Skill 流式执行处理函数
async fn skill_execute_stream_handler(
    State(state): State<AppState>,
    Path(skill_name): Path<String>,
    Json(req): Json<SkillExecuteRequest>,
) -> impl IntoResponse {
    let user_id = req.user_id.unwrap_or_else(|| "anonymous".to_string());
    let session_id = req.session_id.unwrap_or_else(uuid_v4);

    tracing::info!(
        "Skill stream request: skill={}, session={}, message={}",
        skill_name,
        session_id,
        req.message
    );

    // 创建 channel 用于流式输出
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let tx_clone = Arc::new(tx);

    // 启动后台任务执行 Skill
    let subhuti_clone = state.subhuti.clone();
    let message_clone = req.message.clone();
    let user_id_clone = user_id.clone();
    let skill_name_clone = skill_name.clone();
    let tx_arc_clone = tx_clone.clone();

    tokio::spawn(async move {
        let tx_for_callback = tx_arc_clone.clone();
        let _callback = move |chunk: String| {
            let _ = tx_for_callback.blocking_send(chunk);
        };

        // 注意：Skill 流式执行需要使用 SkillManager 的流式接口
        // 这里暂时使用非流式接口，后续可以优化
        let _ = subhuti_clone
            .run_simple_with_skill(&user_id_clone, &message_clone, Some(&skill_name_clone))
            .await;
        let _ = tx_clone.blocking_send("[DONE]".to_string());
    });

    // 转换流
    let stream = ReceiverStream::new(rx).map(|chunk| -> Result<Bytes, IoError> {
        let data = if chunk == "[DONE]" {
            Bytes::from("event: done\ndata: true\n\n")
        } else if chunk.starts_with("[ERROR]") {
            Bytes::from(format!(
                "event: error\ndata: {}\n\n",
                chunk.strip_prefix("[ERROR] ").unwrap_or(&chunk)
            ))
        } else {
            Bytes::from(format!("event: message\ndata: {}\n\n", chunk))
        };
        Ok(data)
    });

    // 发送初始响应（包含 session_id）
    let session_id_clone = session_id.clone();
    let initial_stream = futures::stream::once(async move {
        Ok::<Bytes, IoError>(Bytes::from(format!(
            "event: session_id\ndata: {}\n\n",
            session_id_clone
        )))
    });

    let stream = initial_stream.chain(stream);

    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
}

// ============================================================
// 第八部分：主函数
// ============================================================

#[tokio::main]
async fn main() -> Result<()> {
    // 1. 初始化日志（控制台 + 文件）
    let _log_guard = middleware::init_logging();

    tracing::info!("Starting Subhuti HTTP Server...");
    tracing::info!("Log files will be written to ./logs/ directory");

    // 2. 加载环境变量
    dotenvy::dotenv().ok();

    // 3. 加载 TOML 配置（支持环境变量覆盖）
    let app_config = AppConfig::load().unwrap_or_else(|e| {
        eprintln!("⚠️  配置加载失败: {}", e);
        eprintln!("   使用默认配置");
        config::default_config()
    });

    // 4. 创建框架统一配置
    let config = SubhutiConfig {
        llm: LLMConfig {
            model: app_config.llm.model.clone(),
            api_url: app_config.llm.api_url.clone(),
            api_key: std::env::var("DOUBAO_API_KEY").ok(),
            temperature: app_config.llm.temperature as f32,
            max_tokens: app_config.llm.max_tokens,
        },
        provider: match app_config.llm.provider.as_str() {
            "openai" => LLMProvider::OpenAI,
            "ollama" => LLMProvider::Ollama,
            "doubao" => LLMProvider::Doubao,
            _ => LLMProvider::Doubao,
        },
        runtime: RuntimeConfig::default(),
        memory: MemoryConfig::default(),
        flow: FlowConfig::default(),
        db: None,
    };

    tracing::info!(
        "LLM Config: provider={}, model={}",
        match config.provider {
            LLMProvider::OpenAI => "OpenAI",
            LLMProvider::Ollama => "Ollama",
            LLMProvider::Doubao => "Doubao",
            LLMProvider::Custom => "Custom",
        },
        config.llm.model
    );

    // 4. 创建 Agent（使用框架配置）
    let mut subhuti = create_agent(config);

    // 5. 初始化数据库
    let db_config = subhuti::DbConfig {
        host: app_config.database.host.clone(),
        port: app_config.database.port,
        database: app_config.database.database.clone(),
        username: app_config.database.username.clone(),
        password: app_config.database.password.clone(),
        max_connections: app_config.database.max_connections,
    };

    match subhuti.init_database(&db_config).await {
        Ok(_) => tracing::info!("Database initialized successfully"),
        Err(e) => tracing::warn!("Database initialization failed (using file storage): {}", e),
    }

    let subhuti = Arc::new(subhuti);
    tracing::info!(
        "Agent built with {} skills, {} tools",
        subhuti.skill_count(),
        subhuti.runtime().get_tools().len()
    );

    // 5. 创建 Axum 应用（统一网关路由）
    let app = Router::new()
        // 统一入口 - AI 自动判断调用哪个 Skill
        .route("/subhuti/api/v1/chat", post(chat_handler))
        .route("/subhuti/api/v1/chat/stream", post(chat_stream_handler))
        // Skill 列表
        .route("/subhuti/api/v1/skills", post(skill_list_handler))
        .route("/subhuti/api/v1/skills", get(skill_list_handler))
        // Skill 执行
        .route("/subhuti/api/v1/skills/:name", post(skill_execute_handler))
        .route(
            "/subhuti/api/v1/skills/:name/stream",
            post(skill_execute_stream_handler),
        )
        // 健康检查
        .route("/subhuti/api/v1/health", get(health_handler))
        .route(
            "/subhuti/api/v1/health/detailed",
            get(health_detailed_handler),
        )
        // 日志查询
        .route("/subhuti/api/v1/logs", get(logs_handler))
        .route("/subhuti/api/v1/persona", get(persona_handler))
        .route(
            "/subhuti/api/v1/persona/evolve",
            post(persona_evolve_handler),
        )
        .route(
            "/subhuti/api/v1/persona/feedback",
            post(persona_feedback_handler),
        )
        // 心灵宫殿
        .route("/subhuti/api/v1/palace/stats", get(palace_stats_handler))
        .route("/subhuti/api/v1/palace/forget", post(palace_forget_handler))
        .route("/subhuti/api/v1/palace/search", post(palace_search_handler))
        // 专家插件
        .route("/subhuti/api/v1/experts", get(experts_list_handler))
        .route(
            "/subhuti/api/v1/experts/plugins",
            get(experts_plugins_handler),
        )
        .route(
            "/subhuti/api/v1/experts/active",
            get(experts_active_handler),
        )
        .route(
            "/subhuti/api/v1/experts/:id/activate",
            post(experts_activate_handler),
        )
        .route(
            "/subhuti/api/v1/experts/deactivate",
            post(experts_deactivate_handler),
        )
        .route(
            "/subhuti/api/v1/experts/:id/enable",
            post(experts_enable_handler),
        )
        .route(
            "/subhuti/api/v1/experts/:id/disable",
            post(experts_disable_handler),
        )
        .route("/subhuti/api/v1/experts/match", post(experts_match_handler))
        // Trace 追踪（可观测性）
        .route("/subhuti/api/v1/traces", get(traces_list_handler))
        .route("/subhuti/api/v1/traces/:id", get(traces_get_handler))
        .route("/subhuti/api/v1/traces/:id/tree", get(traces_tree_handler))
        // 测试页面（静态文件）
        .nest_service("/subhuti/test", ServeDir::new("static"))
        // 中间件（注册顺序从内到外，执行顺序从外到内）
        // 执行顺序: RequestLogLayer → TraceIdLayer → CorsLayer → handler
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(RequestLogLayer) // 第2层：记录请求日志（能拿到 Trace ID）
        .layer(TraceIdLayer) // 第1层（最外层）：生成 Trace ID
        .with_state(AppState {
            subhuti,
            trace_observer: Arc::new(subhuti::observe::TraceObserver::new()),
        });

    // 6. 启动服务器
    let addr = app_config.http.addr.clone();

    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// UUID v4 生成（使用标准 UUID crate）
fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
