# 藏经阁记忆引擎 —— 实测分析与改进计划

> 排查时间：2026-09-13 23:15~23:35
> 排查方式：真实 MCP stdio JSON-RPC 三轮对话探针（`scripts/debug/mcp_sutra_probe.py`）+ HTTP 接口实测 + 落库只读核对
> 结论：**引擎骨架很完整（约 8000 行），但对实际回答的贡献几乎为零——记忆闭环没有打通。**

---

## 一、实测结果：现状数据

### 1.1 落库存量（sutra_library.sqlite，只读查询）

| 表 | 行数 | 说明 |
|---|---|---|
| `memory_collections` | 1 | `blender_knowledge`（幂等修复已生效，重启不再膨胀）|
| `memory_nodes` | **0** | 长期记忆节点一个都没有 |
| `memory_edges` | **0** | 关联边为空 |
| `kb_chunk` / `domain_kb` | **0** | 知识库切片为空 |
| `graph_*`（3 张）| **0** | 实体图为空 |
| `memory_snapshots` | 0 | 无快照 |

### 1.2 记忆闭环三轮探针（真 MCP 调用，非 mock）

| 轮次 | 输入 | 结果 |
|---|---|---|
| ① 注入 | "我的 Blender 项目固定使用 Cycles 渲染器，采样 128，输出 PNG" | 5.6s 回答正确（只是 LLM 复述，未落库） |
| ② 同会话追问 | "我刚才说的渲染器和采样值是多少？" | 4.1s **答对**（Cycles / 128）|
| ③ **新会话**追问 | 同样的问题 | 11.8s **完全答错**，反问"你用的是哪款软件？" |

**关键判定**：轮次②答对**不是藏经阁的功劳**，而是 `SessionManager` 的会话历史在起作用——因为 `memory_nodes` 全程为 0，藏经阁根本没有可供召回的数据。轮次③换会话后记忆立即归零，暴露了闭环断裂。

### 1.3 接口与运行态

- `GET /subhuti/api/v1/knowledge-bases` → `{"error":"数据库未连接（降级模式），知识库功能不可用"}`（所有 KB 接口依赖 PG，当前未启用）
- MCP 工具仅 5 个：`subhuti_chat`、`list_experts`、`match_expert`、`skill_list`、`skill_run`——**没有任何记忆/知识工具**
- 日志：`FeedbackAnalyzer: 后台分析完成, 日志数=5, 命中率=0.00%`
- Rust 专家实测输出："未在藏经阁知识库中找到与「trait」相关的内容"

---

## 二、架构现状：代码很完整

```
专家层   blender.rs / rust_expert.rs
           ├─ create_collection  建集合
           ├─ add_session        写会话临时记忆（内存态）
           ├─ library_retrieve   五阶段召回
           └─ record_execution   反馈记录
              ↓
端口层   SutraLibraryPort
              ↓
引擎层   engine.rs (1026行)  集合/节点CRUD、热度衰减、沉淀、反馈
           ├─ recall/    (3268行) BaseSearch→Space→Graph→合并→排序
           ├─ domain/    (1031行) 静态内置知识库（兜底）
           ├─ feedback/  (含 LLM 打分器)
           └─ Tantivy 中文全文索引（jieba 分词）
              ↓
存储层   PG(可选) / SQLite(降级，已接通) / 内存
```

能力清单（已实现但当前基本空转）：三级检索、五阶段召回流水线、热度衰减与冷热分层、快照版本、实体图、反馈分析器（含 LLM 打分）、Tantivy 中文全文索引。

---

## 三、根因：五个断裂点

### R1（致命）写入链断裂：会话记忆永不沉淀
`add_session_memory()` 把节点写进**内存态** session 存储；唯一把它转成长期节点的桥梁是 `precipitate_session()`，而该方法的**唯一调用方是 `MemorySkill`（死代码，从未被构造）**。生产链路没有任何地方调用它 → `memory_nodes` 恒为 0。

### R2 知识内容零供给
`kb_chunk` / `domain_kb` 的 CRUD 全部走独占 PG 路径，PG 未启用时接口直接返回"数据库未连接"。专家侧因此回落到 `domain/*.rs` 里硬编码的 1031 行静态知识库——内容固定、无法增长。

### R3 检索索引不持久
Tantivy 默认走 `Index::create_in_ram()`：进程内存、重启即丢，且当前本就无数据可索引。

### R4 无对外操作面
MCP 未暴露任何记忆工具；`MemorySkill`（264 行，含建集合/写记忆/检索/沉淀/点赞）是**全仓零调用的死代码**。AI 无法自主写入或沉淀记忆，人也只能靠 HTTP + PG 才能灌知识。

### R5 反馈闭环空转
`record_execution` 每轮都在调用，但因为召回结果恒空，`used_chunk_ids` 永远为空 → 命中率 0.00%，反馈信号没有任何信息量，热度/排序也无法被真正调优。

---

## 四、改进计划（按优先级）

### P0 —— 打通闭环（1~2 天，收益最大）

| # | 动作 | 落点 | 验收 |
|---|---|---|---|
| 1 | **会话结束时自动沉淀**：orchestration 完成处调用 `precipitate_session(session_id, 领域集合, domain)` | `orchestration_service.rs` 或 `blender.rs`/`rust_expert.rs` 执行末尾 | 探针轮次③（新会话）能答出 128 |
| 2 | **Tantivy 落盘**：改用 `create_in_dir` + 启动时从 SQLite 全量重建索引 | `recall/tantivy_index.rs` + `engine.sync_all_to_tantivy()` | 重启后召回不归零 |
| 3 | **沉淀内容做一次提炼**，不要原样存整段对话 | 沉淀前用 LLM 抽取"事实/偏好/配置"后再 `write_node` | 节点内容是可复用事实，而非流水账 |

> 好消息：SQLite 持久化**已经接通**（`SqliteStorage::open` 降级路径），P0 不需要新建存储层。

### P1 —— 让知识进得来、看得见（2~3 天）

| # | 动作 | 说明 |
|---|---|---|
| 4 | **冷启动知识灌入**：把 `domain/*.rs` 1031 行静态知识在首次启动时写入 `memory_nodes` | 立刻让召回有数据，把"硬编码兜底"变成"可检索、可反馈、可增长"的真实知识 |
| 5 | **MCP 暴露记忆工具**：`subhuti_memory_write` / `subhuti_memory_search` / `subhuti_memory_stats`，或直接把已有的 `MemorySkill` 接进 MCP（复用现成 264 行，别重写） | AI 可自主"记住/回忆"，死代码变活 |
| 6 | **KB 接口去 PG 强依赖**：`knowledge.rs` 的 PG-only 分支降级到 SQLite | 无 PG 环境也能灌知识库 |

### P2 —— 让闭环可度量、可自优化（3~5 天）

| # | 动作 | 说明 |
|---|---|---|
| 7 | **反馈闭环真实化**：只有真正被采用的 chunk 才计入 `used_chunk_ids`，把"召回命中率"做成可观测指标 | 命中率 0% 的假闭环变成真信号 |
| 8 | **可观测**：新增 `/subhuti/api/v1/sutra/stats`（节点数/冷热分布/召回命中率/Top 热记忆），并在 trace 里记录"本次召回了哪些节点" | 和已有的 token 统计一样能自证效果 |
| 9 | **沉淀去噪**：沉淀前过滤寒暄/闲聊，只沉淀含事实价值的轮次 | 防止把"好的""谢谢"写成长期记忆 |

### P3 —— 进阶（可选，视需求）

- 实体图 `graph_*` 真正启用（当前 0 行），支撑多跳关联召回
- 冷热分层淘汰策略调优（`HotnessCalculator` 参数）
- PG 模式下的多实例共享记忆

---

## 五、验证方式（可复用）

已新增探针脚本 `scripts/debug/mcp_sutra_probe.py`：

```bash
cargo build --bin subhuti
SUBHUTI_BIN=target/debug/subhuti SUBHUTI_DATA_DIR=/Users/hezenghui/sqlite \
  python3 scripts/debug/mcp_sutra_probe.py
```

它会自动完成「注入事实 → 同会话追问 → 新会话追问」三轮，并对比前后各表行数。
**判定标准：轮次③答对 + `memory_nodes > 0` = 闭环打通。** 修复前后各跑一次即可量化效果。

---

## 六、一句话总结

藏经阁现在是一套**建好了水利系统但没有通水**的工程：引擎、召回流水线、索引、反馈分析器一应俱全（约 8000 行），却因为「会话记忆不沉淀（R1）」+「知识内容零供给（R2）」+「无写入入口（R4）」三重断裂，对实际回答的贡献为 0。P0 只需补上"沉淀调用 + 索引落盘"两个动作，就能让它从装饰变成真正可用的记忆。
