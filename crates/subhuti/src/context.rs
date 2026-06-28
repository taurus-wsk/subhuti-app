//! # 上下文模块
//!
//! 采用 HTTP 框架类似的分层设计：
//! - **全局状态**：Subhuti 结构体本身（类似 AppState），包含 runtime、memory 等共享资源
//! - **请求级上下文**：RunContext（类似 Request Extensions），包含 session、tokens、chain 等每次请求的数据
//!
//! ## 设计理念
//!
//! 参考 Axum 的 State + Extensions 模式：
//! - 全局资源只读共享，用 Arc
//! - 请求级资源可变，生命周期与请求绑定
//! - 避免"上帝对象"，职责清晰

use crate::runtime::Session;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Token 使用统计
#[derive(Debug, Clone, Default)]
pub struct TokenStats {
    /// 使用的模型
    pub model: Option<String>,
    /// Prompt Token 总数
    pub prompt_tokens: u32,
    /// Completion Token 总数
    pub completion_tokens: u32,
    /// Token 总数
    pub total_tokens: u32,
}

impl TokenStats {
    /// 添加 token 使用
    pub fn add(&mut self, response: &crate::runtime::llm::LLMResponse) {
        if let Some(model) = &response.model {
            if self.model.is_none() {
                self.model = Some(model.clone());
            }
        }
        if let Some(prompt) = response.prompt_tokens {
            self.prompt_tokens += prompt;
        }
        if let Some(completion) = response.completion_tokens {
            self.completion_tokens += completion;
        }
        if let Some(total) = response.total_tokens {
            self.total_tokens += total;
        }
    }
}

/// 请求级运行上下文
///
/// 类似 HTTP 的 Request Extensions，每次请求创建一个，
/// 包含该次请求的所有可变状态。
///
/// 全局共享资源（runtime、memory）不放在这里，
/// 通过 &Subhuti 或直接传引用访问。
pub struct RunContext {
    /// 会话状态
    pub session: Session,
    /// Token 统计（Arc 支持跨调用共享）
    pub tokens: Arc<RwLock<TokenStats>>,
    /// Skill 调用链
    pub chain: Vec<String>,
}

impl RunContext {
    /// 创建新的请求级上下文
    pub fn new(session: Session) -> Self {
        Self {
            session,
            tokens: Arc::new(RwLock::new(TokenStats::default())),
            chain: Vec::new(),
        }
    }

    /// 添加到调用链
    pub fn add_to_chain(&mut self, skill_name: &str) {
        self.chain.push(skill_name.to_string());
    }

    /// 获取当前调用链
    pub fn get_chain(&self) -> &[String] {
        &self.chain
    }
}
