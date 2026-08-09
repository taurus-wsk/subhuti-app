use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionStrategy {
    SummaryOnly,
    DeduplicationOnly,
    Both,
}

pub struct ContextCompressor {
    strategy: CompressionStrategy,
}

impl ContextCompressor {
    pub fn new(strategy: CompressionStrategy) -> Self {
        Self { strategy }
    }

    pub async fn compress(&self, manager: &super::manager::ContextManager) {
        match self.strategy {
            CompressionStrategy::SummaryOnly => {
                self.summarize_low_priority(manager).await;
            }
            CompressionStrategy::DeduplicationOnly => {
                self.deduplicate(manager).await;
            }
            CompressionStrategy::Both => {
                self.deduplicate(manager).await;
                self.summarize_low_priority(manager).await;
            }
        }
    }

    async fn summarize_low_priority(&self, manager: &super::manager::ContextManager) {
        let snapshot = manager.snapshot().await;

        if snapshot.low_entries.len() >= 3 {
            let low_contents: Vec<String> = snapshot
                .low_entries
                .iter()
                .map(|e| e.content.clone())
                .collect();

            let summary = self.generate_summary(&low_contents);

            for entry in &snapshot.low_entries {
                manager.remove_by_id(&entry.id).await;
            }

            let entry = super::manager::ContextEntry {
                id: format!("entry_{}", chrono::Utc::now().timestamp_millis()),
                content: summary,
                priority: super::manager::ContextPriority::Low,
                timestamp: chrono::Utc::now(),
                source: "compressed_summary".to_string(),
                metadata: std::collections::HashMap::new(),
            };
            manager.add_entry(entry).await;
        }
    }

    async fn deduplicate(&self, manager: &super::manager::ContextManager) {
        let snapshot = manager.snapshot().await;

        let mut seen = HashSet::new();
        let mut to_remove = Vec::new();

        for entry in snapshot
            .normal_entries
            .iter()
            .chain(snapshot.low_entries.iter())
        {
            let inserted = seen.insert(entry.content.clone());
            if !inserted {
                to_remove.push(entry.id.clone());
            }
        }

        for id in to_remove {
            manager.remove_by_id(&id).await;
        }
    }

    fn generate_summary(&self, contents: &[String]) -> String {
        if contents.is_empty() {
            return "".to_string();
        }
        if contents.len() == 1 {
            return contents[0].clone();
        }

        let combined = contents.join(" ");
        let max_chars = std::cmp::min(combined.chars().count(), 200);

        let summary: String = combined.chars().take(max_chars).collect();
        format!("摘要（{}条）: {}", contents.len(), summary)
    }
}

pub struct SummaryCompressor;

impl SummaryCompressor {
    pub fn new() -> Self {
        Self
    }

    pub async fn compress(&self, manager: &super::manager::ContextManager) {
        let compressor = ContextCompressor::new(CompressionStrategy::SummaryOnly);
        compressor.compress(manager).await;
    }
}

pub struct DeduplicationCompressor;

impl DeduplicationCompressor {
    pub fn new() -> Self {
        Self
    }

    pub async fn compress(&self, manager: &super::manager::ContextManager) {
        let compressor = ContextCompressor::new(CompressionStrategy::DeduplicationOnly);
        compressor.compress(manager).await;
    }
}

#[cfg(test)]
mod tests {
    use super::super::manager::{ContextConfig, ContextManager};
    use super::*;

    #[tokio::test]
    async fn test_context_compressor_summary() {
        let config = ContextConfig {
            enable_compression: true,
            enable_deduplication: false,
            ..Default::default()
        };
        let manager = ContextManager::new(config);

        manager
            .add_low("第一条低优先级信息，内容比较长", "source")
            .await;
        manager
            .add_low("第二条低优先级信息，内容也比较长", "source")
            .await;
        manager
            .add_low("第三条低优先级信息，内容同样比较长", "source")
            .await;

        let compressor = ContextCompressor::new(CompressionStrategy::SummaryOnly);
        compressor.compress(&manager).await;

        let snapshot = manager.snapshot().await;
        assert!(snapshot.low_entries.len() <= 1);
        assert!(snapshot.low_entries[0].content.contains("摘要"));
    }

    #[tokio::test]
    async fn test_context_compressor_deduplication() {
        let config = ContextConfig {
            enable_compression: false,
            enable_deduplication: false,
            enable_overflow_protection: false,
            max_tokens: 1000,
            max_entries: 100,
            compression_threshold_ratio: 0.8,
            safety_margin: 512,
        };
        let manager = ContextManager::new(config);

        manager.add_normal("duplicate content", "source1").await;
        manager.add_normal("duplicate content", "source2").await;
        manager.add_normal("different content", "source3").await;

        let snapshot_before = manager.snapshot().await;
        assert_eq!(snapshot_before.normal_entries.len(), 3);

        let compressor = ContextCompressor::new(CompressionStrategy::DeduplicationOnly);
        compressor.compress(&manager).await;

        let snapshot = manager.snapshot().await;
        assert_eq!(snapshot.normal_entries.len(), 2);
    }

    #[tokio::test]
    async fn test_context_compressor_both() {
        let config = ContextConfig {
            enable_compression: true,
            enable_deduplication: false,
            enable_overflow_protection: false,
            max_tokens: 1000,
            ..Default::default()
        };
        let manager = ContextManager::new(config);

        manager
            .add_low("duplicate low priority content", "source1")
            .await;
        manager
            .add_low("duplicate low priority content", "source2")
            .await;
        manager
            .add_low("another low priority content", "source3")
            .await;
        manager
            .add_low("third low priority content", "source4")
            .await;

        let compressor = ContextCompressor::new(CompressionStrategy::Both);
        compressor.compress(&manager).await;

        let snapshot = manager.snapshot().await;
        assert!(snapshot.low_entries.len() <= 1);
    }
}
