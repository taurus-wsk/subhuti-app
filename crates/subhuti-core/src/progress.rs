//! # 框架级结构化进度事件
//!
//! 取代原先专家侧「手拼 JSON 字符串」+ 框架 `on_progress: FnMut(&str)` 字符串回调的
//! 非结构化进度方式。专家/编排层发出类型化的 [`ProgressEvent`]，应用层再把它映射为
//! 前端协议（如 `StreamEvent` / SSE），框架自身不感知任何展示协议。
//!
//! 这与 `AgentEventData`（框架内部生命周期事件，走 `EventBus`）是两条互补通道：
//! - `AgentEventData`：框架内部动作（专家匹配、工具调用、LLM 调用…），由
//!   `ProgressEventBridge` 挑选后转成 `ProgressEvent::Step` 汇入同一条进度流；
//! - `ProgressEvent`：统一的进度流，承载 step / chunk / ask 三类语义。

/// 框架级结构化进度事件（协议中立，不含展示细节）
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// 阶段步骤（统一承载 analyze/route/plan/edit/verify/think/tool/retrieve/run/done 等）
    Step {
        /// 人类可读的进度文案
        message: String,
        /// 事件来源：框架级步骤为 "框架"，专家级步骤为专家名
        source: String,
        /// 阶段标识（可选）：analyze/route/plan/edit/verify/tool/retrieve/think/run/done…
        phase: Option<String>,
        /// 待办清单快照（可选，plan 阶段），前端据此原位替换渲染打勾清单
        todo_state: Option<String>,
        /// 已完成步数（可选，用于 `(done/total)` 渲染）
        done: Option<usize>,
        /// 总步数（可选）
        total: Option<usize>,
    },
    /// 真流式文本分片（模型真实输出的增量）
    Chunk { content: String },
    /// 主动提问：专家在规划阶段信息不足时向用户发起单选提问
    Ask {
        ask_id: String,
        question: String,
        options: Vec<String>,
    },
}
