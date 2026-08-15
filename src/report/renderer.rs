//! # 火焰图报表渲染器
//!
//! 将 `SpanData` 列表渲染为火焰图（flame graph）样式的 HTML 报表。
//!
//! ## 火焰图特点
//! - 横向：时间轴，每个 span 的宽度 = 耗时占比
//! - 纵向：调用栈深度，子 span 堆叠在父 span 下方
//! - 颜色：成功绿色、失败红色、进行中蓝色，按 span_type 分色调
//! - 悬停显示输入/输出/元数据
//!
//! ## 数据来源
//! `SpanData` 由框架 EventBus 事件转换而来（见 TraceEventBridge）。
//! 本渲染器只做纯函数转换：span 列表 → HTML 字符串。

use crate::application::SpanData;

/// 转义 HTML 特殊字符，防止注入
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Span 树节点（带计算后的布局信息）
#[derive(Clone)]
struct FlameNode {
    /// 原始 span 索引
    idx: usize,
    /// 子节点索引（指向 nodes 数组的下标，即 span idx）
    children: Vec<usize>,
    /// 在整体时间轴上的起始偏移（毫秒）
    start_ms: u64,
    /// 持续时间（毫秒）
    duration_ms: u64,
    /// 栈深度（root = 0）
    depth: usize,
}

/// 渲染完整火焰图 HTML 报表
pub fn render_html(trace_id: &str, spans: &[SpanData], summary: &serde_json::Value) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Trace 火焰图 - {trace_id}</title>
<style>
:root {{ --bg:#0f1117; --panel:#1a1e2a; --border:#2a2f3f; --text:#e6e9f0;
  --muted:#8b93a7; --ok:#4ade80; --fail:#f87171; --accent:#60a5fa; --warn:#fbbf24; }}
* {{ box-sizing:border-box; }}
body {{ margin:0; font-family:-apple-system,'Segoe UI','PingFang SC','Microsoft YaHei',sans-serif;
  background:var(--bg); color:var(--text); line-height:1.5; }}
.wrap {{ max-width:1200px; margin:0 auto; padding:20px; }}
h1 {{ font-size:20px; margin:0 0 2px; }}
h2 {{ font-size:13px; color:var(--muted); font-weight:500; margin:0 0 16px; }}
.code {{ background:#0b0d14; padding:2px 6px; border-radius:4px; color:#93c5fd;
  font-family:ui-monospace,monospace; font-size:12px; }}

/* 顶部摘要 */
.meta {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:10px;
  background:var(--panel); border:1px solid var(--border); border-radius:8px; padding:14px; margin-bottom:20px; }}
.meta div label {{ display:block; font-size:10px; color:var(--muted); text-transform:uppercase; letter-spacing:.5px; }}
.meta div span {{ font-size:13px; }}
.badge {{ display:inline-block; padding:2px 8px; border-radius:12px; font-size:11px; font-weight:600; }}
.badge.ok {{ background:rgba(74,222,128,.15); color:var(--ok); }}
.badge.fail {{ background:rgba(248,113,113,.15); color:var(--fail); }}
.badge.progress {{ background:rgba(96,165,250,.15); color:var(--accent); }}

/* 火焰图主体 */
.flame-wrap {{ background:var(--panel); border:1px solid var(--border); border-radius:8px; padding:16px; margin-bottom:20px; }}
.flame-title {{ font-size:13px; color:var(--muted); margin-bottom:12px; display:flex; justify-content:space-between; }}
.flame {{ position:relative; font-family:ui-monospace,'SF Mono',Menlo,monospace; font-size:12px;
  overflow-x:auto; padding:4px 0; }}
.flame-row {{ position:relative; height:28px; margin-bottom:2px; white-space:nowrap; }}
.bar {{ position:absolute; height:26px; border-radius:3px; overflow:hidden; cursor:pointer;
  display:flex; align-items:center; padding:0 8px; color:#0b0d14; font-weight:600;
  white-space:nowrap; text-overflow:ellipsis; transition:opacity .15s, transform .15s;
  border:1px solid rgba(0,0,0,.2); }}
.bar:hover {{ opacity:.85; transform:translateY(-1px); z-index:10;
  box-shadow:0 4px 12px rgba(0,0,0,.4); }}
.bar .label {{ overflow:hidden; text-overflow:ellipsis; }}
/* 颜色：按 span_type 分组 */
.bar.user_message {{ background:#60a5fa; color:#fff; }}
.bar.chain_selected {{ background:#a78bfa; color:#fff; }}
.bar.agent_started {{ background:#fbbf24; }}
.bar.agent_completed {{ background:#4ade80; }}
.bar.agent_failed {{ background:#f87171; color:#fff; }}
.bar.graph_started, .bar.graph_completed {{ background:#34d399; }}
.bar.node_execute_requested, .bar.node_completed, .bar.node_failed {{ background:#f59e0b; }}
.bar.flow_started, .bar.flow_completed, .bar.flow_step_executed {{ background:#22d3ee; }}
.bar.llm_call, .bar.llm_response {{ background:#c084fc; color:#fff; }}
.bar.tool_call_started, .bar.tool_call_completed {{ background:#fb923c; }}
.bar.trace {{ background:#64748b; color:#fff; }}
/* 架构层 span（六边形调用链） */
.bar.http_request {{ background:#3b82f6; color:#fff; }}
.bar.inbound_adapter {{ background:#2563eb; color:#fff; }}
.bar.app_service {{ background:#7c3aed; color:#fff; }}
.bar.outbound_adapter {{ background:#9333ea; color:#fff; }}
.bar.framework_dispatch {{ background:#0891b2; color:#fff; }}
.bar.rule_engine {{ background:#0e7490; color:#fff; }}
.bar.domain_expert {{ background:#16a34a; color:#fff; }}
.bar.llm_chat {{ background:#c084fc; color:#fff; }}
.bar.toolchain_check {{ background:#ea580c; color:#fff; }}
.bar.default {{ background:#94a3b8; }}

/* 时间标尺 */
.ruler {{ position:relative; height:20px; margin-bottom:8px; border-bottom:1px solid var(--border); }}
.ruler .tick {{ position:absolute; top:0; font-size:10px; color:var(--muted); transform:translateX(-50%); }}
.ruler .tick::after {{ content:''; position:absolute; top:14px; left:50%; width:1px; height:6px;
  background:var(--border); transform:translateX(-50%); }}

/* 详情面板 */
.detail {{ background:var(--panel); border:1px solid var(--border); border-radius:8px; padding:16px;
  min-height:80px; }}
.detail .title {{ font-size:14px; font-weight:600; margin-bottom:8px; }}
.detail .row {{ display:grid; grid-template-columns:80px 1fr; gap:8px; margin:4px 0; font-size:12px; }}
.detail .row .k {{ color:var(--muted); }}
.detail .row .v {{ word-break:break-word; white-space:pre-wrap; font-family:ui-monospace,monospace; }}
.detail .hint {{ color:var(--muted); font-size:12px; font-style:italic; }}

/* 图例 */
.legend {{ display:flex; gap:12px; flex-wrap:wrap; margin-top:12px; font-size:11px; color:var(--muted); }}
.legend .item {{ display:flex; align-items:center; gap:4px; }}
.legend .swatch {{ width:12px; height:12px; border-radius:2px; }}
</style>
</head>
<body>
<div class="wrap">
  <h1>Trace 火焰图</h1>
  <h2>trace_id: <span class="code">{trace_id}</span></h2>
  {meta_html}
  <div class="flame-wrap">
    <div class="flame-title">
      <span>执行调用栈（横向=耗时，纵向=调用深度）</span>
      <span id="total-dur">{total_dur}</span>
    </div>
    <div class="ruler">{ruler}</div>
    <div class="flame" id="flame">{flame}</div>
    <div class="legend">
      <div class="item"><div class="swatch" style="background:#60a5fa"></div>用户消息</div>
      <div class="item"><div class="swatch" style="background:#a78bfa"></div>链路选择</div>
      <div class="item"><div class="swatch" style="background:#fbbf24"></div>专家启动</div>
      <div class="item"><div class="swatch" style="background:#4ade80"></div>专家完成</div>
      <div class="item"><div class="swatch" style="background:#f87171"></div>失败</div>
      <div class="item"><div class="swatch" style="background:#34d399"></div>图编排</div>
      <div class="item"><div class="swatch" style="background:#22d3ee"></div>Flow</div>
      <div class="item"><div class="swatch" style="background:#c084fc"></div>LLM</div>
      <div class="item"><div class="swatch" style="background:#fb923c"></div>工具调用</div>
    </div>
  </div>
  <div class="detail" id="detail">
    <div class="hint">点击火焰图任意色块查看详情</div>
  </div>
  <script>
  const spans = {spans_json};
  function showDetail(i) {{
    const s = spans[i];
    if (!s) return;
    const d = document.getElementById('detail');
    let html = '<div class="title">' + escHtml(s.name) + ' <span style="color:var(--muted);font-size:11px">(' + s.span_type + ')</span></div>';
    html += row('耗时', (s.duration_ms || 0) + ' ms');
    html += row('状态', s.success === null ? '进行中' : (s.success ? '成功' : '失败'));
    if (s.input) html += row('输入', escHtml(s.input));
    if (s.output) html += row('输出', escHtml(s.output));
    if (s.tokens !== null) html += row('Tokens', s.tokens);
    if (s.extra && Object.keys(s.extra).length) {{
      html += row('元数据', Object.entries(s.extra).map(([k,v]) => k + ': ' + v).join('\\n'));
    }}
    html += row('时间戳', s.timestamp);
    d.innerHTML = html;
  }}
  function row(k, v) {{ return '<div class="row"><div class="k">' + k + '</div><div class="v">' + v + '</div></div>'; }}
  function escHtml(s) {{ return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }}
  </script>
</div>
</body>
</html>"#,
        trace_id = esc(trace_id),
        meta_html = render_meta(summary),
        total_dur = format_total_duration(spans, summary),
        ruler = render_ruler(spans),
        flame = render_flame(spans),
        spans_json = render_spans_json(spans),
    )
}

/// 格式化总耗时
fn format_total_duration(spans: &[SpanData], summary: &serde_json::Value) -> String {
    if let Some(d) = summary.get("total_duration_ms").and_then(|v| v.as_u64()) {
        return format!("{} ms", d);
    }
    // 兜底：用最后一个 span 的 end 时间
    let max_end = spans
        .iter()
        .filter_map(|s| s.duration_ms)
        .max()
        .unwrap_or(0);
    format!("{} ms", max_end)
}

/// 渲染顶部摘要卡片
fn render_meta(summary: &serde_json::Value) -> String {
    let get = |key: &str| -> String {
        match summary.get(key) {
            Some(serde_json::Value::String(s)) => esc(s),
            Some(serde_json::Value::Array(arr)) => {
                let parts: Vec<String> = arr
                    .iter()
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect();
                esc(&parts.join(", "))
            }
            Some(serde_json::Value::Null) | None => "-".into(),
            Some(other) => esc(&other.to_string()),
        }
    };

    let status = get("status");
    let badge = match status.as_str() {
        "Success" => "ok",
        "Failed" => "fail",
        _ => "progress",
    };

    let duration = get("total_duration_ms");
    let duration = if duration == "-" {
        "-".into()
    } else {
        format!("{} ms", duration)
    };

    format!(
        r#"<div class="meta">
  <div><label>用户</label><span>{user}</span></div>
  <div><label>会话</label><span>{session}</span></div>
  <div><label>状态</label><span class="badge {badge}">{status}</span></div>
  <div><label>总耗时</label><span>{duration}</span></div>
  <div><label>策略链</label><span>{chain}</span></div>
  <div><label>专家链</label><span>{experts}</span></div>
</div>"#,
        user = get("user_id"),
        session = get("session_id"),
        badge = badge,
        status = status,
        duration = duration,
        chain = get("chain_name"),
        experts = get("expert_chain"),
    )
}

/// 构建火焰图所需的 span 树
///
/// 嵌套规则（复用 observer_adapters.rs 语义）：
/// - user_message → root
/// - chain_selected / agent_matched → root child，记为 chain_idx
/// - agent_started / agent_completed / agent_failed → chain_idx child
/// - graph_started → chain_idx child，记为 graph_idx
/// - node_* → graph_idx child
/// - graph_completed → graph_idx child，然后 graph_idx = None
/// - flow_started → chain_idx child，记为 flow_idx
/// - flow_step_executed → flow_idx child
/// - flow_completed → flow_idx child，然后 flow_idx = None
fn build_flame_tree(spans: &[SpanData]) -> (Vec<FlameNode>, u64) {
    if spans.is_empty() {
        return (vec![], 0);
    }

    // 排序后的 span 索引
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| spans[i].timestamp);

    // 计算每个 span 的 start_ms（相对于第一个 span）
    let base_ts = spans[order[0]].timestamp;
    let mut start_ms: Vec<u64> = vec![0; spans.len()];
    for &i in &order {
        let dt = spans[i].timestamp - base_ts;
        start_ms[i] = dt.num_milliseconds().max(0) as u64;
    }

    // 每个 span 的持续时间：有就用，没有就估算（到下一个同层 span 的起点）
    let mut duration: Vec<u64> = spans.iter().map(|s| s.duration_ms.unwrap_or(0)).collect();
    // 对没有 duration 的 span，用其后继同层 span 的 start 兜底
    for &i in &order {
        if duration[i] == 0 {
            // 找下一个时间更晚的 span 作为结束估计
            if let Some(&next) = order.iter().find(|&&j| start_ms[j] > start_ms[i]) {
                duration[i] = start_ms[next].saturating_sub(start_ms[i]).max(1);
            } else {
                duration[i] = 1; // 最小 1ms
            }
        }
    }

    // 总时间跨度
    let total_ms = start_ms
        .iter()
        .zip(duration.iter())
        .map(|(&s, &d)| s + d)
        .max()
        .unwrap_or(1)
        .max(1);

    // 构建树
    let mut root_idx: Option<usize> = None;
    let mut chain_idx: Option<usize> = None;
    let mut graph_idx: Option<usize> = None;
    let mut flow_idx: Option<usize> = None;
    // 暂存所有节点用于引用
    let mut nodes: Vec<FlameNode> = (0..spans.len())
        .map(|i| FlameNode {
            idx: i,
            children: vec![],
            start_ms: start_ms[i],
            duration_ms: duration[i],
            depth: 0,
        })
        .collect();

    // 父子关系映射：parent[i] = Some(parent_idx_in_nodes)
    let mut parent: Vec<Option<usize>> = vec![None; spans.len()];

    for &i in &order {
        match spans[i].span_type.as_str() {
            "user_message" => {
                parent[i] = None;
                root_idx = Some(i);
            }
            "chain_selected" | "agent_matched" => {
                parent[i] = root_idx;
                chain_idx = Some(i);
            }
            "agent_started" | "agent_completed" | "agent_failed" => {
                parent[i] = chain_idx.or(root_idx);
            }
            "graph_started" => {
                parent[i] = chain_idx.or(root_idx);
                graph_idx = Some(i);
            }
            "node_execute_requested" | "node_completed" | "node_failed" => {
                parent[i] = graph_idx.or(chain_idx).or(root_idx);
            }
            "graph_completed" => {
                parent[i] = graph_idx.or(chain_idx).or(root_idx);
                graph_idx = None;
            }
            "flow_started" => {
                parent[i] = chain_idx.or(root_idx);
                flow_idx = Some(i);
            }
            "flow_step_executed" => {
                parent[i] = flow_idx.or(chain_idx).or(root_idx);
            }
            "flow_completed" => {
                parent[i] = flow_idx.or(chain_idx).or(root_idx);
                flow_idx = None;
            }
            "llm_call" | "llm_response" => {
                parent[i] = chain_idx.or(root_idx);
            }
            "tool_call_started" | "tool_call_completed" => {
                parent[i] = chain_idx.or(root_idx);
            }
            _ => {
                parent[i] = graph_idx.or(flow_idx).or(chain_idx).or(root_idx);
            }
        }
    }

    // 组装子节点：nodes[i].children 存子节点索引
    let mut roots: Vec<usize> = vec![];
    for i in 0..spans.len() {
        if let Some(p) = parent[i] {
            nodes[p].children.push(i);
        } else {
            roots.push(i);
        }
    }

    // 递归设置 depth
    fn set_depths(nodes: &mut [FlameNode], roots: &[usize], depth: usize) {
        for &r in roots {
            nodes[r].depth = depth;
            let kids = nodes[r].children.clone();
            set_depths(nodes, &kids, depth + 1);
        }
    }
    set_depths(&mut nodes, &roots, 0);

    // 扁平化收集（父在前，子在后），渲染时按 depth 分行
    fn collect(nodes: &[FlameNode], roots: &[usize], out: &mut Vec<FlameNode>) {
        for &r in roots {
            let node = FlameNode {
                idx: nodes[r].idx,
                children: vec![],
                start_ms: nodes[r].start_ms,
                duration_ms: nodes[r].duration_ms,
                depth: nodes[r].depth,
            };
            out.push(node);
            let kids = nodes[r].children.clone();
            collect(nodes, &kids, out);
        }
    }
    let mut flat_nodes: Vec<FlameNode> = vec![];
    collect(&nodes, &roots, &mut flat_nodes);

    (flat_nodes, total_ms)
}

/// 渲染时间标尺
fn render_ruler(spans: &[SpanData]) -> String {
    if spans.is_empty() {
        return String::new();
    }
    let (_, total_ms) = build_flame_tree(spans);
    let total = total_ms.max(1);

    // 5 个刻度
    let mut html = String::new();
    for i in 0..=5 {
        let pct = i as f64 * 100.0 / 5.0;
        let ms = total * i / 5;
        html.push_str(&format!(
            r#"<div class="tick" style="left:{}%">{}</div>"#,
            pct, ms
        ));
    }
    html
}

/// 渲染火焰图主体
fn render_flame(spans: &[SpanData]) -> String {
    if spans.is_empty() {
        return r#"<div style="color:var(--muted);padding:20px;text-align:center">无 span 数据</div>"#
            .to_string();
    }

    let (flat_nodes, total_ms) = build_flame_tree(spans);
    let total = total_ms.max(1) as f64;

    // 按 depth 分组
    use std::collections::BTreeMap;
    let mut by_depth: BTreeMap<usize, Vec<&FlameNode>> = BTreeMap::new();
    for n in &flat_nodes {
        by_depth.entry(n.depth).or_default().push(n);
    }

    let mut html = String::new();
    for (_depth, nodes) in &by_depth {
        html.push_str(r#"<div class="flame-row">"#);
        for n in nodes {
            let span = &spans[n.idx];
            let left = (n.start_ms as f64 / total * 100.0).max(0.0);
            let width = (n.duration_ms as f64 / total * 100.0).max(0.5); // 最小 0.5% 可见
            let cls = span_type_class(&span.span_type);
            let label = if width > 8.0 {
                // 宽度够，显示名称+耗时
                format!("{} ({}ms)", span.name, n.duration_ms)
            } else if width > 3.0 {
                span.name.clone()
            } else {
                String::new()
            };
            html.push_str(&format!(
                r#"<div class="bar {cls}" style="left:{left:.2}%;width:{width:.2}%"
                     onclick="showDetail({idx})" title="{title}">
                  <span class="label">{label}</span>
                </div>"#,
                cls = cls,
                left = left,
                width = width,
                idx = n.idx,
                title = esc(&format!("{} | {}ms", span.name, n.duration_ms)),
                label = esc(&label),
            ));
        }
        html.push_str("</div>");
    }
    html
}

/// span_type → CSS 类名
fn span_type_class(span_type: &str) -> &'static str {
    match span_type {
        "user_message" => "user_message",
        "chain_selected" | "agent_matched" => "chain_selected",
        "agent_started" => "agent_started",
        "agent_completed" => "agent_completed",
        "agent_failed" => "agent_failed",
        "graph_started" | "graph_completed" => "graph_started",
        "node_execute_requested" | "node_completed" | "node_failed" => "node_completed",
        "flow_started" | "flow_completed" | "flow_step_executed" => "flow_started",
        "llm_call" | "llm_response" => "llm_call",
        "tool_call_started" | "tool_call_completed" => "tool_call_started",
        "trace" => "trace",
        _ => "default",
    }
}

/// 把 spans 序列化为 JSON，供 JS 详情面板使用
fn render_spans_json(spans: &[SpanData]) -> String {
    let arr: Vec<serde_json::Value> = spans
        .iter()
        .map(|s| {
            serde_json::json!({
                "span_type": s.span_type,
                "name": s.name,
                "input": s.input,
                "output": s.output,
                "duration_ms": s.duration_ms,
                "tokens": s.tokens,
                "timestamp": s.timestamp.to_rfc3339(),
                "success": s.success,
                "extra": s.extra,
            })
        })
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}
