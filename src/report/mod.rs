//! # 报表生成器
//!
//! 将一次 HTTP 请求的 trace/span 数据渲染为 HTML 报表，
//! 按时间格式命名写入 `report/` 目录。
//!
//! 设计说明：
//! - 纯函数式：`render_html()` / `build_filename()` 不依赖框架，便于测试
//! - 数据来源：`SpanData`（由框架 EventBus 事件转换而来，见 application::observer）
//! - 每次 HTTP 请求完成后调用 `generate()` 落盘一个 HTML 报表
//!
//! ## 文件命名格式
//! `trace_2026-08-12_153045_123456.html`
//! （时间戳精确到毫秒，避免同一秒多次请求覆盖）

pub mod renderer;

use std::path::PathBuf;

use crate::application::SpanData;
use renderer::render_html;

/// 报表输出目录名
pub const REPORT_DIR: &str = "report";

/// 生成一份 HTML 报表并写入 report 目录
///
/// # 参数
/// - `report_dir`: 输出目录（默认可传 `REPORT_DIR`）
/// - `trace_id`:   追踪 ID（用于标题、内容展示）
/// - `spans`:      本次请求的细粒度 span 列表
/// - `summary`:    附加摘要信息（用户、会话、输入、输出、状态、总耗时等）
///
/// # 返回
/// 写入的完整文件路径
pub fn generate(
    report_dir: &str,
    trace_id: &str,
    spans: &[SpanData],
    summary: &serde_json::Value,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(report_dir)?;

    let html = render_html(trace_id, spans, summary);
    let filename = build_filename(trace_id);
    let path = PathBuf::from(report_dir).join(filename);

    std::fs::write(&path, html)?;
    Ok(path)
}

/// 生成报表文件名：`trace_{YYYYMMDD_HHMMSS}_{millis}_{trace_id净化前缀}.html`
///
/// 文件名中的 trace_id 前缀做净化：只保留字母数字和下划线，且最长 8 个字符，
/// 避免多字节 UTF-8 字节切片 panic，也避免文件名含非法字符。
pub fn build_filename(trace_id: &str) -> String {
    let now = chrono::Local::now();
    let ts = now.format("%Y%m%d_%H%M%S").to_string();
    let millis = now.timestamp_subsec_millis();
    let short_id = sanitize_id_prefix(trace_id);
    format!("trace_{}_{}_{}.html", ts, millis, short_id)
}

/// 净化 trace_id 为安全的文件名前缀（字母数字下划线，最长 8 字符）
fn sanitize_id_prefix(trace_id: &str) -> String {
    trace_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_filename_format() {
        let name = build_filename("0123456789abcdef");
        // 形如 trace_20260812_153045_123_01234567.html
        assert!(name.starts_with("trace_"), "应以 trace_ 开头: {}", name);
        assert!(name.ends_with(".html"), "应以 .html 结尾: {}", name);
        // 时间部分：8位日期 + _ + 6位时间
        let mid = &name["trace_".len()..];
        let date_part = &mid[..8];
        assert!(
            date_part.chars().all(|c| c.is_ascii_digit()),
            "日期部分应为数字: {}",
            date_part
        );
    }
}
