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

use async_trait::async_trait;
use chrono::Local;
use serde_json::Value;
use subhuti::{
    runtime::tools::{Tool, ToolInfo, ToolResult},
    skill::{CalculatorSkill, DefaultChatSkill, FlowTemplate, SearchLongMemorySkill, WeatherSkill},
    FlowConfig, LLMConfig, LLMProvider, MemoryConfig, RuntimeConfig, SessionRecordParams, Subhuti,
    SubhutiConfig,
};

use crate::config::AppConfig;
use crate::middleware::{RequestLogLayer, TraceIdLayer};

pub async fn start_server() -> Result<()> {
    let _log_guard = crate::middleware::init_logging();

    tracing::info!("Starting Subhuti HTTP Server...");
    tracing::info!("Log files will be written to ./logs/ directory");

    dotenvy::dotenv().ok();

    let app_config = AppConfig::load().unwrap_or_else(|e| {
        eprintln!("⚠️  配置加载失败: {}", e);
        eprintln!("   使用默认配置");
        crate::config::default_config()
    });

    let config = SubhutiConfig {
        llm: LLMConfig {
            model: app_config.llm.model.clone(),
            api_url: app_config.llm.api_url.clone(),
            api_key: std::env::var("DOUBAO_API_KEY").ok(),
            temperature: app_config.llm.temperature as f32,
            max_tokens: app_config.llm.max_tokens,
        },
        provider: if app_config.test_mode.enabled {
            LLMProvider::Custom
        } else {
            match app_config.llm.provider.as_str() {
                "openai" => LLMProvider::OpenAI,
                "ollama" => LLMProvider::Ollama,
                "doubao" => LLMProvider::Doubao,
                _ => LLMProvider::Doubao,
            }
        },
        runtime: RuntimeConfig::default(),
        memory: MemoryConfig::default(),
        flow: FlowConfig::default(),
        db: None,
    };

    tracing::info!(
        "LLM Config: provider={}, model={}, test_mode={}",
        match config.provider {
            LLMProvider::OpenAI => "OpenAI",
            LLMProvider::Ollama => "Ollama",
            LLMProvider::Doubao => "Doubao",
            LLMProvider::Custom => "MockLLM",
        },
        config.llm.model,
        app_config.test_mode.enabled
    );

    let mut subhuti = create_agent(config);

    if app_config.test_mode.enabled {
        let mock_client = subhuti::runtime::llm::MockLlmClient::new(
            subhuti::runtime::llm::LLMConfig {
                model: "mock-llm".to_string(),
                api_url: "mock://local".to_string(),
                api_key: None,
                temperature: 0.0,
                max_tokens: 1024,
            },
            &app_config.test_mode.mock_responses_path,
            app_config.test_mode.mock_delay_ms,
        );
        subhuti
            .runtime()
            .set_llm(subhuti::runtime::llm::LLMClient::Mock(mock_client));
        tracing::info!(
            "✅ 测试模式已启用，使用 Mock LLM (延迟: {}ms)",
            app_config.test_mode.mock_delay_ms
        );
    }

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

    subhuti.sync_experts_to_orchestrator().await;

    let subhuti = Arc::new(subhuti);
    tracing::info!(
        "Agent built with {} skills, {} tools",
        subhuti.skill_count(),
        subhuti.runtime().get_tools().len()
    );

    let app = Router::new()
        .route("/subhuti/api/v1/chat", post(chat_handler))
        .route("/subhuti/api/v1/chat/stream", post(chat_stream_handler))
        .route("/subhuti/api/v1/skills", post(skill_list_handler))
        .route("/subhuti/api/v1/skills", get(skill_list_handler))
        .route("/subhuti/api/v1/skills/:name", post(skill_execute_handler))
        .route(
            "/subhuti/api/v1/skills/:name/stream",
            post(skill_execute_stream_handler),
        )
        .route("/subhuti/api/v1/health", get(health_handler))
        .route(
            "/subhuti/api/v1/health/detailed",
            get(health_detailed_handler),
        )
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
        .route("/subhuti/api/v1/palace/stats", get(palace_stats_handler))
        .route("/subhuti/api/v1/palace/forget", post(palace_forget_handler))
        .route("/subhuti/api/v1/palace/search", post(palace_search_handler))
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
        .route("/subhuti/api/v1/orchestrate", post(orchestrate_handler))
        .route(
            "/subhuti/api/v1/orchestrate/analyze",
            post(orchestrate_analyze_handler),
        )
        .route(
            "/subhuti/api/v1/orchestrate/match",
            post(orchestrate_match_handler),
        )
        .route(
            "/subhuti/api/v1/orchestrate/experts",
            get(orchestrate_experts_handler),
        )
        .route("/subhuti/api/v1/traces", get(traces_list_handler))
        .route("/subhuti/api/v1/traces/:id", get(traces_get_handler))
        .route("/subhuti/api/v1/traces/:id/tree", get(traces_tree_handler))
        .route("/subhuti/api/v1/sessions", get(sessions_list_handler))
        .route("/subhuti/api/v1/sessions/:id", get(sessions_get_handler))
        .nest_service("/subhuti/test", ServeDir::new("static"))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(RequestLogLayer)
        .layer(TraceIdLayer)
        .with_state(AppState {
            subhuti,
            trace_observer: Arc::new(subhuti::observe::TraceObserver::new()),
            session_observer: Arc::new(subhuti::observe::SessionObserver::new()),
        });

    let addr = app_config.http.addr.clone();

    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn create_agent(config: SubhutiConfig) -> Subhuti {
    let subhuti = Subhuti::with_config(config);
    subhuti.register_skill(WeatherSkill);
    subhuti.register_skill(CalculatorSkill);
    subhuti.register_skill(SearchLongMemorySkill);
    subhuti.register_skill(DefaultChatSkill);
    subhuti.runtime().register_tool(CalculatorTool);
    subhuti.register_expert(subhuti_expert_psychology::PsychologyExpert::new());
    subhuti
}

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

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub skill: Option<String>,
    pub flow_template: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub response: String,
    pub session_id: String,
    pub trace_id: String,
    pub skill_used: Option<String>,
    pub chain: Vec<String>,
    pub duration_ms: u64,
    pub model: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct SkillExecuteRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub flow_template: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SkillListResponse {
    pub skills: Vec<SkillInfoItem>,
}

#[derive(Debug, Serialize)]
pub struct SkillInfoItem {
    pub name: String,
    pub description: String,
    pub flow_template: Option<String>,
    pub flow_templates: Vec<String>,
    pub priority: i32,
}

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

#[derive(Debug, Serialize)]
pub struct BigFiveResponse {
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

#[derive(Debug, Serialize)]
pub struct InteractionStatsResponse {
    pub total_interactions: u32,
    pub skill_usage: std::collections::HashMap<String, u32>,
    pub avg_response_time_ms: u64,
    pub likes: u32,
    pub dislikes: u32,
}

#[derive(Debug, Serialize)]
pub struct EvolveResponse {
    pub success: bool,
    pub old_version: u32,
    pub new_version: u32,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub feedback_type: String,
    pub content: String,
    pub skill_name: String,
}

#[derive(Debug, Serialize)]
pub struct FeedbackResponse {
    pub success: bool,
    pub likes: u32,
    pub dislikes: u32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

#[derive(Debug, Deserialize)]
pub struct OrchestrateRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub chain: Option<String>,
}

#[derive(Clone)]
struct AppState {
    subhuti: Arc<Subhuti>,
    trace_observer: Arc<subhuti::observe::TraceObserver>,
    session_observer: Arc<subhuti::observe::SessionObserver>,
}

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

fn parse_flow_template(template_str: &str) -> Option<FlowTemplate> {
    match template_str.to_lowercase().as_str() {
        "simple" => Some(FlowTemplate::Simple),
        "react" => Some(FlowTemplate::ReAct),
        "plan_act" => Some(FlowTemplate::PlanAct),
        "chain_of_thought" => Some(FlowTemplate::ChainOfThought),
        _ => None,
    }
}

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

    let mut trace = state
        .trace_observer
        .create_trace(&user_id, &session_id, &req.message);
    let trace_id = trace.id.0.clone();

    let flow_template = req.flow_template.as_deref().and_then(parse_flow_template);
    let skill_name = req.skill.as_deref();

    match state
        .subhuti
        .run_simple_with_template(&user_id, &req.message, skill_name, flow_template)
        .await
    {
        Ok((response, skill_used, tokens)) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            trace.complete_success(response.clone(), duration_ms);
            state.trace_observer.store_trace(trace);

            let token_usage_str = format!("{{\"total_tokens\": {}}}", tokens.total_tokens);
            state.session_observer.record_request(&SessionRecordParams {
                session_id: &session_id,
                user_id: &user_id,
                trace_id: &trace_id,
                input: &req.message,
                output: Some(&response),
                duration_ms: Some(duration_ms),
                matched_skill: skill_used.as_deref(),
                token_usage: Some(token_usage_str),
                status: "Success",
            });

            tracing::info!(
                "Chat response: session={}, skill_used={:?}, duration={}ms, tokens={}, trace_id={}",
                session_id,
                skill_used,
                duration_ms,
                tokens.total_tokens,
                trace_id
            );

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

            trace.complete_failed(e.to_string(), duration_ms);
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

    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let tx_clone = Arc::new(tx);

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

async fn traces_list_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let summaries = state.trace_observer.list_summaries();

    let format = params.get("format").map(|s| s.as_str()).unwrap_or("json");

    if format == "html" {
        let html = generate_traces_list_html(&summaries);
        return (
            StatusCode::OK,
            [("Content-Type", "text/html; charset=utf-8")],
            html,
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": summaries,
            "total": summaries.len(),
        })),
    )
        .into_response()
}

async fn traces_get_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    match state.trace_observer.get_trace(&id) {
        Some(trace) => {
            let format = params.get("format").map(|s| s.as_str()).unwrap_or("json");

            if format == "html" {
                let html = generate_trace_detail_html(trace);
                return (
                    StatusCode::OK,
                    [("Content-Type", "text/html; charset=utf-8")],
                    html,
                )
                    .into_response();
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "data": trace,
                })),
            )
                .into_response()
        }
        None => {
            let response: axum::response::Response = (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "success": false,
                    "message": "Trace not found",
                })),
            )
                .into_response();
            response
        }
    }
}

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

async fn sessions_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = state.session_observer.list_sessions();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": sessions,
            "total": sessions.len(),
        })),
    )
}

async fn sessions_get_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.session_observer.get_session(&id) {
        Some(session) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "data": session,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "message": "Session not found",
            })),
        ),
    }
}

async fn orchestrate_handler(
    State(state): State<AppState>,
    Json(req): Json<OrchestrateRequest>,
) -> axum::response::Response {
    let start_time = std::time::Instant::now();

    let user_id = req
        .user_id
        .clone()
        .unwrap_or_else(|| "anonymous".to_string());
    let session_id = req.session_id.clone().unwrap_or_else(uuid_v4);

    let mut trace = state
        .trace_observer
        .create_trace(&user_id, &session_id, &req.message);
    let trace_id = trace.id.0.clone();

    let span = tracing::info_span!(
        "orchestrate",
        %trace_id,
        %user_id,
        %session_id,
        message_len = req.message.len(),
        chain = ?req.chain,
    );
    let _enter = span.enter();

    tracing::info!("收到编排请求");

    let result = match &req.chain {
        Some(chain_name) => {
            tracing::info!("使用指定链路: {}", chain_name);
            state
                .subhuti
                .run_orchestrated_with_strategy(&req.message, &user_id, chain_name)
                .await
        }
        None => {
            tracing::info!("使用自动链路匹配");
            state
                .subhuti
                .run_orchestrated_with_trace(&req.message, &user_id, &mut trace)
                .await
        }
    };

    match result {
        Ok(orchestration_result) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            tracing::info!(
                "编排完成: chain={}, experts={}, duration={}ms",
                orchestration_result.strategy,
                orchestration_result.expert_chain.len(),
                duration_ms,
            );

            trace.complete_success(orchestration_result.output.clone(), duration_ms);

            state.session_observer.record_request(&SessionRecordParams {
                session_id: &session_id,
                user_id: &user_id,
                trace_id: &trace_id,
                input: &req.message,
                output: Some(&orchestration_result.output),
                duration_ms: Some(duration_ms),
                matched_skill: orchestration_result
                    .expert_chain
                    .first()
                    .map(|s| s.as_str()),
                token_usage: Some(format!(
                    "{{\"total_tokens\": {}}}",
                    orchestration_result.tokens.total_tokens
                )),
                status: "Success",
            });

            let body = Json(serde_json::json!({
                "success": true,
                "output": orchestration_result.output,
                "session_id": session_id,
                "trace_id": trace_id,
                "chain": orchestration_result.strategy,
                "expert_chain": orchestration_result.expert_chain,
                "expert_outputs": orchestration_result.expert_outputs,
                "duration_ms": duration_ms,
                "critique_rounds": 0,
                "critique_records": [],
            }));

            state.trace_observer.store_trace(trace);

            (StatusCode::OK, body).into_response()
        }
        Err(e) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            tracing::error!("编排错误: {}, duration={}ms", e, duration_ms);

            trace.complete_failed(e.to_string(), duration_ms);

            let body = Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
                "session_id": session_id,
                "trace_id": trace_id,
                "duration_ms": duration_ms,
            }));

            state.trace_observer.store_trace(trace);

            (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
        }
    }
}

async fn orchestrate_analyze_handler(
    State(state): State<AppState>,
    Json(req): Json<OrchestrateRequest>,
) -> axum::response::Response {
    let profile = state.subhuti.analyze_task(&req.message).await;

    let body = Json(serde_json::json!({
        "success": true,
        "profile": profile,
        "suggested_strategy": "SimpleDispatch",
    }));
    (StatusCode::OK, body).into_response()
}

async fn orchestrate_match_handler(
    State(state): State<AppState>,
    Json(_req): Json<OrchestrateRequest>,
) -> axum::response::Response {
    let experts = state.subhuti.list_orchestrator_experts().await;

    let body = Json(serde_json::json!({
        "success": true,
        "matches": experts,
        "total": experts.len(),
    }));
    (StatusCode::OK, body).into_response()
}

async fn orchestrate_experts_handler(State(state): State<AppState>) -> axum::response::Response {
    let experts = state.subhuti.list_orchestrator_experts().await;

    let body = Json(serde_json::json!({
        "success": true,
        "data": experts,
        "total": experts.len(),
    }));
    (StatusCode::OK, body).into_response()
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

#[derive(Debug, Deserialize)]
struct LogQueryParams {
    trace_id: Option<String>,
    level: Option<String>,
    target: Option<String>,
    keyword: Option<String>,
    start: Option<String>,
    end: Option<String>,
    page: Option<usize>,
    page_size: Option<usize>,
}

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

#[derive(Debug, Serialize)]
struct LogListResponse {
    total: usize,
    page: usize,
    page_size: usize,
    logs: Vec<LogEntry>,
}

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

    let entries = std::fs::read_dir(log_dir)?;
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();
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

            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                let ts = value["timestamp"].as_str().unwrap_or("").to_string();
                let lv = value["level"].as_str().unwrap_or("").to_string();
                let tgt = value["target"].as_str().unwrap_or("").to_string();
                let fields = value["fields"].clone();
                let msg = fields["message"].as_str().unwrap_or("").to_string();
                let filename = value["filename"].as_str().map(|s| s.to_string());
                let line_number = value["line_number"].as_u64().map(|n| n as u32);

                if let Some(ref tid) = filter.trace_id {
                    let field_tid = fields["trace_id"].as_str().unwrap_or("");
                    if !field_tid.contains(tid) {
                        continue;
                    }
                }

                if let Some(ref lv_filter) = filter.level {
                    if !lv.eq_ignore_ascii_case(lv_filter) {
                        continue;
                    }
                }

                if let Some(ref tgt_filter) = filter.target {
                    if !tgt.contains(tgt_filter) {
                        continue;
                    }
                }

                if let Some(ref kw) = filter.keyword {
                    let kw_lower = kw.to_lowercase();
                    let haystack = format!("{} {}", msg, fields).to_lowercase();
                    if !haystack.contains(&kw_lower) {
                        continue;
                    }
                }

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

    let flow_template = req.flow_template.as_deref().and_then(parse_flow_template);

    let mut trace = state
        .trace_observer
        .create_trace(&user_id, &session_id, &req.message);
    let trace_id = trace.id.0.clone();

    match state
        .subhuti
        .run_simple_with_template(&user_id, &req.message, Some(&skill_name), flow_template)
        .await
    {
        Ok((response, skill_used, tokens)) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            trace.complete_success(response.clone(), duration_ms);
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

            trace.complete_failed(e.to_string(), duration_ms);
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

    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let tx_clone = Arc::new(tx);

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

        let _ = subhuti_clone
            .run_simple_with_skill(&user_id_clone, &message_clone, Some(&skill_name_clone))
            .await;
        let _ = tx_clone.blocking_send("[DONE]".to_string());
    });

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

fn generate_traces_list_html(_summaries: &[(String, String, String)]) -> String {
    String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Trace 列表 - Subhuti</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); }
        h1 { color: #333; border-bottom: 3px solid #4CAF50; padding-bottom: 10px; }
        p { color: #666; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔍 Trace 列表</h1>
        <p>Trace 列表功能已迁移到业务埋点层，调试链路请使用标准 tracing 生态工具。</p>
        <p>可通过 <code>RUST_LOG=debug</code> 环境变量查看详细日志，配合 <code>jq</code> 按 trace_id 过滤。</p>
    </div>
</body>
</html>"#,
    )
}

fn generate_trace_detail_html(trace: &subhuti::observe::Trace) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Trace 详情 - Subhuti</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); }
        h1 { color: #333; border-bottom: 3px solid #4CAF50; padding-bottom: 10px; }
        .info { background: #e8f5e9; padding: 15px; border-radius: 5px; margin: 15px 0; }
        .info p { margin: 8px 0; }
        .badge { padding: 4px 8px; border-radius: 4px; font-size: 12px; }
        .success { background: #4CAF50; color: white; }
        .error { background: #f44336; color: white; }
        pre { background: #f5f5f5; padding: 15px; border-radius: 5px; overflow-x: auto; }
        a { color: #4CAF50; text-decoration: none; }
        .expert-chain { margin: 8px 0; padding: 4px 8px; background: #fff3e0; border-radius: 4px; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔍 Trace 详情</h1>"#,
    );

    html.push_str("<div class='info'>");
    html.push_str(&format!("<p><strong>Trace ID:</strong> {}</p>", trace.id.0));
    html.push_str(&format!(
        "<p><strong>用户 ID:</strong> {}</p>",
        trace.user_id
    ));
    html.push_str(&format!(
        "<p><strong>会话 ID:</strong> {}</p>",
        trace.session_id
    ));
    html.push_str(&format!(
        "<p><strong>用户输入:</strong> {}</p>",
        trace.input
    ));

    let duration_text = trace
        .total_duration_ms
        .map(|d| format!("{}ms ({:.1}秒)", d, d as f64 / 1000.0))
        .unwrap_or_else(|| "-".to_string());
    html.push_str(&format!(
        "<p><strong>总耗时:</strong> {}</p>",
        duration_text
    ));

    if let Some(chain_name) = &trace.chain_name {
        html.push_str(&format!("<p><strong>链路名称:</strong> {}</p>", chain_name));
    }

    if !trace.expert_chain.is_empty() {
        html.push_str("<p><strong>专家执行链:</strong></p>");
        for (i, expert) in trace.expert_chain.iter().enumerate() {
            html.push_str(&format!(
                "<div class='expert-chain'>步骤 {}: {}</div>",
                i + 1,
                expert
            ));
        }
    }

    let status_class = if matches!(trace.status, subhuti::observe::TraceStatus::Success) {
        "success"
    } else {
        "error"
    };
    let status_text = format!("{:?}", trace.status);
    html.push_str(&format!(
        "<p><strong>状态:</strong> <span class='badge {}'>{}</span></p>",
        status_class, status_text
    ));

    if let Some(output) = &trace.output {
        html.push_str("<p><strong>输出:</strong></p>");
        html.push_str(&format!("<pre>{}</pre>", output));
    }

    html.push_str("</div>");

    html.push_str("<p><a href='/subhuti/api/v1/traces?format=html'>← 返回 Trace 列表</a></p>");
    html.push_str("</div></body></html>");
    html
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
