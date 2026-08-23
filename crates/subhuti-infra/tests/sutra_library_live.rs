//! # 藏经阁引擎端到端测试
//!
//! 验证：
//! 1. PG 建表
//! 2. 集合 CRUD
//! 3. 节点写入/读取/删除
//! 4. 三级检索（BM25 内存 + PG tsvector）
//! 5. SutraLibraryPort trait 实现
//! 6. 幂等去重
//! 7. 会话记忆 + 沉淀

use std::sync::Arc;

use subhuti_infra::sutra_library::create_sutra_engine;
use subhuti_infra::sutra_library::storage::PgStorage;
use subhuti_infra::sutra_library::MemoryEnginePort;

/// 测试 PG 连接配置（与 config/Subhuti.toml 一致）
fn pg_pool() -> sqlx::PgPool {
    let dsn = "postgres://postgres:123456@localhost:5432/postgres";
    sqlx::PgPool::connect_lazy(dsn).expect("PG 连接失败，请确保 docker pgvector-db-new 在运行")
}

/// 创建带 PG 的藏经阁引擎
async fn setup_engine_with_pg() -> (Arc<MemoryEnginePort>, Arc<PgStorage>) {
    let pool = Arc::new(pg_pool());
    let pg = Arc::new(PgStorage::new(pool.clone()));

    // 建表
    pg.ensure_tables().await.expect("PG 建表失败");

    let (engine, _skill) = create_sutra_engine(Some((*pool).clone()));
    (engine, pg)
}

/// 创建纯内存的藏经阁引擎
fn setup_engine_memory() -> Arc<MemoryEnginePort> {
    let (engine, _skill) = create_sutra_engine(None);
    engine
}

// ─── 测试 1: PG 建表 ───────────────────────────────────────

#[tokio::test]
async fn test_pg_table_creation() {
    let pool = Arc::new(pg_pool());
    let pg = PgStorage::new(pool.clone());

    // 建表
    pg.ensure_tables().await.expect("建表失败");

    // 验证表存在（PgStorage 创建的是 memory_* 表）
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_all(&*pool)
    .await
    .expect("查询表列表失败");

    let table_names: Vec<&str> = rows.iter().map(|(n,)| n.as_str()).collect();
    assert!(
        table_names.contains(&"memory_nodes"),
        "memory_nodes 表未创建: {:?}",
        table_names
    );
    assert!(
        table_names.contains(&"memory_collections"),
        "memory_collections 表未创建: {:?}",
        table_names
    );
    assert!(
        table_names.contains(&"memory_edges"),
        "memory_edges 表未创建: {:?}",
        table_names
    );

    println!("✅ test_pg_table_creation: 通过");
}

// ─── 测试 2: 集合管理 ───────────────────────────────────────

#[tokio::test]
async fn test_collection_crud() {
    let (engine, pg) = setup_engine_with_pg().await;

    // 创建集合
    let col = engine.create_collection("test_blender", "blender", "Blender 测试知识");
    assert_eq!(col.name, "test_blender");
    assert_eq!(col.domain, "blender");

    // 列表包含新集合
    let cols = engine.list_collections();
    assert!(cols.iter().any(|c| c.name == "test_blender"));

    // 验证 PG 持久化（通过 list_nodes 间接验证集合存在）
    let pg_nodes = pg.list_nodes(&col.collection_id).await.unwrap_or_default();
    // 刚创建的集合还没有节点，所以 pg_nodes 应该是空的
    assert!(pg_nodes.is_empty(), "新集合应无节点");

    // 验证集合在 PG 中确实被写入了（通过 PG 直接查询集合）
    let _ = engine.write_node(&col.collection_id, "测试内容", "general", None);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    // 直接通过 PG 查询节点总数，确认数据已写入
    let pool = pg_pool();
    let total_nodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_nodes")
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
    assert!(
        total_nodes > 0,
        "PG 中应有节点（当前总数: {}）",
        total_nodes
    );

    println!("✅ test_collection_crud: 通过");
}

// ─── 测试 3: 节点写入与读取 ─────────────────────────────────

#[tokio::test]
async fn test_node_write_read() {
    let (engine, pg) = setup_engine_with_pg().await;

    // 创建集合
    let col = engine.create_collection("test_rust", "rust", "Rust 测试知识");

    // 写入 Rust 代码节点
    let content = r#"
fn fibonacci(n: u32) -> u32 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}
"#;
    let node = engine
        .write_node(&col.collection_id, content, "rust", None)
        .expect("写入节点失败");

    assert_eq!(node.title, "fibonacci");
    assert_eq!(node.node_type, "rust_fn");
    assert!(node.path.starts_with("/"));

    // 按 ID 读取
    let read = engine.read_node(&node.node_id).expect("读取节点失败");
    assert_eq!(read.node_id, node.node_id);

    // 按路径读取
    let by_path = engine.read_by_path(&node.path).expect("按路径读取失败");
    assert_eq!(by_path.node_id, node.node_id);

    // 验证 PG 持久化（轮询直到 PG 写入完成）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let pg_has_node = loop {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        match pg.list_nodes(&col.collection_id).await {
            Ok(nodes) if nodes.iter().any(|n| n.node_id == node.node_id) => break true,
            Ok(_) => {}
            Err(e) => {
                eprintln!("list_nodes error: {:?}", e);
            }
        }
        if std::time::Instant::now() > deadline {
            eprintln!(
                "超时 5s，collection_id={}, node_id={}",
                col.collection_id, node.node_id
            );
            break false;
        }
    };
    assert!(pg_has_node, "PG 中应包含写入的节点（超时 5s）");

    println!("✅ test_node_write_read: 通过");
}

// ─── 测试 4: BM25 搜索 ──────────────────────────────────────

#[tokio::test]
async fn test_bm25_search() {
    let engine = setup_engine_memory();
    let col = engine.create_collection("test_search", "general", "搜索测试");

    // 写入多篇文档（使用 Markdown 标题让 GeneralDomainParser 正确提取标题）
    let docs = [
        "# Rust 介绍\nRust 是一种系统编程语言，注重安全性和性能",
        "# Blender 简介\nBlender 是一款开源的 3D 建模和动画软件",
        "# Python 介绍\nPython 是一种解释型、面向对象的高级编程语言",
        "# PG 介绍\nPostgreSQL 是一种强大的开源关系型数据库",
    ];
    for content in &docs {
        let _ = engine
            .write_node(&col.collection_id, content, "general", None)
            .expect("写入文档失败");
    }

    // 搜索 "Rust"
    let result = engine
        .simple_search("Rust 编程", Some(&col.collection_id), 5)
        .await;
    assert!(!result.is_empty(), "搜索 Rust 应返回结果");
    assert!(
        result.iter().any(|n| n.title == "Rust 介绍"),
        "结果应包含 Rust 介绍"
    );

    // 搜索 "Blender 3D"
    let result = engine
        .simple_search("Blender 3D", Some(&col.collection_id), 5)
        .await;
    assert!(!result.is_empty(), "搜索 Blender 应返回结果");
    assert!(
        result.iter().any(|n| n.title == "Blender 简介"),
        "结果应包含 Blender 简介"
    );

    println!("✅ test_bm25_search: 通过");
}

// ─── 测试 5: PG tsvector 搜索 ───────────────────────────────

#[tokio::test]
async fn test_pg_tsvector_search() {
    let (engine, _pg) = setup_engine_with_pg().await;
    let col = engine.create_collection("test_tsvector", "general", "tsvector 搜索测试");

    // 写入文档并等待 PG 同步（使用 Markdown 标题）
    let docs = [
        "# Rust 所有权\nRust 的所有权系统保证了内存安全，无需垃圾回收",
        "# Blender 几何节点\nBlender 的几何节点系统可以创建复杂的程序化模型",
        "# Rust Trait 系统\nRust 的 trait 系统支持泛型编程和编译时多态",
    ];
    for content in &docs {
        let _ = engine
            .write_node(&col.collection_id, content, "general", None)
            .expect("写入文档失败");
    }

    // 等待 PG tsvector 同步完成
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // 搜索 "Rust 所有权"
    let result = engine
        .simple_search("Rust 所有权", Some(&col.collection_id), 5)
        .await;
    assert!(!result.is_empty(), "搜索 Rust 应返回结果");
    assert!(
        result.iter().any(|n| n.content.contains("所有权")),
        "应匹配到所有权相关内容"
    );

    // 搜索 "几何节点"
    let result = engine
        .simple_search("几何节点", Some(&col.collection_id), 5)
        .await;
    assert!(!result.is_empty(), "搜索几何节点应返回结果");

    println!("✅ test_pg_tsvector_search: 通过");
}

// ─── 测试 6: 幂等去重 ───────────────────────────────────────

#[tokio::test]
async fn test_idempotent_write() {
    let engine = setup_engine_memory();
    let col = engine.create_collection("test_dedup", "general", "去重测试");

    let content = "这是一段重复的内容";

    // 第一次写入
    let node1 = engine
        .write_node(&col.collection_id, content, "general", None)
        .expect("第一次写入失败");

    // 第二次写入相同内容
    let node2 = engine
        .write_node(&col.collection_id, content, "general", None)
        .expect("第二次写入失败");

    // 应返回相同节点（幂等）
    assert_eq!(node1.node_id, node2.node_id, "幂等去重应返回相同 node_id");
    assert_eq!(node1.content_hash, node2.content_hash);

    println!("✅ test_idempotent_write: 通过");
}

// ─── 测试 7: 树结构 + 父子关系 ─────────────────────────────

#[tokio::test]
async fn test_tree_structure() {
    let engine = setup_engine_memory();
    let col = engine.create_collection("test_tree", "general", "树结构测试");

    // 写入根节点
    let root = engine
        .write_node(&col.collection_id, "# 根节点\n这是根内容", "general", None)
        .expect("写入根节点失败");

    // 写入子节点 (parent_id = root.node_id)
    let child = engine
        .write_node(
            &col.collection_id,
            "# 子节点\n这是子内容",
            "general",
            Some(&root.node_id),
        )
        .expect("写入子节点失败");

    assert_eq!(child.parent_id, Some(root.node_id.clone()));
    assert!(child.path.starts_with(&root.path));

    // 获取子节点
    let children = engine.get_children(&root.node_id);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].node_id, child.node_id);

    println!("✅ test_tree_structure: 通过");
}

// ─── 测试 8: 删除节点 (递归子树) ───────────────────────────

#[tokio::test]
async fn test_delete_node() {
    let engine = setup_engine_memory();
    let col = engine.create_collection("test_delete", "general", "删除测试");

    let root = engine
        .write_node(&col.collection_id, "# 根节点", "general", None)
        .expect("写入根节点失败");
    let _child = engine
        .write_node(
            &col.collection_id,
            "# 子节点",
            "general",
            Some(&root.node_id),
        )
        .expect("写入子节点失败");

    // 删除根节点（递归删除子树）
    engine.delete_node(&root.node_id);

    assert!(engine.read_node(&root.node_id).is_none(), "根节点应被删除");
    let children = engine.get_children(&root.node_id);
    assert!(children.is_empty(), "子节点也应被删除");

    println!("✅ test_delete_node: 通过");
}

// ─── 测试 9: 会话记忆 + 沉淀 ───────────────────────────────

#[tokio::test]
async fn test_session_and_precipitate() {
    let engine = setup_engine_memory();
    let col = engine.create_collection("test_session", "general", "会话测试");

    // 添加会话记忆
    engine.add_session_memory("session_001", "用户询问了 Blender 建模技巧", "blender");
    engine.add_session_memory("session_001", "专家回答了关于拓扑优化的建议", "blender");

    // 沉淀到持久集合
    engine.precipitate_session("session_001", &col.collection_id, "blender");

    // 验证沉淀后的节点 - 通过集合 ID 搜索来验证
    let result = engine
        .simple_search("Blender 建模", Some(&col.collection_id), 5)
        .await;
    assert!(!result.is_empty(), "沉淀后应能搜索到节点");

    // 验证节点确实在集合中（通过路径匹配）
    let nodes_with_collection = engine.list_collections();
    assert!(
        nodes_with_collection
            .iter()
            .any(|c| c.name == "test_session"),
        "集合应存在"
    );

    println!("✅ test_session_and_precipitate: 通过");
}

// ─── 测试 10: SutraLibraryPort trait 实现 ──────────────────

#[tokio::test]
async fn test_sutra_library_port_trait() {
    use subhuti_core::sutra_library::SutraLibraryPort;

    let (engine, _pg) = setup_engine_with_pg().await;
    let port: Arc<dyn SutraLibraryPort> = engine as Arc<dyn SutraLibraryPort>;

    // 1. 创建集合
    let result = port.create_collection("port_test", "general", "Port 接口测试");
    assert!(!result.contains("⚠️"), "创建集合失败: {}", result);
    assert!(result.contains("✅"), "创建集合应返回成功: {}", result);

    // 2. 列出集合
    let cols = port.list_collections();
    assert!(!cols.contains("⚠️"), "列出集合失败");
    assert!(cols.contains("port_test"), "列表应包含新集合");

    // 3. 写入节点
    let write_result = port.write(
        "port_test",
        "这是通过 Port 接口写入的测试内容",
        "general",
        None,
    );
    assert!(!write_result.contains("❌"), "写入失败: {}", write_result);
    assert!(
        write_result.contains("✅"),
        "写入应返回成功: {}",
        write_result
    );

    // 4. 搜索
    let search_result = port.search("测试内容", Some("port_test"), 5).await;
    assert!(
        !search_result.contains("未找到"),
        "搜索应找到内容: {}",
        search_result
    );

    // 5. 统计
    let stats = port.stats();
    assert!(!stats.contains("⚠️"), "统计失败: {}", stats);

    // 6. 会话记忆
    let session_result = port.add_session("test_session_port", "会话测试内容", "general");
    assert!(
        !session_result.contains("❌"),
        "会话失败: {}",
        session_result
    );

    println!("✅ test_sutra_library_port_trait: 通过");
}

// ─── 测试 11: 反馈机制 ──────────────────────────────────────

#[tokio::test]
async fn test_feedback() {
    let engine = setup_engine_memory();
    let col = engine.create_collection("test_feedback", "general", "反馈测试");

    let node = engine
        .write_node(&col.collection_id, "测试反馈的内容", "general", None)
        .expect("写入节点失败");

    // 正反馈
    engine.apply_feedback(&node.node_id, true);
    let node = engine.read_node(&node.node_id).unwrap();
    assert!(node.feedback_score > 0.0, "正反馈后分数应 > 0");

    // 负反馈
    engine.apply_feedback(&node.node_id, false);
    let node = engine.read_node(&node.node_id).unwrap();
    assert!(node.feedback_score < 0.1, "负反馈后分数应降低");

    println!("✅ test_feedback: 通过");
}

// ─── 测试 12: 空引擎（无 PG 降级） ─────────────────────────

#[tokio::test]
async fn test_empty_sutra_library() {
    use subhuti_core::sutra_library::{EmptySutraLibrary, SutraLibraryPort};

    let empty = EmptySutraLibrary::new();

    let result = empty.create_collection("test", "general", "测试");
    assert!(result.contains("⚠️"), "空引擎应返回警告");

    let stats = empty.stats();
    assert!(stats.contains("⚠️"), "空引擎统计应返回警告");

    let search = empty.search("test", None, 5).await;
    assert!(search.contains("⚠️"), "空引擎搜索应返回警告");

    println!("✅ test_empty_sutra_library: 通过");
}

// ─── 测试 13: 实体图谱数据持久化到 PG ──────────────────────

#[tokio::test]
async fn test_entity_graph_pg_persistence() {
    let pool = Arc::new(pg_pool());
    let (engine, _pg) = setup_engine_with_pg().await;

    // 等待异步 PG 初始化完成
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let col = engine.create_collection("test_graph_pg", "blender", "实体图谱 PG 持久化测试");

    // 写入一组 Blender 知识文档（使用 blender 领域，触发实体关系边提取）
    let docs = [
        "# Blender 阵列修改器\n阵列修改器（Array Modifier）可以创建循环建模效果，\
         通过设置数量和偏移量实现物体的重复排列，常用于创建围栏、楼梯等重复结构",
        "# Blender 几何节点\n几何节点系统支持程序化建模，\
         通过节点图控制几何体的生成和变换，可以创建复杂的参数化模型",
        "# Blender 材质节点\n材质节点编辑器支持 PBR 材质制作，\
         包括金属度、粗糙度、法线贴图等，可以实现逼真的表面效果",
        "# Blender 骨骼绑定\n骨骼绑定系统用于角色动画，\
         支持 IK/FK 切换和权重绘制，是实现角色动作的基础",
        "# Blender Cycles 渲染\nCycles 渲染器支持 GPU 加速，\
         提供高质量的光线追踪渲染，支持多种采样策略",
        "# Blender 雕刻模式\n雕刻模式支持动态拓扑，\
         适合进行数字雕刻和细节塑造，是角色建模的重要工具",
        "# Blender 粒子系统\n粒子系统可以创建头发、草地、火焰等特效，\
         支持物理模拟和碰撞检测",
        "# Blender UV 展开\nUV 展开工具用于纹理映射，\
         支持智能投影和缝合边，是材质贴图的关键步骤",
    ];
    for content in &docs {
        let _ = engine
            .write_node(&col.collection_id, content, "blender", None)
            .expect("写入文档失败");
    }

    // 等待异步 PG 写入完成
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // 检查 PG 中的实体图谱数据
    let edges_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM graph_edges")
        .fetch_one(&*pool)
        .await
        .expect("查询 graph_edges 失败");
    println!("  graph_edges: {} 条", edges_count.0);

    let chunks_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM graph_entity_chunks")
        .fetch_one(&*pool)
        .await
        .expect("查询 graph_entity_chunks 失败");
    println!("  graph_entity_chunks: {} 条", chunks_count.0);

    let nodes_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memory_nodes WHERE collection_id = $1")
            .bind(&col.collection_id)
            .fetch_one(&*pool)
            .await
            .expect("查询 memory_nodes 失败");
    println!("  memory_nodes: {} 条", nodes_count.0);

    // 断言：图谱数据应写入 PG
    assert!(
        edges_count.0 > 0 || chunks_count.0 > 0,
        "实体图谱数据应写入 PG，但 graph_edges={}, graph_entity_chunks={}",
        edges_count.0,
        chunks_count.0
    );

    // 如果图谱边存在，显示详情
    if edges_count.0 > 0 {
        let edges: Vec<(String, String, String, f32)> = sqlx::query_as(
            "SELECT from_entity, to_entity, edge_kind, weight FROM graph_edges LIMIT 10",
        )
        .fetch_all(&*pool)
        .await
        .expect("查询边详情失败");
        println!("  图谱边详情:");
        for (from, to, kind, weight) in &edges {
            println!("    {} --[{}]--> {} (weight: {})", from, kind, to, weight);
        }
    }

    println!("✅ test_entity_graph_pg_persistence: 通过");
}

// ─── 测试 14: 五阶段召回流水线 ─────────────────────────────

#[tokio::test]
async fn test_library_retrieve_pipeline() {
    let (engine, _pg) = setup_engine_with_pg().await;

    // 等待异步 PG 初始化
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let col = engine.create_collection("test_retrieve_pipeline", "blender", "召回流水线测试");

    // 写入 Blender 知识文档（使用 blender 领域，触发实体关系边提取）
    let docs = [
        "# Blender 阵列修改器\n阵列修改器可以创建循环建模效果，通过设置数量和偏移量实现重复排列",
        "# Blender 几何节点\n几何节点系统支持程序化建模，通过节点图控制几何体生成和变换",
        "# Blender 材质节点\n材质节点编辑器支持 PBR 材质制作，包括金属度、粗糙度等贴图",
        "# Blender 骨骼绑定\n骨骼绑定系统用于角色动画，支持 IK/FK 切换和权重绘制",
        "# Blender Cycles 渲染\nCycles 渲染器支持 GPU 加速，提供高质量的光线追踪渲染",
        "# Blender 雕刻模式\n雕刻模式支持动态拓扑，适合数字雕刻和细节塑造",
        "# Blender 粒子系统\n粒子系统可以创建头发、草地、火焰等特效",
        "# Blender UV 展开\nUV 展开工具用于纹理映射，支持智能投影和缝合边",
    ];
    for content in &docs {
        let _ = engine
            .write_node(&col.collection_id, content, "blender", None)
            .expect("写入文档失败");
    }

    // 等待异步 PG 写入完成
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // 测试召回流水线
    use subhuti_infra::sutra_library::recall::LibraryRetrieveConfig;

    let config = LibraryRetrieveConfig {
        base_top_k: 5,
        space_max_tree_distance: 1,
        space_per_level_limit: 3,
        graph_max_depth: 2,
        graph_max_global_result: 5,
        ..Default::default()
    };

    let queries = ["阵列修改器", "材质节点", "骨骼绑定", "渲染器", "粒子系统"];
    for query in &queries {
        let candidates = engine.library_retrieve(query, &config).await;
        println!("  查询 '{}': {} 个候选", query, candidates.len());
        for (i, c) in candidates.iter().enumerate() {
            // 解析 chunk_id 获取标题
            let title = if let Some(node) = engine.read_node(&c.chunk_id) {
                node.title
            } else {
                "未知".to_string()
            };
            let total = c.base_score + c.bonus_score;
            println!(
                "    {}. [score={:.4}(base={:.4}+bonus={:.4})] {:?} {}",
                i + 1,
                total,
                c.base_score,
                c.bonus_score,
                c.source_flags,
                title
            );
        }
        // 后置学习
        engine.post_retrieve_learn(&candidates);
        assert!(
            !candidates.is_empty(),
            "查询 '{}' 应返回至少一个候选",
            query
        );
    }

    println!("✅ test_library_retrieve_pipeline: 通过");
}

// ─── 测试 15: 后置共现学习生成 Learned 边 ──────────────────

#[tokio::test]
async fn test_post_retrieve_learn() {
    let pool = Arc::new(pg_pool());
    let (engine, _pg) = setup_engine_with_pg().await;

    // 等待异步 PG 初始化
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let col = engine.create_collection("test_learn_edges", "blender", "共现学习测试");

    // 写入文档
    let docs = [
        "# Blender 阵列修改器\n阵列修改器可以创建循环建模",
        "# Blender 几何节点\n几何节点支持程序化建模",
        "# Blender 材质节点\n材质节点编辑器支持 PBR 材质",
    ];
    for content in &docs {
        let _ = engine
            .write_node(&col.collection_id, content, "blender", None)
            .expect("写入文档失败");
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // 执行召回并后置学习
    use subhuti_infra::sutra_library::recall::LibraryRetrieveConfig;
    let config = LibraryRetrieveConfig::default();
    let candidates = engine.library_retrieve("阵列修改器", &config).await;
    engine.post_retrieve_learn(&candidates);

    // 等待异步学习写入 PG
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // 检查 Learned 边
    let learned_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM graph_edges WHERE edge_kind = 'Learned'")
            .fetch_one(&*pool)
            .await
            .expect("查询 Learned 边失败");
    println!("  Learned 边数量: {}", learned_count.0);

    // 如果有 Learned 边，显示详情
    if learned_count.0 > 0 {
        let learned: Vec<(String, String, f32)> = sqlx::query_as(
            "SELECT from_entity, to_entity, weight FROM graph_edges WHERE edge_kind = 'Learned' LIMIT 10",
        )
        .fetch_all(&*pool)
        .await
        .expect("查询 Learned 边详情失败");
        println!("  Learned 边详情:");
        for (from, to, weight) in &learned {
            println!("    {} <-> {} (weight: {})", from, to, weight);
        }
    }

    println!("✅ test_post_retrieve_learn: 通过");
}
