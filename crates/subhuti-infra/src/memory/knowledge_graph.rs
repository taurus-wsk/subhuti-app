//! # 时序知识图谱 (Temporal Knowledge Graph)
//!
//! 存储实体-关系三元组（subject-predicate-object），并支持时间有效性管理。
//!
//! ## 核心概念
//!
//! - **三元组 (Triple)**: 主语-谓词-宾语，描述实体间的关系
//! - **时间有效性**: 每条三元组都有 `valid_from`（生效时间）和可选的 `valid_to`（失效时间）
//! - **时序查询**: 支持按指定时间点查询当时有效的关系
//!
//! ## 数据库表
//!
//! 表名：`knowledge_triples`
//! - `id`: 主键
//! - `subject`: 主语实体
//! - `predicate`: 关系谓词
//! - `object`: 宾语实体
//! - `valid_from`: 生效时间
//! - `valid_to`: 失效时间（NULL 表示当前有效）
//! - `source`: 数据来源
//! - `created_at`: 创建时间

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::storage::Database;

/// 查询方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueryDirection {
    /// 出边：subject = entity
    Outgoing,
    /// 入边：object = entity
    Incoming,
    /// 双向：subject 或 object = entity
    #[default]
    Both,
}

impl std::str::FromStr for QueryDirection {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let result = match s.to_lowercase().as_str() {
            "outgoing" | "out" => QueryDirection::Outgoing,
            "incoming" | "in" => QueryDirection::Incoming,
            _ => QueryDirection::Both,
        };
        debug!(
            "KnowledgeGraph: QueryDirection::from_str input={}, result={:?}",
            s, result
        );
        Ok(result)
    }
}

/// 三元组：主语-谓词-宾语，带时间有效性
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Triple {
    /// 主键 ID
    pub id: i32,
    /// 主语实体
    pub subject: String,
    /// 谓词（关系）
    pub predicate: String,
    /// 宾语实体
    pub object: String,
    /// 生效时间
    pub valid_from: DateTime<Utc>,
    /// 失效时间（NULL 表示当前有效）
    pub valid_to: Option<DateTime<Utc>>,
    /// 数据来源
    pub source: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

/// 知识图谱统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGraphStats {
    /// 三元组总数
    pub total_triples: i64,
    /// 当前有效三元组数（valid_to IS NULL）
    pub active_triples: i64,
    /// 已失效三元组数
    pub invalidated_triples: i64,
    /// 不同主语实体数
    pub distinct_subjects: i64,
    /// 不同宾语实体数
    pub distinct_objects: i64,
    /// 不同谓词数
    pub distinct_predicates: i64,
}

/// 时序知识图谱
///
/// 基于 PostgreSQL 存储，支持时间有效性管理。
pub struct KnowledgeGraph {
    /// 数据库引用
    db: Arc<Database>,
}

impl KnowledgeGraph {
    /// 创建新的知识图谱实例
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 初始化数据库表和索引
    ///
    /// 创建 `knowledge_triples` 表以及常用查询字段的索引。
    pub async fn init_table(&self) -> Result<()> {
        let pool = self.db.pool();

        // 创建表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS knowledge_triples (
                id SERIAL PRIMARY KEY,
                subject VARCHAR(255) NOT NULL,
                predicate VARCHAR(255) NOT NULL,
                object VARCHAR(255) NOT NULL,
                valid_from TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                valid_to TIMESTAMPTZ,
                source VARCHAR(255),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(pool)
        .await?;

        // 创建索引：加速按主语查询
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_triples_subject ON knowledge_triples(subject)",
        )
        .execute(pool)
        .await?;

        // 创建索引：加速按宾语查询
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_triples_object ON knowledge_triples(object)",
        )
        .execute(pool)
        .await?;

        // 创建索引：加速按谓词查询
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_triples_predicate ON knowledge_triples(predicate)",
        )
        .execute(pool)
        .await?;

        // 创建索引：加速时序查询（valid_to 是否为 NULL）
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_triples_valid_to ON knowledge_triples(valid_to)",
        )
        .execute(pool)
        .await?;

        // 创建索引：加速按生效时间排序
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_triples_valid_from ON knowledge_triples(valid_from)",
        )
        .execute(pool)
        .await?;

        tracing::info!("KnowledgeGraph: 表 knowledge_triples 初始化完成");
        Ok(())
    }

    /// 添加一条三元组
    ///
    /// # 参数
    /// - `subject`: 主语实体
    /// - `predicate`: 谓词（关系）
    /// - `object`: 宾语实体
    /// - `source`: 数据来源（可选）
    /// - `valid_from`: 生效时间（可选，默认当前时间）
    ///
    /// # 返回
    /// 新插入三元组的 ID
    pub async fn add_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        source: Option<&str>,
        valid_from: Option<DateTime<Utc>>,
    ) -> Result<i32> {
        info!(
            "KnowledgeGraph: 开始添加三元组 subject={}, predicate={}, object={}",
            subject, predicate, object
        );
        let pool = self.db.pool();
        let valid_from = valid_from.unwrap_or_else(Utc::now);

        let row = sqlx::query(
            r#"
            INSERT INTO knowledge_triples (subject, predicate, object, valid_from, source)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(subject)
        .bind(predicate)
        .bind(object)
        .bind(valid_from)
        .bind(source)
        .fetch_one(pool)
        .await?;

        let id: i32 = row.get("id");

        info!(
            "KnowledgeGraph: 添加三元组成功 id={}, subject={}, predicate={}, object={}",
            id, subject, predicate, object
        );
        debug!(
            "KnowledgeGraph: 三元组详情 valid_from={}, source={:?}",
            valid_from, source
        );

        Ok(id)
    }

    /// 查询与某个实体相关的三元组
    ///
    /// # 参数
    /// - `entity`: 实体名称
    /// - `direction`: 查询方向（outgoing/incoming/both）
    /// - `as_of`: 时间点过滤（可选）。当提供时，只返回在该时间点有效的三元组：
    ///   `valid_from <= as_of AND (valid_to IS NULL OR valid_to >= as_of)`
    /// - `limit`: 返回数量上限
    ///
    /// # 返回
    /// 匹配的三元组列表，按 valid_from 倒序排列
    pub async fn query_entity(
        &self,
        entity: &str,
        direction: QueryDirection,
        as_of: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<Vec<Triple>> {
        info!(
            "KnowledgeGraph: 开始查询实体 entity={}, direction={:?}, as_of={:?}, limit={}",
            entity, direction, as_of, limit
        );
        let pool = self.db.pool();

        let dir_condition = match direction {
            QueryDirection::Outgoing => "(subject = $1)",
            QueryDirection::Incoming => "(object = $1)",
            QueryDirection::Both => "(subject = $1 OR object = $1)",
        };

        let sql = if as_of.is_some() {
            format!(
                r#"
                SELECT id, subject, predicate, object, valid_from, valid_to, source, created_at
                FROM knowledge_triples
                WHERE {}
                  AND valid_from <= $2
                  AND (valid_to IS NULL OR valid_to >= $2)
                ORDER BY valid_from DESC
                LIMIT $3
                "#,
                dir_condition
            )
        } else {
            format!(
                r#"
                SELECT id, subject, predicate, object, valid_from, valid_to, source, created_at
                FROM knowledge_triples
                WHERE {}
                ORDER BY valid_from DESC
                LIMIT $3
                "#,
                dir_condition
            )
        };

        let query = sqlx::query(&sql).bind(entity);

        let query = if let Some(t) = as_of {
            query.bind(t)
        } else {
            query.bind::<Option<DateTime<Utc>>>(None)
        };

        let rows = query.bind(limit).fetch_all(pool).await?;

        let mut result = Vec::new();
        for row in rows {
            result.push(Triple {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                valid_from: row.get("valid_from"),
                valid_to: row.get("valid_to"),
                source: row.get("source"),
                created_at: row.get("created_at"),
            });
        }

        info!(
            "KnowledgeGraph: 查询实体完成 entity={}, 命中 {} 条",
            entity,
            result.len()
        );
        debug!("KnowledgeGraph: 查询结果 {:?}", result);

        Ok(result)
    }

    /// 使一条三元组失效（设置 valid_to）
    ///
    /// # 参数
    /// - `triple_id`: 三元组 ID
    /// - `valid_to`: 失效时间（可选，默认当前时间）
    ///
    /// # 返回
    /// 受影响的行数（0 表示未找到或已失效）
    pub async fn invalidate(&self, triple_id: i32, valid_to: Option<DateTime<Utc>>) -> Result<u64> {
        info!("KnowledgeGraph: 开始失效三元组 id={}", triple_id);
        let pool = self.db.pool();
        let valid_to = valid_to.unwrap_or_else(Utc::now);

        let result = sqlx::query(
            r#"
            UPDATE knowledge_triples
            SET valid_to = $2
            WHERE id = $1 AND valid_to IS NULL
            "#,
        )
        .bind(triple_id)
        .bind(valid_to)
        .execute(pool)
        .await?;

        let affected = result.rows_affected();

        info!(
            "KnowledgeGraph: 失效三元组完成 id={}, valid_to={}, 受影响行数={}",
            triple_id, valid_to, affected
        );
        if affected == 0 {
            warn!("KnowledgeGraph: 三元组 id={} 未找到或已失效", triple_id);
        }

        Ok(affected)
    }

    /// 查询某个实体的时间线
    ///
    /// 返回该实体参与的所有三元组，按时间正序排列（从最早到最近）。
    ///
    /// # 参数
    /// - `entity`: 实体名称
    /// - `limit`: 返回数量上限
    pub async fn timeline(&self, entity: &str, limit: i64) -> Result<Vec<Triple>> {
        info!(
            "KnowledgeGraph: 开始查询实体时间线 entity={}, limit={}",
            entity, limit
        );
        let pool = self.db.pool();

        let rows = sqlx::query(
            r#"
            SELECT id, subject, predicate, object, valid_from, valid_to, source, created_at
            FROM knowledge_triples
            WHERE subject = $1 OR object = $1
            ORDER BY valid_from ASC
            LIMIT $2
            "#,
        )
        .bind(entity)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            result.push(Triple {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                valid_from: row.get("valid_from"),
                valid_to: row.get("valid_to"),
                source: row.get("source"),
                created_at: row.get("created_at"),
            });
        }

        info!(
            "KnowledgeGraph: 实体时间线查询完成 entity={}, 共 {} 条记录",
            entity,
            result.len()
        );

        Ok(result)
    }

    /// 获取知识图谱统计信息
    pub async fn stats(&self) -> Result<KnowledgeGraphStats> {
        info!("KnowledgeGraph: 开始获取统计信息");
        let pool = self.db.pool();

        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) AS total_triples,
                COUNT(*) FILTER (WHERE valid_to IS NULL) AS active_triples,
                COUNT(*) FILTER (WHERE valid_to IS NOT NULL) AS invalidated_triples,
                COUNT(DISTINCT subject) AS distinct_subjects,
                COUNT(DISTINCT object) AS distinct_objects,
                COUNT(DISTINCT predicate) AS distinct_predicates
            FROM knowledge_triples
            "#,
        )
        .fetch_one(pool)
        .await?;

        let stats = KnowledgeGraphStats {
            total_triples: row.get("total_triples"),
            active_triples: row.get("active_triples"),
            invalidated_triples: row.get("invalidated_triples"),
            distinct_subjects: row.get("distinct_subjects"),
            distinct_objects: row.get("distinct_objects"),
            distinct_predicates: row.get("distinct_predicates"),
        };

        info!(
            "KnowledgeGraph: 统计完成 总数={}, 有效={}, 失效={}, 主语数={}, 宾语数={}, 谓词数={}",
            stats.total_triples,
            stats.active_triples,
            stats.invalidated_triples,
            stats.distinct_subjects,
            stats.distinct_objects,
            stats.distinct_predicates
        );

        Ok(stats)
    }

    /// 根据主语和谓词查询当前有效的宾语列表
    ///
    /// 便捷方法：等价于 `query_entity(subject, Outgoing, Some(now), limit)`
    /// 但只返回宾语字符串。
    pub async fn get_objects(
        &self,
        subject: &str,
        predicate: &str,
        limit: i64,
    ) -> Result<Vec<String>> {
        info!(
            "KnowledgeGraph: 开始查询宾语 subject={}, predicate={}, limit={}",
            subject, predicate, limit
        );
        let pool = self.db.pool();

        let rows = sqlx::query(
            r#"
            SELECT object
            FROM knowledge_triples
            WHERE subject = $1
              AND predicate = $2
              AND valid_to IS NULL
            ORDER BY valid_from DESC
            LIMIT $3
            "#,
        )
        .bind(subject)
        .bind(predicate)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let result: Vec<String> = rows.into_iter().map(|r| r.get("object")).collect();

        info!(
            "KnowledgeGraph: 查询宾语完成 subject={}, predicate={}, 命中 {} 条",
            subject,
            predicate,
            result.len()
        );
        debug!("KnowledgeGraph: 查询宾语结果 {:?}", result);

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_query_direction_from_str() {
        assert_eq!(
            QueryDirection::from_str("outgoing").unwrap(),
            QueryDirection::Outgoing
        );
        assert_eq!(
            QueryDirection::from_str("OUT").unwrap(),
            QueryDirection::Outgoing
        );
        assert_eq!(
            QueryDirection::from_str("incoming").unwrap(),
            QueryDirection::Incoming
        );
        assert_eq!(
            QueryDirection::from_str("IN").unwrap(),
            QueryDirection::Incoming
        );
        assert_eq!(
            QueryDirection::from_str("both").unwrap(),
            QueryDirection::Both
        );
        assert_eq!(
            QueryDirection::from_str("unknown").unwrap(),
            QueryDirection::Both
        );
    }

    #[test]
    fn test_query_direction_default() {
        assert_eq!(QueryDirection::default(), QueryDirection::Both);
    }

    #[test]
    fn test_triple_struct_fields() {
        // 验证 Triple 结构体字段可以通过序列化/反序列化往返
        let now = Utc::now();
        let triple = Triple {
            id: 1,
            subject: "alice".to_string(),
            predicate: "knows".to_string(),
            object: "bob".to_string(),
            valid_from: now,
            valid_to: None,
            source: Some("test".to_string()),
            created_at: now,
        };

        let json = serde_json::to_string(&triple).unwrap();
        assert!(json.contains("alice"));
        assert!(json.contains("knows"));
        assert!(json.contains("bob"));

        let deserialized: Triple = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, 1);
        assert_eq!(deserialized.subject, "alice");
        assert_eq!(deserialized.predicate, "knows");
        assert_eq!(deserialized.object, "bob");
        assert_eq!(deserialized.source, Some("test".to_string()));
    }

    #[test]
    fn test_triple_with_null_valid_to() {
        let now = Utc::now();
        let triple = Triple {
            id: 2,
            subject: "entity_a".to_string(),
            predicate: "relates_to".to_string(),
            object: "entity_b".to_string(),
            valid_from: now,
            valid_to: None,
            source: None,
            created_at: now,
        };

        let json = serde_json::to_string(&triple).unwrap();
        assert!(json.contains("\"valid_to\":null"));
    }

    #[test]
    fn test_stats_struct_serialization() {
        let stats = KnowledgeGraphStats {
            total_triples: 100,
            active_triples: 80,
            invalidated_triples: 20,
            distinct_subjects: 50,
            distinct_objects: 60,
            distinct_predicates: 10,
        };

        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: KnowledgeGraphStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_triples, 100);
        assert_eq!(deserialized.active_triples, 80);
        assert_eq!(deserialized.invalidated_triples, 20);
    }
}
