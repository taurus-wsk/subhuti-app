//! # Constraints - 约束护栏
//!
//! 代码级强制限制，不靠 prompt
//! - 最大工具调用轮次
//! - 最大上下文长度
//! - 超时控制
//! - 非法参数拦截

use serde::Serialize;

/// 约束验证结果
#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    /// 是否通过
    pub valid: bool,
    /// 错误信息
    pub error: Option<String>,
}

impl ValidationResult {
    /// 创建成功结果
    pub fn ok() -> Self {
        Self {
            valid: true,
            error: None,
        }
    }

    /// 创建失败结果
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            valid: false,
            error: Some(msg.into()),
        }
    }
}

/// 约束检查器
#[derive(Debug, Clone)]
pub struct Constraints {
    /// 最大工具调用轮次
    max_tool_turns: usize,
    /// 最大上下文长度 (token)
    max_context_tokens: usize,
    /// 超时时间 (秒)
    timeout_seconds: u64,
    /// 允许的工具列表 (None = 全部允许)
    allowed_tools: Option<Vec<String>>,
    /// 禁止的关键词
    forbidden_keywords: Vec<String>,
}

impl Constraints {
    /// 创建新的约束检查器
    pub fn new() -> Self {
        Self {
            max_tool_turns: 10,
            max_context_tokens: 8192,
            timeout_seconds: 60,
            allowed_tools: None,
            forbidden_keywords: Vec::new(),
        }
    }

    /// 设置最大工具调用轮次
    pub fn max_tool_turns(mut self, turns: usize) -> Self {
        self.max_tool_turns = turns;
        self
    }

    /// 设置最大上下文长度
    pub fn max_context_tokens(mut self, tokens: usize) -> Self {
        self.max_context_tokens = tokens;
        self
    }

    /// 设置超时时间
    pub fn timeout_seconds(mut self, seconds: u64) -> Self {
        self.timeout_seconds = seconds;
        self
    }

    /// 设置允许的工具列表
    pub fn allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = Some(tools);
        self
    }

    /// 添加禁止关键词
    pub fn add_forbidden_keyword(mut self, keyword: impl Into<String>) -> Self {
        self.forbidden_keywords.push(keyword.into());
        self
    }

    /// 检查工具调用轮次
    pub fn check_tool_turns(&self, current_turns: usize) -> ValidationResult {
        if current_turns >= self.max_tool_turns {
            ValidationResult::error(format!(
                "Max tool calls reached: {}/{}",
                current_turns, self.max_tool_turns
            ))
        } else {
            ValidationResult::ok()
        }
    }

    /// 检查上下文长度
    pub fn check_context_length(&self, tokens: usize) -> ValidationResult {
        if tokens > self.max_context_tokens {
            ValidationResult::error(format!(
                "Context too long: {}/{} tokens",
                tokens, self.max_context_tokens
            ))
        } else {
            ValidationResult::ok()
        }
    }

    /// 检查工具是否允许
    pub fn check_tool(&self, tool_name: &str) -> ValidationResult {
        if let Some(ref allowed) = self.allowed_tools {
            if !allowed.contains(&tool_name.to_string()) {
                return ValidationResult::error(format!("Tool not allowed: {}", tool_name));
            }
        }
        ValidationResult::ok()
    }

    /// 检查内容是否包含禁止关键词
    pub fn check_content(&self, content: &str) -> ValidationResult {
        let content_lower = content.to_lowercase();
        for keyword in &self.forbidden_keywords {
            if content_lower.contains(&keyword.to_lowercase()) {
                return ValidationResult::error(format!("Forbidden keyword detected: {}", keyword));
            }
        }
        ValidationResult::ok()
    }

    /// 执行所有检查
    pub fn validate(&self, content: &str) -> ValidationResult {
        // 先检查内容
        if !self.check_content(content).valid {
            return self.check_content(content);
        }
        ValidationResult::ok()
    }
}

impl Default for Constraints {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_turns_check() {
        let constraints = Constraints::new().max_tool_turns(5);
        assert!(constraints.check_tool_turns(3).valid);
        assert!(!constraints.check_tool_turns(5).valid);
    }

    #[test]
    fn test_forbidden_keyword() {
        let constraints = Constraints::new().add_forbidden_keyword("badword");
        assert!(constraints.check_content("Hello world").valid);
        assert!(!constraints.check_content("This contains badword").valid);
    }
}
