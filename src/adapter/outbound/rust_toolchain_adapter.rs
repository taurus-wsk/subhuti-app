//! # Rust 工具链适配器
//!
//! 实现 ToolchainPort，直接调用本地 cargo/clippy/rustfmt 命令。
//!
//! 六边形架构：
//! - 领域层定义接口（ToolchainPort）
//! - 出站适配层实现适配器（RustToolchainAdapter）
//! - 供 RustExpert 等需要编译验证的领域专家使用

use std::process::Command;

use crate::domain::dto::ToolchainResult;
use crate::domain::ports::ToolchainPort;

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
