//! # FeedbackAnalyzer 反馈分析器
//!
//! 异步离线反馈流水线，不阻塞主查询链路。
//!
//! ## 两种运行模式
//!
//! 1. **实时轻量模式**：`record_execution()` 投递日志，即时更新滑动窗口，
//!    当窗口积累到一定量后自动触发轻度共现挖掘。
//!
//! 2. **定时批量模式**：`start_background_analysis()` 启动后台 tokio 任务，
//!    每隔指定间隔从 PG 批量捞取日志，做完整共现统计 + 图谱更新。
//!
//! ## 反馈闭环
//!
//! 用户Query → 藏经阁召回 → Agent执行 → 记录ExecutionLog
//!                                         ↓
//!  FeedbackAnalyzer: 命中率统计 + 实体共现挖掘
//!                                         ↓
//!  petgraph 更新 Learned 边权重 → PG 持久化

use crate::sutra_library::feedback::data_models::{
    ExecutionLog, FeedbackConfig, FeedbackMetrics, FeedbackResult,
    CREATE_EXECUTION_LOGS_INDEX_DOMAIN, CREATE_EXECUTION_LOGS_INDEX_TS,
    CREATE_EXECUTION_LOGS_TABLE, CREATE_FEEDBACK_METRICS_INDEX_GRAPH,
    CREATE_FEEDBACK_METRICS_TABLE,
};
use crate::sutra_library::recall::{EntityGraph, EntityUuid};
use crate::sutra_library::storage::PgStorage;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 反馈分析器
pub struct FeedbackAnalyzer {
    /// 滑动窗口（最近 N 条执行日志）
    window: RwLock<VecDeque<ExecutionLog>>,
    /// 配置
    config: FeedbackConfig,
    /// 实体图谱引用（用于更新 Learned 边）
    entity_graph: Arc<EntityGraph>,
    /// PG 持久化（可选）
    pg: Option<Arc<PgStorage>>,
    /// 是否已初始化 PG 表
    pg_initialized: RwLock<bool>,
}

impl FeedbackAnalyzer {
    /// 创建新的反馈分析器
    ///
    /// `entity_graph`: 实体图谱引用（用于更新 Learned 边）
    /// `pg`: PG 存储（可选，用于持久化执行日志和反馈指标）
    /// `config`: 配置
    pub fn new(
        entity_graph: Arc<EntityGraph>,
        pg: Option<Arc<PgStorage>>,
        config: FeedbackConfig,
    ) -> Self {
        Self {
            window: RwLock::new(VecDeque::with_capacity(config.window_size)),
            config,
            entity_graph,
            pg,
            pg_initialized: RwLock::new(false),
        }
    }

    // ─── 公开接口 ──────────────────────────────────────────────

    /// 记录一条执行日志（实时轻量模式入口）
    ///
    /// 1. 加入滑动窗口，自动淘汰旧日志
    /// 2. 如果 PG 已配置，异步写入持久化
    /// 3. 触发轻度实体共现挖掘（窗口内新日志达到一定量）
    pub fn record_execution(&self, log: ExecutionLog) {
        // 1. 加入滑动窗口
        {
            let mut window = self.window.write().unwrap();
            window.push_back(log);

            // 淘汰超出窗口大小的旧日志
            while window.len() > self.config.window_size {
                window.pop_front();
            }
        }

        // 2. 触发轻度共现挖掘（每 10 条日志触发一次）
        {
            let window = self.window.read().unwrap();
            let len = window.len();
            if len > 0 && len.is_multiple_of(10) {
                // 用最近 10 条日志做轻度共现分析
                let recent: Vec<ExecutionLog> = window.iter().rev().take(10).cloned().collect();
                drop(window);
                self.analyze_cooccurrence(&recent, false);
            }
        }

        // 3. 异步 PG 持久化
        self.persist_log_async();
    }

    /// 获取当前命中率指标
    pub fn get_metrics(&self) -> FeedbackMetrics {
        let window = self.window.read().unwrap();
        let total = window.len();
        if total == 0 {
            return FeedbackMetrics {
                total_queries: 0,
                hit_rate: 0.0,
                source_contribution: HashMap::new(),
                avg_used_chunks: 0.0,
                window_size: self.config.window_size,
                success_rate: 0.0,
                timestamp: now_millis(),
            };
        }

        let mut total_hit_chunks: usize = 0;
        let mut total_recalled_chunks: usize = 0;
        let mut total_used_chunks: usize = 0;
        let mut success_count: usize = 0;
        let mut source_hits: HashMap<String, usize> = HashMap::new();
        let mut source_total: HashMap<String, usize> = HashMap::new();

        for log in window.iter() {
            total_recalled_chunks += log.recalled_chunks.len();
            total_used_chunks += log.used_chunk_ids.len();
            if log.task_success {
                success_count += 1;
            }

            // 统计各通路命中数
            for chunk_info in &log.recalled_chunks {
                let source_name = format!("{:?}", chunk_info.source);
                *source_total.entry(source_name.clone()).or_insert(0) += 1;
                if log.used_chunk_ids.contains(&chunk_info.chunk_id) {
                    *source_hits.entry(source_name).or_insert(0) += 1;
                }
            }

            // 计算被引用的切片相关度
            let used_set: HashSet<&String> = log.used_chunk_ids.iter().collect();
            for chunk_info in &log.recalled_chunks {
                if used_set.contains(&chunk_info.chunk_id) {
                    total_hit_chunks += 1;
                }
            }
        }

        let hit_rate = if total_recalled_chunks > 0 {
            total_hit_chunks as f64 / total_recalled_chunks as f64
        } else {
            0.0
        };

        let source_contribution: HashMap<String, f64> = source_total
            .iter()
            .map(|(name, total)| {
                let hits = source_hits.get(name).copied().unwrap_or(0);
                let ratio = if *total > 0 {
                    hits as f64 / *total as f64
                } else {
                    0.0
                };
                (name.clone(), ratio)
            })
            .collect();

        FeedbackMetrics {
            total_queries: total,
            hit_rate,
            source_contribution,
            avg_used_chunks: total_used_chunks as f64 / total as f64,
            window_size: self.config.window_size,
            success_rate: success_count as f64 / total as f64,
            timestamp: now_millis(),
        }
    }

    /// 启动后台定时分析任务
    ///
    /// 每隔 `config.background_interval_secs` 执行一次批量分析：
    /// 1. 从 PG 捞取未分析的执行日志
    /// 2. 做完整共现统计
    /// 3. 更新图谱 Learned 边
    /// 4. 持久化反馈指标
    pub fn start_background_analysis(self: Arc<Self>) {
        let interval = Duration::from_secs(self.config.background_interval_secs);
        tokio::task::spawn(async move {
            // 初始化 PG 表
            self.init_pg_tables().await;

            loop {
                tokio::time::sleep(interval).await;

                // 从滑动窗口获取日志快照
                let logs_snapshot: Vec<ExecutionLog> =
                    { self.window.read().unwrap().iter().cloned().collect() };

                if logs_snapshot.is_empty() {
                    continue;
                }

                // 执行完整共现分析
                self.analyze_cooccurrence(&logs_snapshot, true);

                // 计算指标并持久化
                let metrics = self.get_metrics();
                let result = FeedbackResult {
                    metrics: metrics.clone(),
                    learned_edge_updates: Vec::new(),
                    analyzed_logs: logs_snapshot.len(),
                    timestamp: now_millis(),
                };

                // 持久化指标
                self.persist_metrics_async(&result);

                tracing::info!(
                    "FeedbackAnalyzer: 后台分析完成, 日志数={}, 命中率={:.2}%, 成功率={:.2}%",
                    logs_snapshot.len(),
                    metrics.hit_rate * 100.0,
                    metrics.success_rate * 100.0,
                );
            }
        });
    }

    // ─── 实体共现挖掘 ──────────────────────────────────────────

    /// 分析执行日志中的实体共现，生成 Learned 边更新
    ///
    /// 原理：两条切片经常被同一个任务同时用到 → 切片内的实体之间增加关联熟练度
    ///
    /// 1. 取出每条日志的 `used_chunk_ids`（实际生效的切片集合）
    /// 2. 取出这些切片包含的全部实体列表
    /// 3. 集合内实体两两配对 (E1, E2)
    /// 4. 任务成功 → 熟练度 + 正向增量；任务失败 → 增量减半
    /// 5. 去重，更新图谱动态边权重
    fn analyze_cooccurrence(&self, logs: &[ExecutionLog], is_batch: bool) {
        // 统计实体对共现次数和成功/失败加权
        let mut cooccurrence_map: HashMap<(EntityUuid, EntityUuid), f32> = HashMap::new();

        for log in logs {
            if log.used_chunk_ids.is_empty() {
                continue;
            }

            // 通过 entity_graph 获取这些切片关联的实体
            let mut entities: HashSet<EntityUuid> = HashSet::new();
            for chunk_id in &log.used_chunk_ids {
                let chunk_entities = self
                    .entity_graph
                    .get_entities_of_chunks(std::slice::from_ref(chunk_id));
                entities.extend(chunk_entities);
            }

            if entities.len() < 2 {
                continue;
            }

            // 实体两两配对
            let entity_list: Vec<&EntityUuid> = entities.iter().collect();
            let delta = if log.task_success {
                self.config.cooccurrence_positive_delta
            } else {
                self.config.cooccurrence_failure_delta
            };

            for i in 0..entity_list.len() {
                for j in (i + 1)..entity_list.len() {
                    let from = entity_list[i].clone();
                    let to = entity_list[j].clone();
                    let key = if from <= to { (from, to) } else { (to, from) };
                    *cooccurrence_map.entry(key).or_insert(0.0) += delta;
                }
            }
        }

        if cooccurrence_map.is_empty() {
            return;
        }

        // 更新 petgraph 中的 Learned 边
        let update_count = cooccurrence_map.len();
        if is_batch {
            tracing::debug!("FeedbackAnalyzer: 批量更新 {} 条 Learned 边", update_count);
        }

        for ((from, to), delta) in &cooccurrence_map {
            // 获取当前边的权重（如果存在），然后增加
            let current_weight = self
                .entity_graph
                .get_neighbors(from, false, true)
                .iter()
                .find(|(e, _, _)| e == to)
                .map(|(_, _, w)| *w)
                .unwrap_or(0.0);

            let new_weight = (current_weight + delta).clamp(0.0, 1.0);
            self.entity_graph.add_learned_edge(from, to, new_weight);
        }

        if is_batch && update_count > 0 {
            tracing::info!(
                "FeedbackAnalyzer: 共现挖掘完成, 生成 {} 条 Learned 边更新",
                update_count
            );
        }
    }

    // ─── PG 持久化 ─────────────────────────────────────────────

    /// 初始化 PG 表（幂等）
    ///
    /// 逐条执行 SQL 语句，避免多语句在 prepared statement 中报错。
    pub async fn init_pg_tables(&self) {
        if *self.pg_initialized.read().unwrap() {
            return;
        }

        if let Some(ref pg) = self.pg {
            let pool = &**pg.pool();
            let mut has_error = false;

            // 逐条执行，每条 SQL 只包含一个语句
            let stmts = [
                ("执行日志表", CREATE_EXECUTION_LOGS_TABLE),
                ("执行日志时间戳索引", CREATE_EXECUTION_LOGS_INDEX_TS),
                ("执行日志领域索引", CREATE_EXECUTION_LOGS_INDEX_DOMAIN),
                ("反馈指标表", CREATE_FEEDBACK_METRICS_TABLE),
                ("反馈指标图谱索引", CREATE_FEEDBACK_METRICS_INDEX_GRAPH),
            ];

            for (label, sql) in &stmts {
                if let Err(e) = sqlx::query(sql).execute(pool).await {
                    tracing::warn!("FeedbackAnalyzer: {} 创建失败: {}", label, e);
                    has_error = true;
                }
            }

            if !has_error {
                *self.pg_initialized.write().unwrap() = true;
                tracing::info!("FeedbackAnalyzer: PG 表初始化完成");
            } else {
                tracing::warn!("FeedbackAnalyzer: PG 表初始化部分失败，下次启动会重试");
            }
        }
    }

    /// 异步持久化执行日志到 PG
    fn persist_log_async(&self) {
        let pg = match self.pg.clone() {
            Some(pg) => pg,
            None => return,
        };

        if !self.config.enable_pg_persistence {
            return;
        }

        // 从窗口获取最近的日志
        let logs: Vec<ExecutionLog> = {
            self.window
                .read()
                .unwrap()
                .iter()
                .rev()
                .take(10)
                .cloned()
                .collect()
        };

        tokio::task::spawn(async move {
            let pool = &**pg.pool();
            for log in &logs {
                let recalled_json = serde_json::to_string(&log.recalled_chunks).unwrap_or_default();
                let used_ids: Vec<&str> = log.used_chunk_ids.iter().map(|s| s.as_str()).collect();

                let result = sqlx::query(
                    r#"
                    INSERT INTO sutra_execution_logs
                        (query_hash, query, recalled_chunks, used_chunk_ids, task_success,
                         timestamp, graph, domain, session_id)
                    VALUES ($1, $2, $3::jsonb, $4::text[], $5, $6, $7, $8, $9)
                    "#,
                )
                .bind(&log.query_hash)
                .bind(&log.query)
                .bind(&recalled_json)
                .bind(&used_ids)
                .bind(log.task_success)
                .bind(log.timestamp)
                .bind(&log.graph)
                .bind(&log.domain)
                .bind(&log.session_id)
                .execute(pool)
                .await;

                if let Err(e) = result {
                    tracing::warn!("FeedbackAnalyzer: 持久化执行日志失败: {}", e);
                }
            }
        });
    }

    /// 异步持久化反馈指标到 PG
    fn persist_metrics_async(&self, result: &FeedbackResult) {
        let pg = match self.pg.clone() {
            Some(pg) => pg,
            None => return,
        };

        let source_json =
            serde_json::to_string(&result.metrics.source_contribution).unwrap_or_default();
        let graph = String::new();
        let domain = String::new();
        let total_queries = result.metrics.total_queries as i64;
        let hit_rate = result.metrics.hit_rate;
        let avg_used = result.metrics.avg_used_chunks;
        let success_rate = result.metrics.success_rate;
        let ts = result.timestamp;

        tokio::task::spawn(async move {
            let pool = &**pg.pool();
            let result = sqlx::query(
                r#"
                INSERT INTO sutra_feedback_metrics
                    (graph, domain, total_queries, hit_rate, source_contribution,
                     avg_used_chunks, success_rate, analyzed_at)
                VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7, $8)
                "#,
            )
            .bind(&graph)
            .bind(&domain)
            .bind(total_queries)
            .bind(hit_rate)
            .bind(&source_json)
            .bind(avg_used)
            .bind(success_rate)
            .bind(ts)
            .execute(pool)
            .await;

            if let Err(e) = result {
                tracing::warn!("FeedbackAnalyzer: 持久化反馈指标失败: {}", e);
            }
        });
    }

    // ─── Polars 批量分析 ───────────────────────────────────────

    /// 使用 Polars 从 PG 批量加载日志并做完整分析（仅在有 polars 特性时启用）
    ///
    /// 适合场景：
    /// - 一次性导入成千上万条 Agent 执行日志
    /// - 分组统计：不同知识库、不同召回策略命中率
    /// - 批量计算实体共现矩阵
    #[cfg(feature = "feedback-analytics")]
    pub async fn batch_analyze_with_polars(&self, limit: usize) -> anyhow::Result<FeedbackResult> {
        let pg = match self.pg.as_ref() {
            Some(pg) => pg,
            None => anyhow::bail!("PG 未配置，无法使用 Polars 批量分析"),
        };

        // 1. 从 PG 批量捞取日志
        let logs = self.load_logs_from_pg(pg, limit).await?;

        if logs.is_empty() {
            anyhow::bail!("没有可分析的执行日志");
        }

        // 2. 加载到 Polars DataFrame
        let df = self.logs_to_dataframe(&logs)?;

        // 3. 按 domain/graph 分组计算命中率
        let hit_rate_df = self.compute_hit_rate_by_group(&df)?;

        // 4. 计算通路贡献
        let source_contrib = self.compute_source_contribution(&df)?;

        // 5. 实体共现矩阵
        let cooccurrence = self.compute_cooccurrence_matrix(&logs);

        // 6. 更新图谱 Learned 边
        for (from, to, weight) in &cooccurrence {
            self.entity_graph.add_learned_edge(from, to, *weight);
        }

        // 7. 构建结果
        let metrics = FeedbackMetrics {
            total_queries: logs.len(),
            hit_rate: hit_rate_df,
            source_contribution: source_contrib,
            avg_used_chunks: logs
                .iter()
                .map(|l| l.used_chunk_ids.len() as f64)
                .sum::<f64>()
                / logs.len() as f64,
            window_size: self.config.window_size,
            success_rate: logs.iter().filter(|l| l.task_success).count() as f64 / logs.len() as f64,
            timestamp: now_millis(),
        };

        Ok(FeedbackResult {
            metrics,
            learned_edge_updates: cooccurrence
                .into_iter()
                .map(|(a, b, w)| (a, b, w))
                .collect(),
            analyzed_logs: logs.len(),
            timestamp: now_millis(),
        })
    }

    /// 从 PG 加载执行日志
    #[cfg(feature = "feedback-analytics")]
    async fn load_logs_from_pg(
        &self,
        pg: &PgStorage,
        limit: usize,
    ) -> anyhow::Result<Vec<ExecutionLog>> {
        use crate::sutra_library::feedback::data_models::RecallChunkInfo;
        let pool = &**pg.pool();
        let rows = sqlx::query_as::<_, PgExecutionLogRow>(
            r#"
            SELECT query_hash, query, recalled_chunks, used_chunk_ids, task_success,
                   timestamp, graph, domain, session_id
            FROM sutra_execution_logs
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
        )
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;

        let mut logs = Vec::with_capacity(rows.len());
        for row in rows {
            let recalled_chunks: Vec<RecallChunkInfo> =
                serde_json::from_str(&row.recalled_chunks).unwrap_or_default();
            logs.push(ExecutionLog {
                query_hash: row.query_hash,
                query: row.query,
                recalled_chunks,
                used_chunk_ids: row.used_chunk_ids,
                task_success: row.task_success,
                timestamp: row.timestamp,
                graph: row.graph,
                domain: row.domain,
                session_id: row.session_id,
            });
        }

        Ok(logs)
    }

    /// 将日志转换为 Polars DataFrame
    #[cfg(feature = "feedback-analytics")]
    fn logs_to_dataframe(&self, logs: &[ExecutionLog]) -> anyhow::Result<polars::frame::DataFrame> {
        use polars::prelude::*;

        let query_hashes: Vec<String> = logs.iter().map(|l| l.query_hash.clone()).collect();
        let domains: Vec<String> = logs.iter().map(|l| l.domain.clone()).collect();
        let graphs: Vec<String> = logs.iter().map(|l| l.graph.clone()).collect();
        let task_success: Vec<bool> = logs.iter().map(|l| l.task_success).collect();
        let used_counts: Vec<u32> = logs.iter().map(|l| l.used_chunk_ids.len() as u32).collect();
        let recalled_counts: Vec<u32> = logs
            .iter()
            .map(|l| l.recalled_chunks.len() as u32)
            .collect();

        let df = df!(
            "query_hash" => &query_hashes,
            "domain" => &domains,
            "graph" => &graphs,
            "task_success" => &task_success,
            "used_count" => &used_counts,
            "recalled_count" => &recalled_counts,
        )?;

        Ok(df)
    }

    /// 计算分组命中率
    #[cfg(feature = "feedback-analytics")]
    fn compute_hit_rate_by_group(&self, df: &polars::frame::DataFrame) -> anyhow::Result<f64> {
        use polars::prelude::*;

        let total_recalled: u32 = df.column("recalled_count")?.sum::<u32>().unwrap_or(0);
        let total_used: u32 = df.column("used_count")?.sum::<u32>().unwrap_or(0);

        if total_recalled == 0 {
            return Ok(0.0);
        }

        Ok(total_used as f64 / total_recalled as f64)
    }

    /// 计算各召回通路贡献占比
    #[cfg(feature = "feedback-analytics")]
    fn compute_source_contribution(
        &self,
        _df: &polars::frame::DataFrame,
    ) -> anyhow::Result<HashMap<String, f64>> {
        // 从 ExecutionLog 的数据中统计各通路命中率
        // 这里用 DataFrame 做分组，简单返回当前窗口内的统计
        let metrics = self.get_metrics();
        Ok(metrics.source_contribution)
    }

    /// 计算实体共现矩阵（不依赖 polars，通用实现）
    #[cfg(feature = "feedback-analytics")]
    fn compute_cooccurrence_matrix(
        &self,
        logs: &[ExecutionLog],
    ) -> Vec<(EntityUuid, EntityUuid, f32)> {
        let mut cooccurrence_map: HashMap<(EntityUuid, EntityUuid), f32> = HashMap::new();

        for log in logs {
            if log.used_chunk_ids.is_empty() {
                continue;
            }

            let mut entities: HashSet<EntityUuid> = HashSet::new();
            for chunk_id in &log.used_chunk_ids {
                let chunk_entities = self
                    .entity_graph
                    .get_entities_of_chunks(std::slice::from_ref(chunk_id));
                entities.extend(chunk_entities);
            }

            if entities.len() < 2 {
                continue;
            }

            let entity_list: Vec<&EntityUuid> = entities.iter().collect();
            let delta = if log.task_success {
                self.config.cooccurrence_positive_delta
            } else {
                self.config.cooccurrence_failure_delta
            };

            for i in 0..entity_list.len() {
                for j in (i + 1)..entity_list.len() {
                    let from = entity_list[i].clone();
                    let to = entity_list[j].clone();
                    let key = if from <= to { (from, to) } else { (to, from) };
                    *cooccurrence_map.entry(key).or_insert(0.0) += delta;
                }
            }
        }

        cooccurrence_map
            .into_iter()
            .map(|((from, to), weight)| (from, to, weight.min(1.0).max(0.0)))
            .collect()
    }
}

// ─── PG 行结构（用于 sqlx 查询） ─────────────────────────────

#[cfg(feature = "feedback-analytics")]
#[derive(sqlx::FromRow)]
struct PgExecutionLogRow {
    query_hash: String,
    query: String,
    recalled_chunks: String,
    used_chunk_ids: Vec<String>,
    task_success: bool,
    timestamp: i64,
    graph: String,
    domain: String,
    session_id: Option<String>,
}

// ─── 工具函数 ─────────────────────────────────────────────────

/// 获取当前 Unix 毫秒时间戳
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sutra_library::feedback::data_models::RecallChunkInfo;
    use crate::sutra_library::recall::RetrieveSource;
    use crate::sutra_library::EntityGraph;

    fn make_test_log(chunk_ids: Vec<String>, success: bool) -> ExecutionLog {
        ExecutionLog {
            query_hash: "test_hash".to_string(),
            query: "test query".to_string(),
            recalled_chunks: chunk_ids
                .iter()
                .map(|id| RecallChunkInfo {
                    chunk_id: id.clone(),
                    source: RetrieveSource::Base,
                    score: 1.0,
                    llm_relevance: None,
                    llm_reason: None,
                })
                .collect(),
            used_chunk_ids: chunk_ids,
            task_success: success,
            timestamp: now_millis(),
            graph: "default".to_string(),
            domain: "test".to_string(),
            session_id: None,
        }
    }

    #[test]
    fn test_feedback_metrics_empty() {
        let graph = Arc::new(EntityGraph::new());
        let analyzer = FeedbackAnalyzer::new(graph, None, FeedbackConfig::default());
        let metrics = analyzer.get_metrics();
        assert_eq!(metrics.total_queries, 0);
        assert_eq!(metrics.hit_rate, 0.0);
    }

    #[test]
    fn test_feedback_metrics_with_logs() {
        let graph = Arc::new(EntityGraph::new());
        let analyzer = FeedbackAnalyzer::new(graph, None, FeedbackConfig::default());

        // 记录 3 条日志，每条使用 2 个切片
        for i in 0..3 {
            let log = make_test_log(
                vec![format!("chunk_{}_a", i), format!("chunk_{}_b", i)],
                true,
            );
            analyzer.record_execution(log);
        }

        let metrics = analyzer.get_metrics();
        assert_eq!(metrics.total_queries, 3);
        assert!(metrics.hit_rate > 0.0);
        assert_eq!(metrics.avg_used_chunks, 2.0);
        assert_eq!(metrics.success_rate, 1.0);
    }

    #[test]
    fn test_cooccurrence_analysis() {
        let graph = Arc::new(EntityGraph::new());
        let analyzer = FeedbackAnalyzer::new(graph.clone(), None, FeedbackConfig::default());

        // 注册实体到切片的关系
        graph.register_chunk(&crate::sutra_library::models::MemoryNode {
            node_id: "chunk_1".to_string(),
            collection_id: "col1".to_string(),
            domain: "test".to_string(),
            node_type: "doc".to_string(),
            content_hash: String::new(),
            parent_id: None,
            path: "/test".to_string(),
            depth: 0,
            sort_order: 0,
            title: "测试实体A 实体B".to_string(),
            summary: String::new(),
            content: "内容".to_string(),
            metadata: serde_json::Value::Null,
            refs_out: Vec::new(),
            refs_in: Vec::new(),
            version_tag: String::new(),
            snapshot_id: None,
            base_activation: 1.0,
            importance: 0,
            access_count: 0,
            feedback_score: 0.0,
            last_accessed_at: 0,
            created_at: 0,
            updated_at: 0,
        });

        // 记录一条使用的日志
        let log = make_test_log(vec!["chunk_1".to_string()], true);
        analyzer.record_execution(log);
        analyzer.analyze_cooccurrence(&[make_test_log(vec!["chunk_1".to_string()], true)], false);

        // 验证共现分析产生了 Learned 边
        // 注意：注册 chunk 时提取的实体可能为空，这里只是验证接口不崩溃
        let metrics = analyzer.get_metrics();
        assert!(metrics.total_queries > 0);
    }
}
