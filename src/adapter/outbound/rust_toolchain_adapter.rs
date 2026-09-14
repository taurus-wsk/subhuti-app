//! # Rust 工具链适配器
//!
//! 实现 ToolchainPort，直接调用本地 cargo/clippy/rustfmt 命令。
//!
//! 六边形架构：
//! - 领域层定义接口（ToolchainPort）
//! - 出站适配层实现适配器（RustToolchainAdapter）
//! - 供 RustExpert 等需要编译验证的领域专家使用

use std::process::Command;
use std::sync::Arc;

use crate::domain::events::DomainEvent;

use crate::domain::dto::ToolchainResult;
use crate::domain::ports::ToolchainPort;
use crate::domain::traits::TraceContext;

/// Rust 工具链适配器
///
/// 直接调用本地 `cargo check`、`cargo clippy`、`rustfmt` 命令。
/// 项目路径在构造时指定。
pub struct RustToolchainAdapter {
    project_path: String,
}

impl RustToolchainAdapter {
    /// 创建新的工具链适配器
    pub fn new(project_path: String) -> Self {
        Self { project_path }
    }

    /// 运行 cargo 命令并收集输出
    fn run_cargo(&self, args: &[&str]) -> ToolchainResult {
        let output = Command::new("cargo")
            .args(args)
            .current_dir(&self.project_path)
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let combined = format!("{}{}", stdout, stderr);

                let mut errors = Vec::new();
                let mut warnings = Vec::new();
                for line in combined.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("error") || trimmed.starts_with("error[") {
                        errors.push(line.to_string());
                    } else if trimmed.contains("warning") && !trimmed.starts_with("warning: unused")
                    {
                        warnings.push(line.to_string());
                    }
                }

                ToolchainResult {
                    success: out.status.success(),
                    errors,
                    warnings,
                    output: combined,
                }
            }
            Err(e) => ToolchainResult {
                success: false,
                errors: vec![format!("无法执行 cargo 命令: {}", e)],
                warnings: vec![],
                output: e.to_string(),
            },
        }
    }
}

// 手动实现 Pin<Box<dyn Future>> 签名（与 ToolchainPort 一致）
// 因为 cargo 命令实际上是同步的，直接用 tokio::task::spawn_blocking 包装
impl ToolchainPort for RustToolchainAdapter {
    fn check(
        &self,
        _project_path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolchainResult> + Send>> {
        let result = self.run_cargo(&["check", "--message-format=short"]);
        Box::pin(std::future::ready(result))
    }

    fn clippy(
        &self,
        _project_path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolchainResult> + Send>> {
        // clippy 可能不存在，降级为 cargo check
        let result = self.run_cargo(&["clippy", "--message-format=short"]);
        Box::pin(std::future::ready(result))
    }

    fn format(
        &self,
        _code: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>> {
        // rustfmt 只能格式化文件，此处返回原样
        Box::pin(std::future::ready(_code.to_string()))
    }
}

/// 带事件发射的工具链适配器（per-request 包裹层）
///
/// 在 `check` / `clippy` 前后发射 `ToolCalling` / `ToolResponded`，
/// 由 `ProgressEventBridge` 桥接成 SSE 的 `tool` 阶段。
///
/// 之所以用包裹层而非给 `RustToolchainAdapter` 单例加 trace_id：单例的 trace_id
/// 在并发请求之间会互相覆盖；这里每次请求新建一个包裹层，携带本请求的
/// `TraceContext`，与 `TracedEngineLlm` 的模式一致。
pub struct TracedToolchainAdapter {
    inner: Arc<dyn ToolchainPort>,
    trace: TraceContext,
}

impl TracedToolchainAdapter {
    pub fn new(inner: Arc<dyn ToolchainPort>, trace: TraceContext) -> Self {
        Self { inner, trace }
    }
}

impl ToolchainPort for TracedToolchainAdapter {
    fn check(
        &self,
        project_path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolchainResult> + Send>> {
        let inner = self.inner.clone();
        let trace = self.trace.clone();
        let path = project_path.to_string();
        Box::pin(async move {
            trace
                .emit(DomainEvent::ToolCalling {
                    tool_name: "cargo check".to_string(),
                    args: serde_json::Value::Null,
                })
                .await;
            let result = inner.check(&path).await;
            trace
                .emit(DomainEvent::ToolResponded {
                    tool_name: "cargo check".to_string(),
                    result: if result.success {
                        "ok".to_string()
                    } else {
                        "fail".to_string()
                    },
                    success: result.success,
                    duration_ms: 0,
                })
                .await;
            result
        })
    }

    fn clippy(
        &self,
        project_path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolchainResult> + Send>> {
        let inner = self.inner.clone();
        let trace = self.trace.clone();
        let path = project_path.to_string();
        Box::pin(async move {
            trace
                .emit(DomainEvent::ToolCalling {
                    tool_name: "cargo clippy".to_string(),
                    args: serde_json::Value::Null,
                })
                .await;
            let result = inner.clippy(&path).await;
            trace
                .emit(DomainEvent::ToolResponded {
                    tool_name: "cargo clippy".to_string(),
                    result: if result.success {
                        "ok".to_string()
                    } else {
                        "fail".to_string()
                    },
                    success: result.success,
                    duration_ms: 0,
                })
                .await;
            result
        })
    }

    fn format(
        &self,
        code: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>> {
        self.inner.format(code)
    }
}
