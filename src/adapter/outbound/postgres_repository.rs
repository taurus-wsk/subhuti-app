//! # PostgreSQL 数据仓库适配器
//!
//! 将领域层定义的 `DomainRepository` 接口转换为 PostgreSQL 数据库实现。
//!
//! 六边形架构：
//! - 领域层定义接口（DomainRepository）
//! - 出站适配层实现适配器（PostgresRepository）
//! - 专家通过依赖注入获取 repository，不依赖具体数据库实现

use async_trait::async_trait;
use sqlx::{postgres::PgRow, PgPool, Row};
use std::sync::Arc;

use crate::domain::traits::{DomainError, DomainRepository, DomainResult};

/// PostgreSQL 数据仓库适配器
///
/// 实现领域层的 DomainRepository 接口，使用 SQLx 连接 PostgreSQL。
pub struct PostgresRepository {
    pool: Arc<PgPool>,
}

impl PostgresRepository {
    /// 创建新的 PostgreSQL 数据仓库实例
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// 获取底层数据库连接池
    pub fn pool(&self) -> &Arc<PgPool> {
        &self.pool
    }

    /// 初始化数据库表（首次使用时调用）
    pub async fn init(&self) -> DomainResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS domain_data (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&*self.pool)
        .await
        .map_err(|e| DomainError::ExecutionError(format!("数据库表初始化失败: {}", e)))?;

        // 创建索引优化查询
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_domain_data_key ON domain_data (key)
            "#,
        )
        .execute(&*self.pool)
        .await
        .map_err(|e| DomainError::ExecutionError(format!("索引创建失败: {}", e)))?;

        Ok(())
    }
}

#[async_trait]
impl DomainRepository for PostgresRepository {
    async fn save(&self, key: &str, value: &str) -> DomainResult<()> {
        sqlx::query(
            r#"
            INSERT INTO domain_data (key, value, updated_at) 
            VALUES ($1, $2, CURRENT_TIMESTAMP) 
            ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&*self.pool)
        .await
        .map_err(|e| DomainError::ExecutionError(format!("数据库保存失败: {}", e)))?;

        Ok(())
    }

    async fn load(&self, key: &str) -> DomainResult<Option<String>> {
        let row = sqlx::query(
            r#"
            SELECT value FROM domain_data WHERE key = $1
            "#,
        )
        .bind(key)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| DomainError::ExecutionError(format!("数据库加载失败: {}", e)))?;

        Ok(row.map(|r: PgRow| r.get("value")))
    }

    async fn delete(&self, key: &str) -> DomainResult<()> {
        sqlx::query(
            r#"
            DELETE FROM domain_data WHERE key = $1
            "#,
        )
        .bind(key)
        .execute(&*self.pool)
        .await
        .map_err(|e| DomainError::ExecutionError(format!("数据库删除失败: {}", e)))?;

        Ok(())
    }

    async fn save_batch(&self, items: &[(&str, &str)]) -> DomainResult<()> {
        if items.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DomainError::ExecutionError(format!("事务开始失败: {}", e)))?;

        for (key, value) in items {
            sqlx::query(
                r#"
                INSERT INTO domain_data (key, value, updated_at) 
                VALUES ($1, $2, CURRENT_TIMESTAMP) 
                ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = CURRENT_TIMESTAMP
                "#,
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::ExecutionError(format!("批量保存失败: {}", e)))?;
        }

        tx.commit()
            .await
            .map_err(|e| DomainError::ExecutionError(format!("事务提交失败: {}", e)))?;

        Ok(())
    }

    async fn load_batch(&self, keys: &[&str]) -> DomainResult<Vec<(String, String)>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = keys
            .iter()
            .enumerate()
            .map(|(i, _)| format!("${}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let query = format!(
            r#"
            SELECT key, value FROM domain_data WHERE key IN ({})
            "#,
            placeholders
        );

        let mut query = sqlx::query(&query);
        for key in keys {
            query = query.bind(key);
        }

        let rows = query
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| DomainError::ExecutionError(format!("批量加载失败: {}", e)))?;

        let result: Vec<(String, String)> = rows
            .into_iter()
            .map(|r: PgRow| (r.get("key"), r.get("value")))
            .collect();

        Ok(result)
    }
}

/// 内存数据仓库（用于测试）
///
/// 在测试环境中使用内存存储，避免依赖数据库。
pub struct InMemoryRepository {
    data: std::sync::RwLock<std::collections::HashMap<String, String>>,
}

impl InMemoryRepository {
    /// 创建新的内存数据仓库实例
    pub fn new() -> Self {
        Self {
            data: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for InMemoryRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DomainRepository for InMemoryRepository {
    async fn save(&self, key: &str, value: &str) -> DomainResult<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| DomainError::ExecutionError(format!("获取写锁失败: {}", e)))?;
        data.insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn load(&self, key: &str) -> DomainResult<Option<String>> {
        let data = self
            .data
            .read()
            .map_err(|e| DomainError::ExecutionError(format!("获取读锁失败: {}", e)))?;
        Ok(data.get(key).cloned())
    }

    async fn delete(&self, key: &str) -> DomainResult<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| DomainError::ExecutionError(format!("获取写锁失败: {}", e)))?;
        data.remove(key);
        Ok(())
    }

    async fn save_batch(&self, items: &[(&str, &str)]) -> DomainResult<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| DomainError::ExecutionError(format!("获取写锁失败: {}", e)))?;
        for (key, value) in items {
            data.insert(key.to_string(), value.to_string());
        }
        Ok(())
    }

    async fn load_batch(&self, keys: &[&str]) -> DomainResult<Vec<(String, String)>> {
        let data = self
            .data
            .read()
            .map_err(|e| DomainError::ExecutionError(format!("获取读锁失败: {}", e)))?;
        let result = keys
            .iter()
            .filter_map(|key| data.get(*key).map(|value| (key.to_string(), value.clone())))
            .collect();
        Ok(result)
    }
}
