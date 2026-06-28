//! # 数据库模块
//!
//! PostgreSQL 数据库集成，支持 pgvector 扩展。

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::collections::HashMap;

/// 数据库配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: u32,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            username: "postgres".to_string(),
            password: "123456".to_string(),
            max_connections: 10,
        }
    }
}

impl DbConfig {
    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.database
        )
    }
}

/// 数据库连接池
#[derive(Debug, Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn new(config: &DbConfig) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.connection_string())
            .await?;

        tracing::info!(
            "Database: Connected to PostgreSQL at {}:{}",
            config.host,
            config.port
        );
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 初始化表结构
    pub async fn init_tables(&self) -> Result<()> {
        // 启用 pgvector 扩展
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&self.pool)
            .await?;
        tracing::info!("Database: pgvector extension enabled");

        // persona_profiles 表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS persona_profiles (
                id SERIAL PRIMARY KEY,
                user_id VARCHAR(255) NOT NULL UNIQUE,
                version INTEGER NOT NULL DEFAULT 1,
                name VARCHAR(255) NOT NULL,
                description TEXT,
                tone VARCHAR(50) NOT NULL,
                emotional_tendency VARCHAR(50) NOT NULL,
                openness REAL NOT NULL DEFAULT 0.6,
                conscientiousness REAL NOT NULL DEFAULT 0.5,
                extraversion REAL NOT NULL DEFAULT 0.5,
                agreeableness REAL NOT NULL DEFAULT 0.7,
                neuroticism REAL NOT NULL DEFAULT 0.4,
                traits JSONB NOT NULL DEFAULT '[]',
                skill_proficiency JSONB NOT NULL DEFAULT '{}',
                expertise_areas JSONB NOT NULL DEFAULT '{}',
                skill_affinity JSONB NOT NULL DEFAULT '{}',
                total_interactions INTEGER NOT NULL DEFAULT 0,
                likes INTEGER NOT NULL DEFAULT 0,
                dislikes INTEGER NOT NULL DEFAULT 0,
                avg_response_time_ms BIGINT NOT NULL DEFAULT 0,
                skill_usage JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // persona_history 表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS persona_history (
                id SERIAL PRIMARY KEY,
                user_id VARCHAR(255) NOT NULL,
                version INTEGER NOT NULL,
                profile_snapshot JSONB NOT NULL,
                reason TEXT NOT NULL,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // user_feedbacks 表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_feedbacks (
                id SERIAL PRIMARY KEY,
                user_id VARCHAR(255) NOT NULL,
                feedback_type VARCHAR(20) NOT NULL,
                content TEXT,
                skill_name VARCHAR(255) NOT NULL,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // memories 表（支持向量存储）
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memories (
                id SERIAL PRIMARY KEY,
                user_id VARCHAR(255) NOT NULL,
                session_id VARCHAR(255),
                role VARCHAR(20) NOT NULL,
                content TEXT NOT NULL,
                metadata JSONB NOT NULL DEFAULT '{}',
                layer VARCHAR(20) NOT NULL DEFAULT 'short_term',
                embedding vector(1024),
                archived BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 迁移：确保 memories 表有新字段（在创建索引之前）
        self.migrate_memories_table().await?;

        // 索引
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_user_id ON memories(user_id)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_layer ON memories(layer)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_archived ON memories(archived)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_feedbacks_user_id ON user_feedbacks(user_id)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_history_user_id ON persona_history(user_id)")
            .execute(&self.pool)
            .await?;

        tracing::info!("Database: Tables initialized successfully");
        Ok(())
    }

    /// 迁移 memories 表，添加缺失的列
    async fn migrate_memories_table(&self) -> Result<()> {
        // 添加 session_id 列
        let result =
            sqlx::query("ALTER TABLE memories ADD COLUMN IF NOT EXISTS session_id VARCHAR(255)")
                .execute(&self.pool)
                .await;
        if let Err(e) = result {
            tracing::debug!("Migration: session_id column may already exist: {}", e);
        }

        // 添加 layer 列
        let result = sqlx::query(
            "ALTER TABLE memories ADD COLUMN IF NOT EXISTS layer VARCHAR(20) NOT NULL DEFAULT 'short_term'"
        )
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::debug!("Migration: layer column may already exist: {}", e);
        }

        // 添加 embedding 列
        let result =
            sqlx::query("ALTER TABLE memories ADD COLUMN IF NOT EXISTS embedding vector(1024)")
                .execute(&self.pool)
                .await;
        if let Err(e) = result {
            tracing::debug!("Migration: embedding column may already exist: {}", e);
        }

        // 如果 embedding 列存在但维度不对，尝试修改维度（需要先删除再重建，因为 pgvector 不支持 ALTER TYPE）
        // 这里只打印警告，实际生产中应该用更安全的迁移方式
        let check_result = sqlx::query(
            "SELECT atttypmod FROM pg_attribute WHERE attname = 'embedding' AND attrelid = 'memories'::regclass"
        )
        .fetch_optional(&self.pool)
        .await;

        if let Ok(Some(row)) = check_result {
            let typmod: i32 = row.get("atttypmod");
            if typmod > 0 {
                // typmod 中存储了维度信息
                // pgvector 的 typmod 编码：(dimensions << 16) + (typmod & 0xFFFF)
                // 简化处理：如果维度不是 1024，删除后重建
                let dims = (typmod >> 16) & 0xFFFF;
                if dims != 0 && dims != 1024 {
                    tracing::warn!("Embedding dimension mismatch: found {}d, expected 1024d. Recreating column...", dims);
                    let _ = sqlx::query("ALTER TABLE memories DROP COLUMN IF EXISTS embedding")
                        .execute(&self.pool)
                        .await;
                    let _ = sqlx::query("ALTER TABLE memories ADD COLUMN embedding vector(1024)")
                        .execute(&self.pool)
                        .await;
                    tracing::info!("Embedding column recreated with 1024 dimensions");
                }
            }
        }

        tracing::info!("Database: Memories table migration completed");
        Ok(())
    }

    // ── Persona CRUD ──────────────────────────────────────────

    pub async fn get_persona(&self, user_id: &str) -> Result<Option<PersonaRow>> {
        let row = sqlx::query(
            r#"
            SELECT 
                id, user_id, version, name, description, tone, emotional_tendency,
                openness, conscientiousness, extraversion, agreeableness, neuroticism,
                traits, skill_proficiency, expertise_areas, skill_affinity,
                total_interactions, likes, dislikes, avg_response_time_ms, skill_usage,
                created_at, updated_at
            FROM persona_profiles
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            Ok(Some(PersonaRow {
                id: r.get("id"),
                user_id: r.get("user_id"),
                version: r.get("version"),
                name: r.get("name"),
                description: r.get("description"),
                tone: r.get("tone"),
                emotional_tendency: r.get("emotional_tendency"),
                openness: r.get("openness"),
                conscientiousness: r.get("conscientiousness"),
                extraversion: r.get("extraversion"),
                agreeableness: r.get("agreeableness"),
                neuroticism: r.get("neuroticism"),
                traits: r.get("traits"),
                skill_proficiency: r.get("skill_proficiency"),
                expertise_areas: r.get("expertise_areas"),
                skill_affinity: r.get("skill_affinity"),
                total_interactions: r.get("total_interactions"),
                likes: r.get("likes"),
                dislikes: r.get("dislikes"),
                avg_response_time_ms: r.get("avg_response_time_ms"),
                skill_usage: r.get("skill_usage"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn upsert_persona(&self, user_id: &str, profile: &PersonaData) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO persona_profiles (
                user_id, version, name, description, tone, emotional_tendency,
                openness, conscientiousness, extraversion, agreeableness, neuroticism,
                traits, skill_proficiency, expertise_areas, skill_affinity,
                total_interactions, likes, dislikes, avg_response_time_ms, skill_usage,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, NOW())
            ON CONFLICT (user_id) DO UPDATE SET
                version = EXCLUDED.version,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                tone = EXCLUDED.tone,
                emotional_tendency = EXCLUDED.emotional_tendency,
                openness = EXCLUDED.openness,
                conscientiousness = EXCLUDED.conscientiousness,
                extraversion = EXCLUDED.extraversion,
                agreeableness = EXCLUDED.agreeableness,
                neuroticism = EXCLUDED.neuroticism,
                traits = EXCLUDED.traits,
                skill_proficiency = EXCLUDED.skill_proficiency,
                expertise_areas = EXCLUDED.expertise_areas,
                skill_affinity = EXCLUDED.skill_affinity,
                total_interactions = EXCLUDED.total_interactions,
                likes = EXCLUDED.likes,
                dislikes = EXCLUDED.dislikes,
                avg_response_time_ms = EXCLUDED.avg_response_time_ms,
                skill_usage = EXCLUDED.skill_usage,
                updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(profile.version)
        .bind(&profile.name)
        .bind(&profile.description)
        .bind(&profile.tone)
        .bind(&profile.emotional_tendency)
        .bind(profile.openness)
        .bind(profile.conscientiousness)
        .bind(profile.extraversion)
        .bind(profile.agreeableness)
        .bind(profile.neuroticism)
        .bind(serde_json::to_value(&profile.traits)?)
        .bind(serde_json::to_value(&profile.skill_proficiency)?)
        .bind(serde_json::to_value(&profile.expertise_areas)?)
        .bind(serde_json::to_value(&profile.skill_affinity)?)
        .bind(profile.total_interactions)
        .bind(profile.likes)
        .bind(profile.dislikes)
        .bind(profile.avg_response_time_ms)
        .bind(serde_json::to_value(&profile.skill_usage)?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn add_history(
        &self,
        user_id: &str,
        version: i32,
        snapshot: &serde_json::Value,
        reason: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO persona_history (user_id, version, profile_snapshot, reason)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(user_id)
        .bind(version)
        .bind(snapshot)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ── Feedback CRUD ──────────────────────────────────────────

    pub async fn add_feedback(
        &self,
        user_id: &str,
        feedback_type: &str,
        content: &str,
        skill_name: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO user_feedbacks (user_id, feedback_type, content, skill_name)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(user_id)
        .bind(feedback_type)
        .bind(content)
        .bind(skill_name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_feedbacks(&self, user_id: &str, limit: i32) -> Result<Vec<FeedbackRow>> {
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, feedback_type, content, skill_name, created_at
            FROM user_feedbacks
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| FeedbackRow {
                id: r.get("id"),
                user_id: r.get("user_id"),
                feedback_type: r.get("feedback_type"),
                content: r.get("content"),
                skill_name: r.get("skill_name"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    // ── Memory CRUD ──────────────────────────────────────────

    pub async fn add_memory(
        &self,
        user_id: &str,
        session_id: Option<&str>,
        role: &str,
        content: &str,
        layer: &str,
        metadata: Option<&serde_json::Value>,
    ) -> Result<i64> {
        let row = sqlx::query(
            r#"
            INSERT INTO memories (user_id, session_id, role, content, layer, metadata)
            VALUES ($1, $2, $3, $4, $5, COALESCE($6, '{}'::jsonb))
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(layer)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get::<i64, _>("id"))
    }

    pub async fn get_recent_memories(
        &self,
        user_id: &str,
        layer: &str,
        limit: i32,
    ) -> Result<Vec<MemoryRow>> {
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, session_id, role, content, metadata, layer, archived, created_at
            FROM memories
            WHERE user_id = $1 AND layer = $2 AND archived = FALSE
            ORDER BY created_at DESC
            LIMIT $3
            "#,
        )
        .bind(user_id)
        .bind(layer)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| MemoryRow {
                id: r.get("id"),
                user_id: r.get("user_id"),
                session_id: r.get("session_id"),
                role: r.get("role"),
                content: r.get("content"),
                metadata: r.get("metadata"),
                layer: r.get("layer"),
                archived: r.get("archived"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn archive_memories_by_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE memories
            SET archived = TRUE, layer = 'archive'
            WHERE user_id = $1 AND session_id = $2 AND layer = 'short_term'
            "#,
        )
        .bind(user_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn search_memories_text(
        &self,
        user_id: &str,
        query: &str,
        limit: i32,
    ) -> Result<Vec<MemoryRow>> {
        let search_pattern = format!("%{}%", query);
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, session_id, role, content, metadata, layer, archived, created_at
            FROM memories
            WHERE user_id = $1 AND content ILIKE $2
            ORDER BY created_at DESC
            LIMIT $3
            "#,
        )
        .bind(user_id)
        .bind(search_pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| MemoryRow {
                id: r.get("id"),
                user_id: r.get("user_id"),
                session_id: r.get("session_id"),
                role: r.get("role"),
                content: r.get("content"),
                metadata: r.get("metadata"),
                layer: r.get("layer"),
                archived: r.get("archived"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn count_memories(&self, user_id: &str, layer: &str) -> Result<i64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM memories
            WHERE user_id = $1 AND layer = $2 AND archived = FALSE
            "#,
        )
        .bind(user_id)
        .bind(layer)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get::<i64, _>("count"))
    }

    /// 更新记忆的 embedding 向量
    pub async fn update_embedding(&self, memory_id: i64, embedding_str: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE memories
            SET embedding = $1::vector
            WHERE id = $2
            "#,
        )
        .bind(embedding_str)
        .bind(memory_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 向量相似度搜索（余弦距离，值越小越相似）
    pub async fn search_semantic(
        &self,
        user_id: &str,
        query_embedding_str: &str,
        limit: i32,
    ) -> Result<Vec<(MemoryRow, f32)>> {
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, session_id, role, content, metadata, layer, archived, created_at,
                   1 - (embedding <=> $2::vector) as similarity
            FROM memories
            WHERE user_id = $1 AND embedding IS NOT NULL
            ORDER BY embedding <=> $2::vector
            LIMIT $3
            "#,
        )
        .bind(user_id)
        .bind(query_embedding_str)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let memory = MemoryRow {
                    id: r.get("id"),
                    user_id: r.get("user_id"),
                    session_id: r.get("session_id"),
                    role: r.get("role"),
                    content: r.get("content"),
                    metadata: r.get("metadata"),
                    layer: r.get("layer"),
                    archived: r.get("archived"),
                    created_at: r.get("created_at"),
                };
                let similarity: f64 = r.get("similarity");
                (memory, similarity as f32)
            })
            .collect())
    }

    pub async fn list_users(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT user_id FROM persona_profiles ORDER BY user_id")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|r| r.get("user_id")).collect())
    }
}

// ─── 数据结构定义 ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaData {
    pub version: i32,
    pub name: String,
    pub description: String,
    pub tone: String,
    pub emotional_tendency: String,
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
    pub traits: Vec<String>,
    pub skill_proficiency: HashMap<String, f32>,
    pub expertise_areas: HashMap<String, f32>,
    pub skill_affinity: HashMap<String, f32>,
    pub total_interactions: i32,
    pub likes: i32,
    pub dislikes: i32,
    pub avg_response_time_ms: i64,
    pub skill_usage: HashMap<String, i32>,
}

#[derive(Debug, Clone)]
pub struct PersonaRow {
    pub id: i32,
    pub user_id: String,
    pub version: i32,
    pub name: String,
    pub description: String,
    pub tone: String,
    pub emotional_tendency: String,
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
    pub traits: serde_json::Value,
    pub skill_proficiency: serde_json::Value,
    pub expertise_areas: serde_json::Value,
    pub skill_affinity: serde_json::Value,
    pub total_interactions: i32,
    pub likes: i32,
    pub dislikes: i32,
    pub avg_response_time_ms: i64,
    pub skill_usage: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub id: i32,
    pub user_id: String,
    pub version: i32,
    pub profile_snapshot: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct FeedbackRow {
    pub id: i32,
    pub user_id: String,
    pub feedback_type: String,
    pub content: Option<String>,
    pub skill_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MemoryRow {
    pub id: i64,
    pub user_id: String,
    pub session_id: Option<String>,
    pub role: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub layer: String,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
}
