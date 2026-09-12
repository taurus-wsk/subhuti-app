#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowStrategy {
    CompressThenDrop,
    DropLowPriority,
    DropOldest,
    TruncateContent,
}

pub struct OverflowProtection {
    max_tokens: usize,
    safety_margin: usize,
    strategy: OverflowStrategy,
}

impl OverflowProtection {
    pub fn new(max_tokens: usize, safety_margin: usize, strategy: OverflowStrategy) -> Self {
        Self {
            max_tokens,
            safety_margin,
            strategy,
        }
    }

    pub async fn protect(&self, manager: &super::manager::ContextManager) {
        let current_tokens = manager.get_token_count().await;
        let safe_limit = self.max_tokens - self.safety_margin;

        if current_tokens <= safe_limit {
            return;
        }

        match self.strategy {
            OverflowStrategy::CompressThenDrop => {
                self.compress_then_drop(manager, safe_limit).await;
            }
            OverflowStrategy::DropLowPriority => {
                self.drop_low_priority(manager, safe_limit).await;
            }
            OverflowStrategy::DropOldest => {
                self.drop_oldest(manager, safe_limit).await;
            }
            OverflowStrategy::TruncateContent => {
                self.truncate_content(manager, safe_limit).await;
            }
        }
    }

    async fn compress_then_drop(
        &self,
        manager: &super::manager::ContextManager,
        safe_limit: usize,
    ) {
        let compressor =
            super::compressor::ContextCompressor::new(super::compressor::CompressionStrategy::Both);
        compressor.compress(manager).await;

        let current_tokens = manager.get_token_count().await;
        if current_tokens <= safe_limit {
            return;
        }

        self.drop_low_priority(manager, safe_limit).await;
    }

    async fn drop_low_priority(&self, manager: &super::manager::ContextManager, safe_limit: usize) {
        manager
            .retain_priority(super::manager::ContextPriority::Normal)
            .await;

        let current_tokens = manager.get_token_count().await;
        if current_tokens <= safe_limit {
            return;
        }

        manager
            .retain_priority(super::manager::ContextPriority::High)
            .await;
    }

    async fn drop_oldest(&self, manager: &super::manager::ContextManager, safe_limit: usize) {
        let snapshot = manager.snapshot().await;

        let mut sorted_entries: Vec<super::manager::ContextEntry> = snapshot
            .normal_entries
            .into_iter()
            .chain(snapshot.low_entries)
            .collect();

        sorted_entries.sort_by_key(|a| a.timestamp);

        for entry in sorted_entries {
            let current_tokens = manager.get_token_count().await;
            if current_tokens <= safe_limit {
                break;
            }
            manager.remove_by_id(&entry.id).await;
        }
    }

    async fn truncate_content(&self, manager: &super::manager::ContextManager, safe_limit: usize) {
        let snapshot = manager.snapshot().await;

        let mut low_priority_entries: Vec<super::manager::ContextEntry> = snapshot
            .normal_entries
            .into_iter()
            .chain(snapshot.low_entries)
            .collect();

        low_priority_entries.sort_by(|a, b| b.priority.cmp(&a.priority));

        for entry in low_priority_entries {
            let current_tokens = manager.get_token_count().await;
            if current_tokens <= safe_limit {
                break;
            }

            let truncated_content = if entry.content.len() > 50 {
                entry.content.chars().take(50).collect::<String>() + "..."
            } else {
                entry.content.clone()
            };

            manager.remove_by_id(&entry.id).await;
            let new_entry = super::manager::ContextEntry {
                id: format!("entry_{}", chrono::Utc::now().timestamp_millis()),
                content: truncated_content,
                priority: entry.priority,
                timestamp: chrono::Utc::now(),
                source: entry.source,
                metadata: std::collections::HashMap::new(),
            };
            manager.add_entry(new_entry).await;
        }
    }

    pub fn get_max_tokens(&self) -> usize {
        self.max_tokens
    }

    pub fn get_safety_margin(&self) -> usize {
        self.safety_margin
    }

    pub fn get_strategy(&self) -> OverflowStrategy {
        self.strategy
    }
}

#[cfg(test)]
mod tests {
    use super::super::manager::{ContextConfig, ContextManager};
    use super::*;

    #[tokio::test]
    async fn test_overflow_protection_compress_then_drop() {
        let config = ContextConfig {
            max_tokens: 50,
            safety_margin: 10,
            enable_compression: true,
            enable_deduplication: true,
            enable_overflow_protection: true,
            compression_threshold_ratio: 0.1,
            ..Default::default()
        };
        let manager = ContextManager::new(config);

        for i in 0..20 {
            manager
                .add_low(
                    &format!("entry{}: very long content that should be compressed", i),
                    "source",
                )
                .await;
        }

        let op = OverflowProtection::new(50, 10, OverflowStrategy::CompressThenDrop);
        op.protect(&manager).await;

        let current_tokens = manager.get_token_count().await;
        assert!(current_tokens <= 40);
    }

    #[tokio::test]
    async fn test_overflow_protection_drop_low_priority() {
        let config = ContextConfig {
            max_tokens: 1000,
            safety_margin: 100,
            enable_compression: false,
            enable_deduplication: false,
            enable_overflow_protection: true,
            compression_threshold_ratio: 0.1,
            ..Default::default()
        };
        let manager = ContextManager::new(config);

        for i in 0..5 {
            manager
                .add_critical(&format!("critical{}", i), "source")
                .await;
        }
        for i in 0..15 {
            manager
                .add_low(&format!("low{}: long content to be dropped", i), "source")
                .await;
        }

        let snapshot_before = manager.snapshot().await;
        assert_eq!(snapshot_before.critical_entries.len(), 5);
        assert_eq!(snapshot_before.low_entries.len(), 15);

        let op = OverflowProtection::new(100, 10, OverflowStrategy::DropLowPriority);
        op.protect(&manager).await;

        let snapshot = manager.snapshot().await;
        assert_eq!(snapshot.critical_entries.len(), 5);
        assert!(snapshot.low_entries.len() < 15);
    }

    #[tokio::test]
    async fn test_overflow_protection_truncate_content() {
        let config = ContextConfig {
            max_tokens: 1000,
            safety_margin: 100,
            enable_compression: false,
            enable_deduplication: false,
            enable_overflow_protection: true,
            compression_threshold_ratio: 0.1,
            ..Default::default()
        };
        let manager = ContextManager::new(config);

        manager.add_normal("This is a very long content that should be truncated for testing truncation feature", "source").await;

        let snapshot_before = manager.snapshot().await;
        let original_len = snapshot_before.normal_entries[0].content.len();

        let op = OverflowProtection::new(10, 1, OverflowStrategy::TruncateContent);
        op.protect(&manager).await;

        let snapshot = manager.snapshot().await;
        assert!(snapshot.normal_entries[0].content.len() <= 53);
        assert!(snapshot.normal_entries[0].content.ends_with("..."));
        assert!(snapshot.normal_entries[0].content.len() < original_len);
    }
}
