//! # Embedding 模块
//!
//! 使用 Ollama 的 embedding API 生成文本向量，支持 pgvector 语义搜索。

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Embedding 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    /// Ollama API 地址
    pub api_url: String,
    /// 模型名称
    pub model: String,
    /// 向量维度
    pub dimensions: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:11434".to_string(),
            model: "bge-m3:latest".to_string(),
            dimensions: 1024,
        }
    }
}

/// Embedding 生成器
#[derive(Debug, Clone)]
pub struct EmbeddingService {
    config: EmbeddingConfig,
    client: reqwest::Client,
}

impl EmbeddingService {
    /// 创建新的 embedding 服务
    pub fn new(config: EmbeddingConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// 获取配置
    pub fn config(&self) -> &EmbeddingConfig {
        &self.config
    }

    /// 生成单个文本的 embedding
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.config.api_url);

        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": text,
        });

        let response = self.client.post(&url).json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Embedding API error ({}): {}", status, text);
        }

        let result: EmbeddingResponse = response.json().await?;

        if result.embedding.len() != self.config.dimensions {
            tracing::warn!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.config.dimensions,
                result.embedding.len()
            );
        }

        Ok(result.embedding)
    }

    /// 批量生成 embedding（串行调用，后续可优化为并行）
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// 将向量转换为 pgvector 格式字符串（用于 SQL 查询）
    pub fn to_pgvector_string(embedding: &[f32]) -> String {
        let values: Vec<String> = embedding.iter().map(|v| v.to_string()).collect();
        format!("[{}]", values.join(","))
    }
}

/// Ollama embedding API 响应
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedding_service() {
        let config = EmbeddingConfig::default();
        let service = EmbeddingService::new(config);

        let result = service.embed("Hello world").await;
        match result {
            Ok(emb) => {
                assert_eq!(emb.len(), 1024);
                println!("Embedding generated: {} dimensions", emb.len());
            }
            Err(e) => {
                println!("Embedding test skipped (Ollama not running?): {}", e);
            }
        }
    }

    #[test]
    fn test_pgvector_format() {
        let emb = vec![0.1, 0.2, 0.3];
        let s = EmbeddingService::to_pgvector_string(&emb);
        assert_eq!(s, "[0.1,0.2,0.3]");
    }
}
