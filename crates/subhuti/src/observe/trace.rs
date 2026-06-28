//! # 可观测性 Trace 系统
//!
//! 提供 Agent 运行过程的完整追踪能力：
//! - Trace ID：每个请求的唯一标识
//! - Span：每个操作的执行记录
//! - 调用链可视化：展示完整的思考过程
//!
//! ## Trace 结构
//!
//! ```text
//! Trace (request_id)
//!   ├── Span: SkillMatch (匹配哪个技能)
//!   ├── Span: MemorySearch (检索哪些记忆)
//!   ├── Span: ToolCall (调用什么工具)
//!   ├── Span: LlmCall (LLM 请求详情)
//!   ├── Span: SkillExecute (技能执行过程)
//!   └── Span: Response (最终输出)
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Trace ID（请求唯一标识）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(pub String);

impl Default for TraceId {
    fn default() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl TraceId {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Span 类型（记录不同阶段的操作）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpanKind {
    /// 请求开始
    Request,
    /// Skill 匹配
    SkillMatch,
    /// Skill 执行
    SkillExecute,
    /// 记忆检索
    MemorySearch,
    /// 工具调用
    ToolCall,
    /// LLM 调用
    LlmCall,
    /// 钩子执行
    HookExecute,
    /// 专家切换
    ExpertSwitch,
    /// 规划执行
    PlanningExecute,
    /// 响应生成
    Response,
}

impl std::fmt::Display for SpanKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpanKind::Request => write!(f, "request"),
            SpanKind::SkillMatch => write!(f, "skill_match"),
            SpanKind::SkillExecute => write!(f, "skill_execute"),
            SpanKind::MemorySearch => write!(f, "memory_search"),
            SpanKind::ToolCall => write!(f, "tool_call"),
            SpanKind::LlmCall => write!(f, "llm_call"),
            SpanKind::HookExecute => write!(f, "hook_execute"),
            SpanKind::ExpertSwitch => write!(f, "expert_switch"),
            SpanKind::PlanningExecute => write!(f, "planning_execute"),
            SpanKind::Response => write!(f, "response"),
        }
    }
}

/// Span 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanStatus {
    /// 正在执行
    Running,
    /// 成功完成
    Success,
    /// 失败
    Failed,
    /// 被取消
    Cancelled,
}

/// Span（单个操作的执行记录）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Span ID
    pub id: String,
    /// Span 类型
    pub kind: SpanKind,
    /// 父 Span ID（可选）
    pub parent_id: Option<String>,
    /// 开始时间（相对 Trace 开始的毫秒数）
    pub start_time_ms: u64,
    /// 持续时间（毫秒）
    pub duration_ms: Option<u64>,
    /// 状态
    pub status: SpanStatus,
    /// 操作名称
    pub name: String,
    /// 输入数据（JSON）
    pub input: Option<serde_json::Value>,
    /// 输出数据（JSON）
    pub output: Option<serde_json::Value>,
    /// 错误信息
    pub error: Option<String>,
    /// 附加元数据
    pub metadata: HashMap<String, serde_json::Value>,
    /// 子 Span ID 列表
    pub children: Vec<String>,
}

impl Span {
    /// 创建新 Span
    pub fn new(kind: SpanKind, name: String, start_time_ms: u64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            parent_id: None,
            start_time_ms,
            duration_ms: None,
            status: SpanStatus::Running,
            name,
            input: None,
            output: None,
            error: None,
            metadata: HashMap::new(),
            children: Vec::new(),
        }
    }

    /// 设置父 Span
    pub fn with_parent(mut self, parent_id: &str) -> Self {
        self.parent_id = Some(parent_id.to_string());
        self
    }

    /// 设置输入
    pub fn with_input(mut self, input: serde_json::Value) -> Self {
        self.input = Some(input);
        self
    }

    /// 设置输出并完成
    pub fn complete_with_output(mut self, output: serde_json::Value, duration_ms: u64) -> Self {
        self.output = Some(output);
        self.duration_ms = Some(duration_ms);
        self.status = SpanStatus::Success;
        self
    }

    /// 设置错误并完成
    pub fn complete_with_error(mut self, error: String, duration_ms: u64) -> Self {
        self.error = Some(error);
        self.duration_ms = Some(duration_ms);
        self.status = SpanStatus::Failed;
        self
    }

    /// 添加元数据
    pub fn add_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        self.metadata.insert(key.to_string(), value);
        self
    }
}

/// Trace（完整的请求追踪记录）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    /// Trace ID
    pub id: TraceId,
    /// 用户 ID
    pub user_id: String,
    /// 会话 ID
    pub session_id: String,
    /// 输入内容
    pub input: String,
    /// 最终输出
    pub output: Option<String>,
    /// 开始时间戳
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// 结束时间戳
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 总耗时（毫秒）
    pub total_duration_ms: Option<u64>,
    /// 当前激活的专家 ID
    pub expert_id: Option<String>,
    /// 匹配的 Skill 名称
    pub matched_skill: Option<String>,
    /// 使用的工具列表
    pub tools_used: Vec<String>,
    /// LLM Token 消耗
    pub token_usage: Option<TokenUsage>,
    /// 所有 Span
    pub spans: HashMap<String, Span>,
    /// 根 Span ID
    pub root_span_id: Option<String>,
    /// 状态
    pub status: TraceStatus,
}

/// Token 消耗统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Trace 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceStatus {
    Running,
    Success,
    Failed,
}

impl Trace {
    /// 创建新 Trace
    pub fn new(user_id: &str, session_id: &str, input: &str) -> Self {
        let id = TraceId::new();
        let now = chrono::Utc::now();

        // 创建根 Span
        let root_span = Span::new(SpanKind::Request, "request".to_string(), 0);
        let root_span_id = root_span.id.clone();

        let mut spans = HashMap::new();
        spans.insert(root_span_id.clone(), root_span);

        Self {
            id,
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            input: input.to_string(),
            output: None,
            started_at: now,
            ended_at: None,
            total_duration_ms: None,
            expert_id: None,
            matched_skill: None,
            tools_used: Vec::new(),
            token_usage: None,
            spans,
            root_span_id: Some(root_span_id),
            status: TraceStatus::Running,
        }
    }

    /// 创建 Span 并添加到 Trace
    pub fn create_span(&mut self, kind: SpanKind, name: String) -> String {
        let start_time_ms = self.elapsed_ms();
        let span = Span::new(kind, name, start_time_ms);

        // 设置父 Span（如果没有指定，使用根 Span）
        if let Some(root_id) = &self.root_span_id {
            if kind != SpanKind::Request {
                let span_with_parent = span.with_parent(root_id);
                let span_id = span_with_parent.id.clone();
                self.spans.insert(span_id.clone(), span_with_parent);

                // 添加到父 Span 的 children
                if let Some(parent) = self.spans.get_mut(root_id) {
                    parent.children.push(span_id.clone());
                }

                return span_id;
            }
        }

        let span_id = span.id.clone();
        self.spans.insert(span_id.clone(), span);
        span_id
    }

    /// 创建子 Span（指定父 Span）
    pub fn create_child_span(&mut self, parent_id: &str, kind: SpanKind, name: String) -> String {
        let start_time_ms = self.elapsed_ms();
        let span = Span::new(kind, name, start_time_ms).with_parent(parent_id);
        let span_id = span.id.clone();

        self.spans.insert(span_id.clone(), span);

        // 添加到父 Span 的 children
        if let Some(parent) = self.spans.get_mut(parent_id) {
            parent.children.push(span_id.clone());
        }

        span_id
    }

    /// 完成 Span（成功）
    pub fn complete_span(&mut self, span_id: &str, output: serde_json::Value) {
        let elapsed = self.elapsed_ms();
        if let Some(span) = self.spans.get_mut(span_id) {
            span.output = Some(output);
            span.duration_ms = Some(elapsed - span.start_time_ms);
            span.status = SpanStatus::Success;
        }
    }

    /// 完成 Span（失败）
    pub fn fail_span(&mut self, span_id: &str, error: String) {
        let elapsed = self.elapsed_ms();
        if let Some(span) = self.spans.get_mut(span_id) {
            span.error = Some(error);
            span.duration_ms = Some(elapsed - span.start_time_ms);
            span.status = SpanStatus::Failed;
        }
    }

    /// 记录专家切换
    pub fn record_expert(&mut self, expert_id: &str) {
        self.expert_id = Some(expert_id.to_string());
        let span_id = self.create_span(SpanKind::ExpertSwitch, format!("switch_to_{}", expert_id));
        self.complete_span(&span_id, serde_json::json!({ "expert_id": expert_id }));
    }

    /// 记录 Skill 匹配
    pub fn record_skill_match(&mut self, skill_name: &str, confidence: f32) {
        self.matched_skill = Some(skill_name.to_string());
        let span_id = self.create_span(SpanKind::SkillMatch, "skill_match".to_string());
        self.complete_span(
            &span_id,
            serde_json::json!({
                "skill": skill_name,
                "confidence": confidence,
            }),
        );
    }

    /// 记录工具调用
    pub fn record_tool_call(
        &mut self,
        tool_name: &str,
        input: &serde_json::Value,
        output: &serde_json::Value,
    ) {
        self.tools_used.push(tool_name.to_string());
        let span_id = self.create_span(SpanKind::ToolCall, format!("tool_{}", tool_name));
        self.complete_span(
            &span_id,
            serde_json::json!({
                "tool": tool_name,
                "input": input,
                "output": output,
            }),
        );
    }

    /// 记录 LLM 调用
    pub fn record_llm_call(&mut self, prompt_tokens: u64, completion_tokens: u64) {
        let span_id = self.create_span(SpanKind::LlmCall, "llm_call".to_string());

        self.token_usage = Some(TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        });

        self.complete_span(
            &span_id,
            serde_json::json!({
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
            }),
        );
    }

    /// 完成整个 Trace
    pub fn complete(&mut self, output: String) {
        self.output = Some(output.clone());
        self.ended_at = Some(chrono::Utc::now());
        self.total_duration_ms = Some(self.elapsed_ms());
        self.status = TraceStatus::Success;

        // 完成根 Span
        if let Some(root_id) = &self.root_span_id {
            if let Some(root_span) = self.spans.get_mut(root_id) {
                root_span.duration_ms = Some(self.total_duration_ms.unwrap_or(0));
                root_span.status = SpanStatus::Success;
                root_span.output = Some(serde_json::json!({ "response": output }));
            }
        }
    }

    /// 计算从开始到现在的时间（毫秒）
    fn elapsed_ms(&self) -> u64 {
        (chrono::Utc::now() - self.started_at)
            .num_milliseconds()
            .max(0) as u64
    }

    /// 获取 Span 树（按层级结构）
    pub fn get_span_tree(&self) -> Vec<SpanTreeNode> {
        let root_id = match &self.root_span_id {
            Some(id) => id,
            None => return Vec::new(),
        };

        self.build_tree(root_id)
    }

    /// 构建树结构
    fn build_tree(&self, span_id: &str) -> Vec<SpanTreeNode> {
        let span = match self.spans.get(span_id) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let children = span
            .children
            .iter()
            .flat_map(|child_id| self.build_tree(child_id))
            .collect();

        vec![SpanTreeNode {
            span: span.clone(),
            children,
        }]
    }

    /// 获取执行摘要
    pub fn summary(&self) -> TraceSummary {
        TraceSummary {
            trace_id: self.id.0.clone(),
            input: self.input.clone(),
            output: self.output.clone(),
            duration_ms: self.total_duration_ms,
            expert_id: self.expert_id.clone(),
            matched_skill: self.matched_skill.clone(),
            tools_used: self.tools_used.clone(),
            token_usage: self.token_usage.clone(),
            span_count: self.spans.len(),
            status: self.status,
        }
    }
}

/// Span 树节点（用于可视化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanTreeNode {
    pub span: Span,
    pub children: Vec<SpanTreeNode>,
}

/// Trace 执行摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    pub input: String,
    pub output: Option<String>,
    pub duration_ms: Option<u64>,
    pub expert_id: Option<String>,
    pub matched_skill: Option<String>,
    pub tools_used: Vec<String>,
    pub token_usage: Option<TokenUsage>,
    pub span_count: usize,
    pub status: TraceStatus,
}

/// Trace 存储管理器
pub struct TraceStore {
    traces: HashMap<String, Trace>,
    max_traces: usize,
    /// 持久化目录（None 表示不持久化）
    persist_dir: Option<String>,
}

impl TraceStore {
    pub fn new(max_traces: usize) -> Self {
        Self {
            traces: HashMap::new(),
            max_traces,
            persist_dir: None,
        }
    }

    /// 创建带持久化的存储
    pub fn with_persistence(max_traces: usize, dir: &str) -> Self {
        // 确保目录存在
        let _ = std::fs::create_dir_all(dir);
        let mut store = Self {
            traces: HashMap::new(),
            max_traces,
            persist_dir: Some(dir.to_string()),
        };
        store.load_from_disk();
        store
    }

    /// 存储 Trace
    pub fn store(&mut self, trace: Trace) {
        // 如果超过限制，删除最旧的 Trace
        if self.traces.len() >= self.max_traces {
            let oldest_id = self
                .traces
                .iter()
                .min_by_key(|(_, t)| t.started_at)
                .map(|(id, _)| id.clone());

            if let Some(id) = oldest_id {
                self.traces.remove(&id);
            }
        }

        // 持久化到磁盘
        if let Some(ref dir) = self.persist_dir {
            let _ = Self::save_trace_to_file(dir, &trace);
        }

        self.traces.insert(trace.id.0.clone(), trace);
    }

    /// 获取 Trace
    pub fn get(&self, trace_id: &str) -> Option<&Trace> {
        self.traces.get(trace_id)
    }

    /// 获取 Trace 列表
    pub fn list(&self) -> Vec<&Trace> {
        self.traces.values().collect()
    }

    /// 获取 Trace 摘要列表
    pub fn list_summaries(&self) -> Vec<TraceSummary> {
        self.traces.values().map(|t| t.summary()).collect()
    }

    /// 按时间范围过滤
    pub fn filter_by_time(
        &self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    ) -> Vec<&Trace> {
        self.traces
            .values()
            .filter(|t| t.started_at >= start && t.started_at <= end)
            .collect()
    }

    /// 按用户 ID 过滤
    pub fn filter_by_user(&self, user_id: &str) -> Vec<&Trace> {
        self.traces
            .values()
            .filter(|t| t.user_id == user_id)
            .collect()
    }

    // ── 持久化辅助方法 ──

    /// 将单个 Trace 保存到文件
    fn save_trace_to_file(dir: &str, trace: &Trace) -> Result<(), String> {
        let filename = format!("{}/trace_{}.json", dir, trace.id.0);
        let json = serde_json::to_string_pretty(trace).map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(&filename, json).map_err(|e| format!("写入文件失败: {}", e))?;
        Ok(())
    }

    /// 从磁盘加载所有 Trace
    fn load_from_disk(&mut self) {
        let dir = match &self.persist_dir {
            Some(d) => d.clone(),
            None => return,
        };

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        let mut loaded = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(trace) = serde_json::from_str::<Trace>(&content) {
                    if self.traces.len() < self.max_traces {
                        self.traces.insert(trace.id.0.clone(), trace);
                        loaded += 1;
                    }
                }
            }
        }

        if loaded > 0 {
            tracing::info!("TraceStore: Loaded {} traces from {}", loaded, dir);
        }
    }

    /// 获取磁盘上的 Trace 文件数量
    pub fn file_count(&self) -> usize {
        match &self.persist_dir {
            Some(dir) => std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .filter(|e| {
                            e.as_ref()
                                .ok()
                                .and_then(|e| e.path().extension().map(|ext| ext == "json"))
                                .unwrap_or(false)
                        })
                        .count()
                })
                .unwrap_or(0),
            None => 0,
        }
    }
}

impl Default for TraceStore {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Trace 观察器（全局共享）
pub struct TraceObserver {
    store: Arc<std::sync::Mutex<TraceStore>>,
}

impl TraceObserver {
    /// 创建内存存储的 Observer（重启丢失数据）
    pub fn new() -> Self {
        Self {
            store: Arc::new(std::sync::Mutex::new(TraceStore::default())),
        }
    }

    /// 创建带文件持久化的 Observer
    ///
    /// Trace 会保存到 `dir` 目录下的 JSON 文件中，
    /// 重启后自动加载历史 Trace。
    ///
    /// ```rust,ignore
    /// let observer = TraceObserver::with_persistence("./traces");
    /// observer.store_trace(trace); // 自动保存到文件
    /// ```
    pub fn with_persistence(dir: &str) -> Self {
        Self {
            store: Arc::new(std::sync::Mutex::new(TraceStore::with_persistence(
                1000, dir,
            ))),
        }
    }

    /// 创建新 Trace
    pub fn create_trace(&self, user_id: &str, session_id: &str, input: &str) -> Trace {
        Trace::new(user_id, session_id, input)
    }

    /// 存储 Trace
    pub fn store_trace(&self, trace: Trace) {
        self.store.lock().unwrap().store(trace);
    }

    /// 获取 Trace
    pub fn get_trace(&self, trace_id: &str) -> Option<Trace> {
        self.store.lock().unwrap().get(trace_id).cloned()
    }

    /// 获取 Trace 摘要列表
    pub fn list_summaries(&self) -> Vec<TraceSummary> {
        self.store.lock().unwrap().list_summaries()
    }

    /// 获取 Span 树
    pub fn get_span_tree(&self, trace_id: &str) -> Option<Vec<SpanTreeNode>> {
        self.store
            .lock()
            .unwrap()
            .get(trace_id)
            .map(|t| t.get_span_tree())
    }
}

impl Default for TraceObserver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_creation() {
        let trace = Trace::new("user1", "session1", "测试输入");
        assert!(!trace.id.0.is_empty());
        assert_eq!(trace.user_id, "user1");
        assert_eq!(trace.status, TraceStatus::Running);
        assert!(trace.root_span_id.is_some());
    }

    #[test]
    fn test_span_creation() {
        let mut trace = Trace::new("user1", "session1", "测试");

        // 创建 Skill 匹配 Span
        let span_id = trace.create_span(SpanKind::SkillMatch, "skill_match".to_string());
        assert!(trace.spans.contains_key(&span_id));

        // 完成 Span
        trace.complete_span(&span_id, serde_json::json!({ "skill": "test" }));
        let span = trace.spans.get(&span_id).unwrap();
        assert_eq!(span.status, SpanStatus::Success);
        assert!(span.duration_ms.is_some());
    }

    #[test]
    fn test_trace_completion() {
        let mut trace = Trace::new("user1", "session1", "测试");
        trace.record_skill_match("test_skill", 0.9);
        trace.record_llm_call(100, 50);
        trace.complete("测试输出".to_string());

        assert_eq!(trace.status, TraceStatus::Success);
        assert!(trace.output.is_some());
        assert!(trace.total_duration_ms.is_some());
        assert!(trace.token_usage.is_some());
    }

    #[test]
    fn test_span_tree() {
        let mut trace = Trace::new("user1", "session1", "测试");

        let parent_id = trace.create_span(SpanKind::SkillExecute, "skill".to_string());
        let child_id = trace.create_child_span(&parent_id, SpanKind::ToolCall, "tool".to_string());

        trace.complete_span(&child_id, serde_json::json!({}));
        trace.complete_span(&parent_id, serde_json::json!({}));

        let tree = trace.get_span_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_trace_store() {
        let mut store = TraceStore::new(10);
        let trace = Trace::new("user1", "session1", "测试");
        let id = trace.id.0.clone();

        store.store(trace);
        assert!(store.get(&id).is_some());

        // 测试最大限制
        for i in 0..15 {
            let t = Trace::new(&format!("user{}", i), "session", "test");
            store.store(t);
        }
        assert!(store.traces.len() <= 10);
    }
}
