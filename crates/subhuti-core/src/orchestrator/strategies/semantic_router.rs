use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::runtime::llm::{Message, LLM};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMatchResult {
    pub target_id: String,
    pub confidence: f64,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCandidate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
}

pub struct SemanticRouter {
    llm: Arc<dyn LLM>,
    enabled: bool,
}

struct DisabledLLM;

#[async_trait::async_trait]
impl LLM for DisabledLLM {
    fn provider(&self) -> crate::runtime::llm::LLMProvider {
        crate::runtime::llm::LLMProvider::Custom
    }
    fn config(&self) -> &crate::runtime::llm::LLMConfig {
        unimplemented!()
    }
    async fn chat(&self, _messages: Vec<Message>) -> crate::Result<String> {
        Ok(String::new())
    }
    async fn chat_with_tools(
        &self,
        _messages: Vec<Message>,
        _tools: Vec<crate::runtime::llm::ToolInfo>,
    ) -> crate::Result<crate::runtime::llm::LLMResponse> {
        unimplemented!()
    }
    async fn chat_streaming(
        &self,
        _messages: Vec<Message>,
        _callback: Box<dyn Fn(String) + Send>,
    ) -> crate::Result<()> {
        unimplemented!()
    }
    async fn health_check(&self) -> crate::Result<bool> {
        Ok(false)
    }
}

impl SemanticRouter {
    pub fn new(llm: Arc<dyn LLM>) -> Self {
        Self { llm, enabled: true }
    }

    pub fn new_disabled() -> Self {
        Self {
            llm: Arc::new(DisabledLLM),
            enabled: false,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub async fn match_candidate(
        &self,
        input: &str,
        candidates: Vec<SemanticCandidate>,
    ) -> Option<SemanticMatchResult> {
        if !self.enabled || candidates.is_empty() {
            return None;
        }

        let prompt = self.build_prompt(input, &candidates);
        let messages = vec![
            Message::system("你是一个智能路由助手，负责根据用户输入选择最合适的专家或工作流。"),
            Message::user(&prompt),
        ];

        match self.llm.chat(messages).await {
            Ok(response) => self.parse_response(&response, &candidates),
            Err(e) => {
                tracing::warn!("语义路由 LLM 调用失败: {}", e);
                None
            }
        }
    }

    fn build_prompt(&self, input: &str, candidates: &[SemanticCandidate]) -> String {
        let mut prompt = format!("用户输入: {}\n\n候选列表:\n", input);

        for (i, candidate) in candidates.iter().enumerate() {
            let tags = candidate.tags.join(", ");
            prompt.push_str(&format!(
                "{}. ID: {}, 名称: {}, 描述: {}, 标签: {}\n",
                i + 1,
                candidate.id,
                candidate.name,
                candidate.description,
                tags
            ));
        }

        prompt.push_str("\n请选择最匹配的候选，返回 JSON 格式：{\"id\": \"候选ID\", \"confidence\": 置信度(0-1), \"reasoning\": \"选择理由\"}");
        prompt
    }

    fn parse_response(
        &self,
        response: &str,
        candidates: &[SemanticCandidate],
    ) -> Option<SemanticMatchResult> {
        let json_str = extract_json(response);
        match serde_json::from_str::<serde_json::Value>(&json_str) {
            Ok(value) => {
                let id = value.get("id")?.as_str()?.to_string();
                let confidence = value.get("confidence")?.as_f64()?;
                let reasoning = value.get("reasoning")?.as_str()?.to_string();

                if candidates.iter().any(|c| c.id == id) {
                    Some(SemanticMatchResult {
                        target_id: id,
                        confidence,
                        reasoning,
                    })
                } else {
                    tracing::warn!("语义路由返回的 ID {} 不在候选列表中", id);
                    None
                }
            }
            Err(e) => {
                tracing::warn!("语义路由响应解析失败: {}", e);
                None
            }
        }
    }
}

fn extract_json(text: &str) -> &str {
    if let Some(start) = text.find('{') {
        let mut depth = 0;
        let mut current_byte = start;
        let bytes = text.as_bytes();

        while current_byte < bytes.len() {
            let c = bytes[current_byte] as char;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &text[start..=current_byte];
                    }
                }
                _ => {}
            }
            current_byte += 1;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLLM;

    #[async_trait::async_trait]
    impl LLM for TestLLM {
        fn provider(&self) -> crate::runtime::llm::LLMProvider {
            crate::runtime::llm::LLMProvider::Custom
        }
        fn config(&self) -> &crate::runtime::llm::LLMConfig {
            unimplemented!()
        }
        async fn chat(&self, _messages: Vec<Message>) -> crate::Result<String> {
            Ok(String::from(
                "{\"id\": \"expert_1\", \"confidence\": 0.9, \"reasoning\": \"匹配\"}",
            ))
        }
        async fn chat_with_tools(
            &self,
            _messages: Vec<Message>,
            _tools: Vec<crate::runtime::llm::ToolInfo>,
        ) -> crate::Result<crate::runtime::llm::LLMResponse> {
            unimplemented!()
        }
        async fn chat_streaming(
            &self,
            _messages: Vec<Message>,
            _callback: Box<dyn Fn(String) + Send>,
        ) -> crate::Result<()> {
            unimplemented!()
        }
        async fn health_check(&self) -> crate::Result<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_semantic_match() {
        let mock_llm = Arc::new(TestLLM);
        let router = SemanticRouter::new(mock_llm);

        let candidates = vec![
            SemanticCandidate {
                id: "expert_1".to_string(),
                name: "代码专家".to_string(),
                description: "擅长编程和代码分析".to_string(),
                tags: vec!["代码".to_string(), "编程".to_string()],
            },
            SemanticCandidate {
                id: "expert_2".to_string(),
                name: "设计专家".to_string(),
                description: "擅长UI设计和用户体验".to_string(),
                tags: vec!["设计".to_string(), "UI".to_string()],
            },
        ];

        let result = router
            .match_candidate("帮我写一段 Rust 代码", candidates)
            .await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().target_id, "expert_1");
    }

    #[tokio::test]
    async fn test_semantic_match_disabled() {
        let router = SemanticRouter::new_disabled();

        let candidates = vec![SemanticCandidate {
            id: "expert_1".to_string(),
            name: "代码专家".to_string(),
            description: "擅长编程和代码分析".to_string(),
            tags: vec!["代码".to_string(), "编程".to_string()],
        }];

        let result = router
            .match_candidate("帮我写一段 Rust 代码", candidates)
            .await;
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_json() {
        let text = "一些前置文字 {\"key\": \"value\"} 一些后置文字";
        assert_eq!(extract_json(text), "{\"key\": \"value\"}");

        let text = "{\"key\": \"value\"}";
        assert_eq!(extract_json(text), "{\"key\": \"value\"}");

        let text = "没有 JSON";
        assert_eq!(extract_json(text), "没有 JSON");
    }
}
