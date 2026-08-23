//! # 本地命令执行适配器
//!
//! 实现 `CommandPort` 端口，使用 tokio::process::Command 执行 shell 命令。
//! 工作目录由调用方指定（通常为 workspace_folder）。

use crate::domain::ports::{CommandOutput, CommandPort};

/// 本地命令执行适配器
///
/// 在指定工作目录中执行 shell 命令，返回 stdout、stderr 和退出码。
pub struct LocalCommandAdapter;

impl LocalCommandAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl CommandPort for LocalCommandAdapter {
    fn run_command(
        &self,
        command: &str,
        args: &[String],
        cwd: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<CommandOutput, String>> + Send>>
    {
        let command = command.to_string();
        let args = args.to_vec();
        let cwd = cwd.to_string();
        Box::pin(async move {
            let output = tokio::process::Command::new(&command)
                .args(&args)
                .current_dir(&cwd)
                .output()
                .await
                .map_err(|e| format!("执行命令 '{}' 失败: {}", command, e))?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            Ok(CommandOutput {
                stdout,
                stderr,
                exit_code,
            })
        })
    }
}
