//! # Extension Layer - 扩展层
//!
//! 职责：不侵入内核，所有附加能力插拔
//!
//! ## 生命周期 Hook
//!
//! - **before_prompt**: 上下文预处理、裁剪
//! - **before_tool**: 工具校验、日志
//! - **after_tool**: 结果摘要、缓存
//! - **after_complete**: 自动归档记忆、总结
//!
//! ## 可扩展能力
//!
//! - 日志
//! - 缓存
//! - 记忆压缩
//! - 敏感词过滤
//! - 统计 token
//! - 自定义拦截器

use crate::runtime::Session;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Hook 生命周期
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPhase {
    /// before_prompt - 上下文预处理
    BeforePrompt,
    /// before_tool - 工具校验
    BeforeTool,
    /// after_tool - 结果处理
    AfterTool,
    /// after_complete - 完成处理
    AfterComplete,
}

/// Hook trait - 生命周期钩子
#[async_trait]
pub trait Hook: Send + Sync {
    /// 获取钩子名称
    fn name(&self) -> &str;

    /// 获取钩子阶段
    fn phase(&self) -> HookPhase;

    /// 执行钩子
    async fn execute(&self, session: &mut Session, context: &HookContext) -> Result<()>;
}

/// Hook 上下文
#[derive(Debug, Clone)]
pub struct HookContext {
    /// 原始输入
    pub input: String,
    /// 工具名称 (before_tool/after_tool 时)
    pub tool_name: Option<String>,
    /// 工具参数
    pub tool_args: Option<serde_json::Value>,
    /// 工具结果
    pub tool_result: Option<String>,
    /// 额外数据
    pub extra: HashMap<String, String>,
}

impl HookContext {
    /// 创建新的上下文
    pub fn new(input: &str) -> Self {
        Self {
            input: input.to_string(),
            tool_name: None,
            tool_args: None,
            tool_result: None,
            extra: HashMap::new(),
        }
    }

    /// 设置工具信息
    pub fn with_tool(mut self, name: &str, args: serde_json::Value) -> Self {
        self.tool_name = Some(name.to_string());
        self.tool_args = Some(args);
        self
    }

    /// 设置工具结果
    pub fn with_result(mut self, result: &str) -> Self {
        self.tool_result = Some(result.to_string());
        self
    }

    /// 添加额外数据
    pub fn with_extra(mut self, key: &str, value: &str) -> Self {
        self.extra.insert(key.to_string(), value.to_string());
        self
    }
}

/// Extension trait - 扩展能力
#[async_trait]
pub trait Extension: Send + Sync + Debug {
    /// 获取扩展名称
    fn name(&self) -> &str;

    /// 获取关联的 Hooks
    fn hooks(&self) -> Vec<Arc<dyn Hook>>;
}

/// Hook 集合类型
type HookMap = HashMap<HookPhase, Vec<Arc<dyn Hook>>>;

/// 扩展管理器
pub struct ExtensionManager {
    extensions: Arc<RwLock<Vec<Arc<dyn Extension>>>>,
    hooks: Arc<RwLock<HookMap>>,
}

impl std::fmt::Debug for ExtensionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionManager").finish()
    }
}

impl ExtensionManager {
    /// 创建新的扩展管理器
    pub fn new() -> Self {
        Self {
            extensions: Arc::new(RwLock::new(Vec::new())),
            hooks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册扩展（同步版本，用于初始化）
    pub fn register_blocking<E: Extension + 'static>(&self, extension: E) {
        // 先获取 hooks
        let ext_hooks = extension.hooks();

        // 注册关联的 Hooks
        {
            let mut hooks = self.hooks.blocking_write();
            for hook in ext_hooks {
                hooks.entry(hook.phase()).or_default().push(hook);
            }
        }

        // 注册扩展
        self.extensions.blocking_write().push(Arc::new(extension));
    }

    /// 异步注册扩展
    pub async fn register<E: Extension + 'static>(&self, extension: E) {
        // 先获取 hooks
        let ext_hooks = extension.hooks();

        // 注册关联的 Hooks
        {
            let mut hooks = self.hooks.write().await;
            for hook in ext_hooks {
                hooks
                    .entry(hook.phase())
                    .or_insert_with(Vec::new)
                    .push(hook);
            }
        }

        // 注册扩展
        self.extensions.write().await.push(Arc::new(extension));
    }

    /// 调用 before_prompt hooks
    pub async fn call_before_prompt(&self, session: &mut Session, input: &str) -> Result<()> {
        let context = HookContext::new(input);
        self.execute_hooks(HookPhase::BeforePrompt, session, context)
            .await
    }

    /// 调用 before_tool hooks
    pub async fn call_before_tool(
        &self,
        session: &mut Session,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<()> {
        let context = HookContext::new("").with_tool(tool_name, args);
        self.execute_hooks(HookPhase::BeforeTool, session, context)
            .await
    }

    /// 调用 after_tool hooks
    pub async fn call_after_tool(
        &self,
        session: &mut Session,
        tool_name: &str,
        result: &str,
    ) -> Result<()> {
        let context = HookContext::new("")
            .with_tool(tool_name, serde_json::json!({}))
            .with_result(result);
        self.execute_hooks(HookPhase::AfterTool, session, context)
            .await
    }

    /// 调用 after_complete hooks
    pub async fn call_after_complete(&self, session: &mut Session) -> Result<()> {
        let context = HookContext::new("");
        self.execute_hooks(HookPhase::AfterComplete, session, context)
            .await
    }

    /// 执行指定阶段的 hooks
    async fn execute_hooks(
        &self,
        phase: HookPhase,
        session: &mut Session,
        context: HookContext,
    ) -> Result<()> {
        // 在单独的 scope 中获取锁，确保 guard 及时释放
        let phase_hooks: Vec<Arc<dyn Hook>> = {
            let hooks_guard = self.hooks.read().await;
            hooks_guard.get(&phase).cloned().unwrap_or_default()
        };

        // 在 phase_hooks 上迭代，不持有锁
        for hook in phase_hooks {
            hook.execute(session, &context).await?;
        }
        Ok(())
    }
}

impl Default for ExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============== 内置 Hooks 实现 ==============

/// 日志 Hook
#[derive(Debug)]
pub struct LoggingHook {
    phase: HookPhase,
}

impl LoggingHook {
    pub fn new(phase: HookPhase) -> Self {
        Self { phase }
    }
}

#[async_trait]
impl Hook for LoggingHook {
    fn name(&self) -> &str {
        "logging_hook"
    }

    fn phase(&self) -> HookPhase {
        self.phase
    }

    async fn execute(&self, session: &mut Session, context: &HookContext) -> Result<()> {
        match self.phase {
            HookPhase::BeforePrompt => {
                tracing::info!(
                    "[BeforePrompt] Session: {}, Input: {}",
                    session.id,
                    context.input
                );
            }
            HookPhase::BeforeTool => {
                tracing::info!(
                    "[BeforeTool] Session: {}, Tool: {:?}",
                    session.id,
                    context.tool_name
                );
            }
            HookPhase::AfterTool => {
                tracing::info!(
                    "[AfterTool] Session: {}, Tool: {:?}, Result: {:?}",
                    session.id,
                    context.tool_name,
                    context.tool_result.as_ref().map(|s| &s[..s.len().min(100)])
                );
            }
            HookPhase::AfterComplete => {
                tracing::info!("[AfterComplete] Session: {}", session.id);
            }
        }
        Ok(())
    }
}

/// 敏感词过滤 Hook
#[derive(Debug)]
pub struct SensitiveWordFilterHook {
    forbidden_words: Vec<String>,
}

impl SensitiveWordFilterHook {
    pub fn new(words: Vec<String>) -> Self {
        Self {
            forbidden_words: words,
        }
    }
}

#[async_trait]
impl Hook for SensitiveWordFilterHook {
    fn name(&self) -> &str {
        "sensitive_word_filter"
    }

    fn phase(&self) -> HookPhase {
        HookPhase::BeforePrompt
    }

    async fn execute(&self, _session: &mut Session, context: &HookContext) -> Result<()> {
        let input_lower = context.input.to_lowercase();
        for word in &self.forbidden_words {
            if input_lower.contains(&word.to_lowercase()) {
                return Err(anyhow::anyhow!("Forbidden word detected: {}", word));
            }
        }
        Ok(())
    }
}

/// Token 统计 Hook
#[derive(Debug)]
pub struct TokenCountHook {
    phase: HookPhase,
}

impl TokenCountHook {
    pub fn new(phase: HookPhase) -> Self {
        Self { phase }
    }
}

#[async_trait]
impl Hook for TokenCountHook {
    fn name(&self) -> &str {
        "token_count_hook"
    }

    fn phase(&self) -> HookPhase {
        self.phase
    }

    async fn execute(&self, session: &mut Session, context: &HookContext) -> Result<()> {
        let msg_count = session.messages().len();
        let char_count = context.input.len();
        // 粗略估算: 1 token ≈ 4 characters
        let token_estimate = char_count / 4;

        match self.phase {
            HookPhase::BeforePrompt => {
                tracing::debug!(
                    "[TokenCount] BeforePrompt - Message count: {}, Input chars: {}, Estimated tokens: {}",
                    msg_count, char_count, token_estimate
                );
            }
            HookPhase::AfterComplete => {
                let total_chars: usize = session.messages().iter().map(|m| m.content.len()).sum();
                tracing::debug!(
                    "[TokenCount] AfterComplete - Total messages: {}, Total chars: {}, Estimated tokens: {}",
                    msg_count, total_chars, total_chars / 4
                );
            }
            _ => {}
        }
        Ok(())
    }
}

/// 包含常用内置 Hooks 的扩展
#[derive(Debug)]
pub struct BuiltinExtensions;

impl BuiltinExtensions {
    pub fn logging() -> Self {
        Self
    }

    pub fn with_sensitive_filter(words: Vec<String>) -> impl Extension {
        BuiltinExtensionWithFilter(words)
    }
}

#[async_trait]
impl Extension for BuiltinExtensions {
    fn name(&self) -> &str {
        "builtin_extensions"
    }

    fn hooks(&self) -> Vec<Arc<dyn Hook>> {
        vec![
            Arc::new(LoggingHook::new(HookPhase::BeforePrompt)),
            Arc::new(LoggingHook::new(HookPhase::AfterComplete)),
            Arc::new(TokenCountHook::new(HookPhase::BeforePrompt)),
            Arc::new(TokenCountHook::new(HookPhase::AfterComplete)),
        ]
    }
}

#[derive(Debug)]
struct BuiltinExtensionWithFilter(Vec<String>);

#[async_trait]
impl Extension for BuiltinExtensionWithFilter {
    fn name(&self) -> &str {
        "builtin_with_filter"
    }

    fn hooks(&self) -> Vec<Arc<dyn Hook>> {
        vec![
            Arc::new(LoggingHook::new(HookPhase::BeforePrompt)),
            Arc::new(LoggingHook::new(HookPhase::AfterComplete)),
            Arc::new(SensitiveWordFilterHook::new(self.0.clone())),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extension_manager() {
        let manager = ExtensionManager::new();
        assert!(manager
            .call_before_prompt(&mut Session::new("test"), "Hello")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_sensitive_word_filter() {
        let filter = SensitiveWordFilterHook::new(vec!["bad".to_string()]);
        let mut session = Session::new("test");
        let context = HookContext::new("This is bad content");

        let result = filter.execute(&mut session, &context).await;
        assert!(result.is_err());
    }
}
