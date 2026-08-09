//! # 入站适配器演示：单元测试作为第二套 inbound adapter
//!
//! 六边形架构核心价值：**多套入站适配器复用同一入站端口**（ChatPort 等）。
//!
//! 当前项目调用 ChatPort 的方式：
//! 1. **HTTP 适配器**（`adapter/inbound/http`）：axum handler 调用 `state.chat_port.orchestrate()`
//! 2. **测试适配器**（本文件）：直接调用 `chat_port.orchestrate()`，无需 HTTP 服务器
//!
//! CLI（`adapter/inbound/cli`）是 HTTP 客户端（reqwest 调服务器），不直接调端口。
//!
//! 本测试演示：mock 出站端口 + 构造 AppService + 通过入站端口 trait 调用，
//! 完全不依赖 HTTP/CLI/数据库/LLM，验证应用层用例逻辑。
//! 这正是六边形"端口"的测试性收益——换 inbound adapter 零成本。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use subhuti_app::application::{
    AppService, ChatPort, ExpertQueryPort, SkillPort, SpanData, StreamEvent, TraceHandle,
    TraceObserverPort,
};
use subhuti_app::domain::dto::{
    ExpertInfo, OrchestrateRequest, OrchestrateResponse, SkillInfo, SkillResponse,
};
use subhuti_app::domain::ports::{
    ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort,
};
use subhuti_app::domain::traits::{DomainExpert, DomainRepository};

// ─── Mock 出站端口（Driven Port Mock）─────────────────────────────
//
// 测试作为 inbound adapter，不需要真实的 subhuti 框架/数据库/LLM。
// 只需 mock 出站端口（driven side），应用层即可完整运行。

/// Mock 编排引擎：返回预设响应
struct MockOrchEngine {
    output: String,
    chain: Vec<String>,
}

impl OrchestrationEnginePort for MockOrchEngine {
    fn orchestrate(
        &self,
        _msg: &str,
        _user: &str,
        _chain: &str,
        _trace_id: &str,
        _session_id: &str,
    ) -> Pin<Box<dyn Future<Output = OrchestrateResponse> + Send>> {
        let output = self.output.clone();
        let chain = self.chain.clone();
        Box::pin(async move {
            OrchestrateResponse {
                success: true,
                output,
                chain: vec![],
                expert_chain: chain,
                expert_outputs: vec![],
                duration_ms: 5,
                error: None,
            }
        })
    }

    fn analyze_task(&self, _msg: &str) -> Pin<Box<dyn Future<Output = serde_json::Value> + Send>> {
        Box::pin(async { serde_json::json!({"mock": true}) })
    }

    fn match_expert(&self, _msg: &str) -> Pin<Box<dyn Future<Output = Vec<ExpertInfo>> + Send>> {
        Box::pin(async { vec![] })
    }
}

/// Mock 专家仓库
struct MockExpertRepo {
    experts: Vec<ExpertInfo>,
}

impl ExpertRepositoryPort for MockExpertRepo {
    fn get_all(&self) -> Pin<Box<dyn Future<Output = Vec<ExpertInfo>> + Send>> {
        let experts = self.experts.clone();
        Box::pin(async move { experts })
    }

    fn get_by_id(&self, id: &str) -> Pin<Box<dyn Future<Output = Option<ExpertInfo>> + Send>> {
        let id = id.to_string();
        let experts = self.experts.clone();
        Box::pin(async move { experts.into_iter().find(|e| e.id == id) })
    }

    fn register(
        &self,
        _expert: Arc<dyn DomainExpert>,
        _repo: Arc<dyn DomainRepository>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async {})
    }

    fn active_expert(&self) -> Pin<Box<dyn Future<Output = Option<ExpertInfo>> + Send>> {
        let experts = self.experts.clone();
        Box::pin(async move { experts.into_iter().next() })
    }
}

/// Mock 技能执行器
struct MockSkillExec {
    output: String,
}

impl SkillExecutionPort for MockSkillExec {
    fn skill_list(&self) -> Pin<Box<dyn Future<Output = Vec<SkillInfo>> + Send>> {
        Box::pin(async {
            vec![SkillInfo {
                id: "mock-skill".into(),
                name: "Mock 技能".into(),
                description: "测试用".into(),
                parameters: vec![],
                expert_id: "mock-expert".into(),
                expert_name: "Mock 专家".into(),
            }]
        })
    }

    fn execute_skill(
        &self,
        skill_id: &str,
        _args: &str,
        _trace_id: &str,
        _session_id: &str,
    ) -> Pin<Box<dyn Future<Output = SkillResponse> + Send>> {
        let skill_id = skill_id.to_string();
        let output = self.output.clone();
        Box::pin(async move {
            SkillResponse {
                success: true,
                output,
                skill_id,
                expert_id: "mock-expert".into(),
                error: None,
            }
        })
    }
}

// ─── 测试辅助：构造 AppService（mock 出站端口）─────────────────────

fn make_app_service() -> AppService {
    let engine = Arc::new(MockOrchEngine {
        output: "mock 编排结果".into(),
        chain: vec!["mock-expert".into()],
    });
    let repo = Arc::new(MockExpertRepo {
        experts: vec![ExpertInfo {
            id: "mock-expert".into(),
            name: "Mock 专家".into(),
            tags: vec!["test".into()],
            skills: vec![],
        }],
    });
    let executor = Arc::new(MockSkillExec {
        output: "mock 技能结果".into(),
    });
    AppService::new(repo, engine, executor)
}

// ─── Mock Trace 观察者（演示测试适配器也能挂 trace 链路）──────────
//
// 当前项目 trace 绑定在 HTTP 中间件（TraceSessionLayer），测试不走 HTTP 所以没 trace。
// 这里手动挂 TraceObserverPort，演示"测试适配器作为第二套 inbound，也能记录 trace"。
// 对比 HTTP 适配器（middleware.rs TraceSessionLayer）：
//   HTTP: 请求 → TraceSessionLayer.create_trace → handler → store_trace
//   测试: 调端口前 create_trace → chat_port.orchestrate → store_trace
// 两者都通过 TraceObserverPort 记录链路，应用层无感知。

/// 测试用 trace 观察者：记录 trace 并打印日志
struct MockTraceObserver {
    traces: Mutex<Vec<TraceHandle>>,
}

impl MockTraceObserver {
    fn new() -> Self {
        Self {
            traces: Mutex::new(vec![]),
        }
    }

    fn count(&self) -> usize {
        self.traces.lock().unwrap().len()
    }
}

fn gen_trace_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("test-trace-{:04}", COUNTER.fetch_add(1, Ordering::SeqCst))
}

impl TraceObserverPort for MockTraceObserver {
    fn create_trace(&self, user_id: &str, session_id: &str, message: &str) -> TraceHandle {
        let trace_id = gen_trace_id();
        tracing::info!(
            "🔵 [TEST ADAPTER] create_trace: trace_id={}, user={}, session={}, message=\"{}\"",
            trace_id,
            user_id,
            session_id,
            message
        );
        TraceHandle::new(trace_id, user_id.into(), session_id.into(), message.into())
    }

    fn store_trace(&self, trace: TraceHandle) {
        tracing::info!(
            "🟢 [TEST ADAPTER] store_trace: trace_id={}, status={:?}, output={:?}, duration={}ms",
            trace.trace_id,
            trace.status(),
            trace.output(),
            trace.duration_ms().unwrap_or(0)
        );
        self.traces.lock().unwrap().push(trace);
    }

    fn record_span(&self, _trace_id: &str, _span: SpanData) {
        // 测试 mock：暂不存 span，保留空实现
    }

    fn list_summaries(&self) -> Vec<serde_json::Value> {
        vec![]
    }

    fn get_trace(&self, _id: &str) -> Option<serde_json::Value> {
        None
    }

    fn get_span_tree(&self, _id: &str) -> Option<serde_json::Value> {
        None
    }
}

/// 初始化测试日志（让 --nocapture 能看到 tracing 输出）
fn init_test_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_test_writer()
        .try_init();
}

// ─── 测试用例：通过入站端口 trait 调用应用层 ────────────────────────

/// ChatPort.orchestrate：非流式调度
#[tokio::test]
async fn chat_port_orchestrate() {
    let app = make_app_service();
    // 同一个 AppService 实例，通过 ChatPort trait 调用（和 HTTP handler 一样）
    let chat_port: Arc<dyn ChatPort> = Arc::new(app);

    let resp = chat_port
        .orchestrate(OrchestrateRequest {
            message: "你好".into(),
            user_id: Some("test-user".into()),
            session_id: None,
            chain: None,
            trace_id: None,
        })
        .await;

    assert!(resp.success);
    assert_eq!(resp.output, "mock 编排结果");
    assert_eq!(resp.expert_chain, vec!["mock-expert"]);
}

/// ChatPort.orchestrate_stream：流式调度（验证语义事件序列）
#[tokio::test]
async fn chat_port_orchestrate_stream() {
    let app = make_app_service();
    let chat_port: Arc<dyn ChatPort> = Arc::new(app);

    // orchestrate_stream 返回 Receiver<StreamEvent>，不是 Future（无需 .await）
    let mut rx = chat_port.orchestrate_stream(OrchestrateRequest {
        message: "流式测试".into(),
        user_id: None,
        session_id: None,
        chain: None,
        trace_id: None,
    });

    let mut events = vec![];
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    // 应用层只发语义事件（Start → Chunk → Done），分块由适配器负责
    assert_eq!(events.len(), 3, "应有 3 个语义事件");
    assert!(matches!(events[0], StreamEvent::Start), "第 1 个应是 Start");
    assert!(
        matches!(events[1], StreamEvent::Chunk { .. }),
        "第 2 个应是 Chunk"
    );
    match &events[2] {
        StreamEvent::Done { output, meta } => {
            assert_eq!(output, "mock 编排结果");
            assert!(meta.get("chain").is_some(), "meta 应含 chain");
            assert!(meta.get("duration_ms").is_some(), "meta 应含 duration_ms");
        }
        _ => panic!("第 3 个应是 Done"),
    }
}

/// ExpertQueryPort.list_experts：专家查询
#[tokio::test]
async fn expert_query_port_list() {
    let app = make_app_service();
    // 同一个 AppService，通过另一个入站窄端口调用
    let expert_port: Arc<dyn ExpertQueryPort> = Arc::new(app);

    let experts = expert_port.list_experts().await;
    assert_eq!(experts.len(), 1);
    assert_eq!(experts[0].id, "mock-expert");
    assert_eq!(experts[0].name, "Mock 专家");
}

/// ExpertQueryPort.active_expert + match_expert：多方法验证
#[tokio::test]
async fn expert_query_port_active_and_match() {
    let app = make_app_service();
    let expert_port: Arc<dyn ExpertQueryPort> = Arc::new(app);

    let active = expert_port.active_expert().await;
    assert!(active.is_some());
    assert_eq!(active.unwrap().id, "mock-expert");

    let matched = expert_port.match_expert("任意消息").await;
    // mock 返回空 Vec（语义合法：未匹配到）
    assert!(matched.is_empty());
}

/// SkillPort.execute_skill：技能执行
#[tokio::test]
async fn skill_port_execute() {
    let app = make_app_service();
    let skill_port: Arc<dyn SkillPort> = Arc::new(app);

    let resp = skill_port.execute_skill("mock-skill", "参数", "", "").await;
    assert!(resp.success);
    assert_eq!(resp.skill_id, "mock-skill");
    assert_eq!(resp.expert_id, "mock-expert");
}

/// SkillPort.execute_skill_stream：流式技能执行
#[tokio::test]
async fn skill_port_execute_skill_stream() {
    let app = make_app_service();
    let skill_port: Arc<dyn SkillPort> = Arc::new(app);

    let mut rx = skill_port.execute_skill_stream("mock-skill", "参数", "", "");

    let mut events = vec![];
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(matches!(events[1], StreamEvent::Chunk { .. }));
    match &events[2] {
        StreamEvent::Done { output, meta } => {
            assert_eq!(output, "mock 技能结果");
            assert!(meta.get("skill_id").is_some());
            assert!(meta.get("expert_id").is_some());
        }
        _ => panic!("第 3 个应是 Done"),
    }
}

/// SkillPort.skill_list：技能列表
#[tokio::test]
async fn skill_port_list() {
    let app = make_app_service();
    let skill_port: Arc<dyn SkillPort> = Arc::new(app);

    let skills = skill_port.skill_list().await;
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].id, "mock-skill");
    assert_eq!(skills[0].expert_id, "mock-expert");
}

// ─── 带 Trace 链路的测试（演示测试适配器也能记录执行链路）──────────

/// 测试适配器挂 trace：和 HTTP TraceSessionLayer 调用同一个 TraceObserverPort
///
/// 运行方式（看 trace 日志）：
///   cargo test chat_port_orchestrate_with_trace -- --nocapture
#[tokio::test]
async fn chat_port_orchestrate_with_trace() {
    init_test_logging();

    let app = make_app_service();
    let chat_port: Arc<dyn ChatPort> = Arc::new(app);
    let trace_observer: Arc<MockTraceObserver> = Arc::new(MockTraceObserver::new());

    let user_id = "test-user";
    let session_id = "test-session";
    let message = "你好，这是带 trace 的测试";

    // 1. 创建 trace（开始计时）—— 和 HTTP TraceSessionLayer 一样
    let mut trace = trace_observer.create_trace(user_id, session_id, message);
    let start = Instant::now();

    // 2. 通过入站端口调用应用层（和 HTTP handler 完全相同的入口）
    let resp = chat_port
        .orchestrate(OrchestrateRequest {
            message: message.into(),
            user_id: Some(user_id.into()),
            session_id: Some(session_id.into()),
            chain: None,
            trace_id: None,
        })
        .await;

    // 3. 完成 trace + 存储（和 HTTP TraceSessionLayer 一样）
    let duration_ms = start.elapsed().as_millis() as u64;
    if resp.success {
        trace.complete_success(resp.output.clone(), duration_ms);
    } else {
        trace.complete_failed(resp.error.clone().unwrap_or_default(), duration_ms);
    }
    trace_observer.store_trace(trace);

    assert!(resp.success);
    assert_eq!(resp.output, "mock 编排结果");
    assert_eq!(trace_observer.count(), 1, "应记录 1 条 trace");
}

/// 流式 + trace：测试适配器对流式调用也能挂 trace
#[tokio::test]
async fn chat_port_orchestrate_stream_with_trace() {
    init_test_logging();

    let app = make_app_service();
    let chat_port: Arc<dyn ChatPort> = Arc::new(app);
    let trace_observer: Arc<MockTraceObserver> = Arc::new(MockTraceObserver::new());

    let message = "流式 trace 测试";

    // 创建 trace
    let mut trace = trace_observer.create_trace("stream-user", "stream-session", message);
    let start = Instant::now();

    // 调用流式入站端口
    let mut rx = chat_port.orchestrate_stream(OrchestrateRequest {
        message: message.into(),
        user_id: Some("stream-user".into()),
        session_id: Some("stream-session".into()),
        chain: None,
        trace_id: None,
    });

    // 收集流事件
    let mut events = vec![];
    let mut final_output = String::new();
    while let Some(event) = rx.recv().await {
        if let StreamEvent::Chunk { content } = &event {
            final_output = content.clone();
        }
        if let StreamEvent::Done { output, .. } = &event {
            final_output = output.clone();
        }
        events.push(event);
    }

    // 完成 trace
    let duration_ms = start.elapsed().as_millis() as u64;
    trace.complete_success(final_output, duration_ms);
    trace_observer.store_trace(trace);

    assert_eq!(events.len(), 3, "应有 3 个语义事件");
    assert_eq!(trace_observer.count(), 1, "应记录 1 条 trace");
}
