use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextPriority {
    Critical,
    High,
    Normal,
    Low,
}

impl PartialOrd for ContextPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ContextPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let order = match self {
            ContextPriority::Critical => 4,
            ContextPriority::High => 3,
            ContextPriority::Normal => 2,
            ContextPriority::Low => 1,
        };
        let other_order = match other {
            ContextPriority::Critical => 4,
            ContextPriority::High => 3,
            ContextPriority::Normal => 2,
            ContextPriority::Low => 1,
        };
        order.cmp(&other_order)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    pub id: String,
    pub content: String,
    pub priority: ContextPriority,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub source: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    pub max_tokens: usize,
    pub safety_margin: usize,
    pub max_entries: usize,
    pub enable_compression: bool,
    pub enable_deduplication: bool,
    pub enable_overflow_protection: bool,
    pub compression_threshold_ratio: f64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 8192,
            safety_margin: 512,
            max_entries: 100,
            enable_compression: true,
            enable_deduplication: true,
            enable_overflow_protection: true,
            compression_threshold_ratio: 0.8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub total_tokens: usize,
    pub entry_count: usize,
    pub critical_entries: Vec<ContextEntry>,
    pub high_entries: Vec<ContextEntry>,
    pub normal_entries: Vec<ContextEntry>,
    pub low_entries: Vec<ContextEntry>,
}

pub struct ContextManager {
    entries: Arc<RwLock<VecDeque<ContextEntry>>>,
    config: ContextConfig,
    compressor: Option<super::compressor::ContextCompressor>,
    overflow_protection: Option<super::overflow_protection::OverflowProtection>,
}

impl ContextManager {
    pub fn new(config: ContextConfig) -> Self {
        let compressor = if config.enable_compression {
            Some(super::compressor::ContextCompressor::new(
                super::compressor::CompressionStrategy::Both,
            ))
        } else {
            None
        };

        let overflow_protection = if config.enable_overflow_protection {
            Some(super::overflow_protection::OverflowProtection::new(
                config.max_tokens,
                config.safety_margin,
                super::overflow_protection::OverflowStrategy::CompressThenDrop,
            ))
        } else {
            None
        };

        Self {
            entries: Arc::new(RwLock::new(VecDeque::new())),
            config,
            compressor,
            overflow_protection,
        }
    }

    pub async fn add_entry(&self, entry: ContextEntry) {
        let mut entries = self.entries.write().await;

        if self.config.enable_deduplication {
            entries.retain(|e| e.content != entry.content);
        }

        entries.push_back(entry);

        if entries.len() > self.config.max_entries {
            entries.pop_front();
        }
    }

    pub async fn add_entry_with_limits(&self, entry: ContextEntry) {
        self.add_entry(entry).await;
        self.enforce_limits().await;
    }

    pub async fn add_text(&self, content: &str, priority: ContextPriority, source: &str) {
        let entry = ContextEntry {
            id: format!("entry_{}", uuid::Uuid::new_v4()),
            content: content.to_string(),
            priority,
            timestamp: chrono::Utc::now(),
            source: source.to_string(),
            metadata: HashMap::new(),
        };
        self.add_entry_with_limits(entry).await;
    }

    pub async fn add_critical(&self, content: &str, source: &str) {
        self.add_text(content, ContextPriority::Critical, source)
            .await;
    }

    pub async fn add_high(&self, content: &str, source: &str) {
        self.add_text(content, ContextPriority::High, source).await;
    }

    pub async fn add_normal(&self, content: &str, source: &str) {
        self.add_text(content, ContextPriority::Normal, source)
            .await;
    }

    pub async fn add_low(&self, content: &str, source: &str) {
        self.add_text(content, ContextPriority::Low, source).await;
    }

    pub async fn get_prompt(&self) -> String {
        let entries = self.entries.read().await;

        let mut sorted_entries: Vec<&ContextEntry> = entries.iter().collect();
        sorted_entries.sort_by(|a, b| b.priority.cmp(&a.priority));

        let mut parts = Vec::new();
        for entry in sorted_entries {
            parts.push(format!("[{}] {}", entry.source, entry.content));
        }

        parts.join("\n\n")
    }

    pub async fn get_token_count(&self) -> usize {
        let entries = self.entries.read().await;
        entries.iter().map(|e| estimate_tokens(&e.content)).sum()
    }

    pub async fn snapshot(&self) -> ContextSnapshot {
        let entries = self.entries.read().await;

        let mut critical = Vec::new();
        let mut high = Vec::new();
        let mut normal = Vec::new();
        let mut low = Vec::new();

        for entry in entries.iter() {
            match entry.priority {
                ContextPriority::Critical => critical.push(entry.clone()),
                ContextPriority::High => high.push(entry.clone()),
                ContextPriority::Normal => normal.push(entry.clone()),
                ContextPriority::Low => low.push(entry.clone()),
            }
        }

        ContextSnapshot {
            total_tokens: entries.iter().map(|e| estimate_tokens(&e.content)).sum(),
            entry_count: entries.len(),
            critical_entries: critical,
            high_entries: high,
            normal_entries: normal,
            low_entries: low,
        }
    }

    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();
    }

    pub async fn remove_by_id(&self, id: &str) {
        let mut entries = self.entries.write().await;
        entries.retain(|e| e.id != id);
    }

    pub async fn retain_priority(&self, min_priority: ContextPriority) {
        let mut entries = self.entries.write().await;
        entries.retain(|e| e.priority >= min_priority);
    }

    async fn enforce_limits(&self) {
        let current_tokens = self.get_token_count().await;
        let threshold =
            (self.config.max_tokens as f64 * self.config.compression_threshold_ratio) as usize;

        if current_tokens >= threshold && self.config.enable_compression {
            if let Some(compressor) = &self.compressor {
                compressor.compress(self).await;
            }
        }

        if let Some(op) = &self.overflow_protection {
            op.protect(self).await;
        }
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_manager_basic() {
        let config = ContextConfig::default();
        let manager = ContextManager::new(config);

        manager.add_critical("核心指令", "system").await;
        manager.add_high("重要信息", "user").await;
        manager.add_normal("普通信息", "tool").await;

        let snapshot = manager.snapshot().await;
        assert_eq!(snapshot.entry_count, 3);
        assert_eq!(snapshot.critical_entries.len(), 1);
        assert_eq!(snapshot.high_entries.len(), 1);
        assert_eq!(snapshot.normal_entries.len(), 1);
    }

    #[tokio::test]
    async fn test_context_manager_deduplication() {
        let config = ContextConfig {
            enable_deduplication: true,
            ..Default::default()
        };
        let manager = ContextManager::new(config);

        manager.add_normal("重复内容", "source1").await;
        manager.add_normal("重复内容", "source2").await;

        let snapshot = manager.snapshot().await;
        assert_eq!(snapshot.entry_count, 1);
    }

    #[tokio::test]
    async fn test_context_manager_limits() {
        let config = ContextConfig {
            max_entries: 2,
            ..Default::default()
        };
        let manager = ContextManager::new(config);

        manager.add_normal("entry1", "source").await;
        manager.add_normal("entry2", "source").await;
        manager.add_normal("entry3", "source").await;

        let snapshot = manager.snapshot().await;
        assert_eq!(snapshot.entry_count, 2);
    }

    #[tokio::test]
    async fn test_context_manager_priority_sort() {
        let config = ContextConfig::default();
        let manager = ContextManager::new(config);

        manager.add_low("low", "source").await;
        manager.add_critical("critical", "source").await;
        manager.add_normal("normal", "source").await;
        manager.add_high("high", "source").await;

        let prompt = manager.get_prompt().await;
        assert!(prompt.starts_with("[source] critical"));
    }
}
