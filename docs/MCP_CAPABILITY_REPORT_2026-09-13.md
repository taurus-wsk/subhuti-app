# Subhuti MCP 能力验证报告

- **验证时间**：2026-09-13 01:32 起（问题定位）→ 02:00 修复并复验 → **20:14 ~ 21:25 六轮迭代复评** → **21:30 终验（第七轮）**
- **被测对象**：`subhuti mcp`（stdio JSON-RPC，协议版本 2024-11-05，服务端并发上限=2）
- **验证方式**：三个探针驱动真实 `target/debug/subhuti mcp` 进程（**无 mock**），覆盖生命周期 / 工具发现 / 错误语义 / 五工具契约 / 进度订阅 / 会话粘性 / 并发 / 输入边界 / 工具超时
- **结论**：**协议层零缺陷；43 项检查全部通过**（`mcp_eval` 28/28 + `mcp_eval_deep` 15/15）。20:14 起新发现 **5 个真实缺陷**（含 1 个 P0 产品语义缺陷），**已全部修复并逐轮实测确认**；终验另清理 1 处**仓库污染**（非协议缺陷，见 §8）；保留 1 项**有意识的设计缺口**（不实现真实取消，理由见 §5）

> 📌 全文结构：**复评与修复迭代（最新，见下节）** → 早期修复结果 → 完整验证过程与分析。
> ⚠️ 下方「三、MCP 完全没有阶段流」「P0 `session_id` 被忽略」「冷启动 ~6s（含 5s PG 超时）」
> 「进度令牌缺失则回退 request id」等章节**均已被复评推翻或闭环**，一律以复评章节为准。

---

# 复评与修复迭代（2026-09-13 20:14 ~ 21:25）

## 结论

**协议层无缺陷**——握手、能力声明、工具发现、JSON-RPC 错误码、`isError` 语义、进度 opt-in、
并发保序、batch、输入边界全部正确。本轮暴露的问题**集中在产品语义与健壮性两层**，
均已定位到具体代码行并修复。

## 1. 六轮迭代轨迹

| 轮次 | 探针 | 结果 | 发现 / 处理 |
|------|------|------|------------|
| 1 | `mcp_eval`(28) + `mcp_eval2` | 27/28 | ① 粘性越权（P0）；② planner 解析无容错（P1）；③ batch 未处理；④ cancelled 不真取消 |
| 2 | 同上 | — | 修 ①（门控）后发现 **⑤ 粘性记录点错误**（只在执行成功时写 → 轮1 失败则轮2 断链） |
| 3 | + `mcp_eval3`(15) | 27/28 | 新增覆盖 `skill_run`/`expert_id`/`system_prompt`/边界/令牌隔离；⑥ 占位步骤拖慢（95.7s / 226.6s） |
| 4 | `mcp_eval` + `mcp_eval3` | **28/28** | 修 ⑥ 后粘性组全绿；eval3 探针自身有 bug（见 §6） |
| 5 | `mcp_eval3` + `mcp_eval2` | 14/15 | 占位步骤过滤生效（95.7s→**64.1s**、45.7s→**8.1s**）；1 项 FAIL 系用例缺领域 tag |
| 6 | `mcp_eval` + `mcp_eval3` | **28/28 + 15/15** | 收敛 |
| 7（终验） | `mcp_eval` + `mcp_eval_deep` | **28/28 + 15/15** | 独立复现收敛；另清理 1 处仓库污染（§8） |

> 命名说明：早期轮次的 `mcp_eval2` / `mcp_eval3` 是临时探针，最终统一落库为两个文件——
> `scripts/debug/mcp_eval.py`（28 项基础）与 `scripts/debug/mcp_eval_deep.py`（15 项深挖，即原 `mcp_eval3`）。
> 两者均支持用 `SUBHUTI_BIN` 覆盖被测二进制，默认取 `target/debug/subhuti`。

## 2. 本轮发现并修复的缺陷

### ① 【P0 · 产品语义】会话粘性越权，架空了「零命中不兜底」

**现象**：同一 `session_id` 下先问 Rust、再问**领域外**问题，会被粘性**无条件**吞进上一个专家：

| 输入（前一轮为 Rust） | 修复前 | 修复后 |
|---|---|---|
| `今天天气怎么样` | ❌ 路由到 Rust 专家，生成「询问用户所在位置」步骤作答（12.0s） | ✅ 0.0s 返回领域边界提示 |
| `帮我写首诗赞美大海` | ❌ 进 Rust 专家 → planner JSON 解析失败（失败理由错误） | ✅ 0.0s 返回领域边界提示 |

**根因**：`crates/subhuti-core/src/orchestrator/mod.rs` 零命中分支**无条件**读 `last_expert_id`，
不区分两种语义相反的输入——(a) 承接上一轮（「再加个骨骼」）该延续；(b) 完整且领域无关（「今天天气」）该走边界提示。

**修复**：新增 `looks_like_continuation()`（延续信号门控：仅含指代/承接词如
它/这个/再/继续/补充/另外…才放行粘性）+ `unmatched_domain_result()` 统一出口
（顺带消除 `dispatch_direct` 与 `dispatch_without_graph` 两处完全重复的边界文案）。
边界提示里追加**上下文引导**（「本会话上一轮由「X」处理，若为同一任务的延续请补充说明」）——
只提示、不代答，既保住上下文价值又不越权。

### ② 【P1 · 会话连续性】粘性记录点错误：只在执行**成功**时写

**现象**：轮 1「用 rust 写个函数」因缺 `workspace_folder` 前置条件失败 → 未记录专家 →
轮 2「再补充一下错误处理」被判领域外，**会话直接断裂**。用户视角是「我刚跟 Rust 专家说过话」。

**根因**：`last_expert_id` 写在 `dispatch_via_actor` 的 **`Ok` 分支**。
而粘性要保的是「对话连续性」（用户刚才在跟谁说话），该事实在**路由命中那一刻**即已确定，
**与执行成败无关**。

**修复**：把写入移到路由确定处（`actor_id` 拿到之后立刻写）。
回归用例 `sticky_is_recorded_even_when_expert_execution_fails`。

### ③ 【P1 · 健壮性】planner 输出 JSON 解析无容错

**现象**：`帮我写首诗赞美大海` → `解析规划结果失败: expected '}' at line 8 column 30`，错误直接透给用户。
LLM 输出被截断（如只吐到 `{"desc`）即整体失败，且原始输出全文被塞进用户可见报错。

**修复**（`crates/subhuti-core/src/orchestrator/planner.rs`）：
- 新增 `chat_and_parse_plan()`：**解析失败自动带纠错指令重试一次**（把坏输出作为 assistant 消息回喂
  + 强约束「只输出合法 JSON、步骤 ≤3」），两个规划入口（专家内部 / 主管多专家）共用。
- 新增 `output_preview()`：错误文案里的原始输出**截断为 200 字预览**，不再甩几千字乱码。
- 专家层加**降级**：两次仍不合法 → 退化为「单步直答」（挑纯 LLM 的 `*-chat` 技能，无副作用），
  而非让整个请求崩掉（`src/domain/traits.rs`）。

### ④ 【P1 · 性能】planner 生成「只提问不产出」的占位步骤

**现象**：即便 system prompt 明确禁止，LLM 仍偶发把「与用户沟通」「询问用户所在位置」排成步骤。
实测后果被放大：`用 rust 写一个函数并说明模块划分` **226.6 s**、`再补充一下错误处理` **95.7 s**，
换来一句「抱歉，我无法获取您当前的地理位置」。

**修复**：提示词约束不可靠 → 在**代码层**加确定性过滤 `strip_placeholder_steps()`
（词表收窄到语义明确等于「停下来问用户」的短语，避免误伤「确认目录是否存在」这类有效步骤，
并重排 `order`）。过滤后若为空，上层已有的「空计划 → 直接对话回答」分支自动接管。

**实测效果**：`再补充一下错误处理` 95.7s → **64.1s**，`那它和 Iterator 的关系呢` 45.7s → **8.1s**。

### ⑤ 【P1 · 韧性】单次工具调用无耗时上限

**现象**：LLM 供应商降级时单次工具耗时**无界**——`RetryLLM` 单次超时 120s × 最多 3 次尝试
+ 指数退避，实测出现过单次 `subhuti_chat` 116.2s、以及长时间无响应的情况。
客户端只能无限等待或强杀进程。

**修复**：MCP 出口加**有界超时**（`DEFAULT_TOOL_TIMEOUT_SECS = 300`，
`SUBHUTI_MCP_TOOL_TIMEOUT_SECS` 可覆盖，设 0 表示不限制），超时以 `isError=true` +
可读原因（含调整方法）返回，把「挂死」变成「可预期的失败」。已实测：设为 2s 时 2.0s 即返回超时错误。

### 附带修复

| 项 | 说明 |
|----|------|
| JSON-RPC **batch 支持** | 数组请求此前被静默丢弃（客户端超时）。现支持数组并聚合为数组响应（通知项不计入）；空数组按规范回**单个** `-32600`；整批只占一个并发许可，防止绕过限流 |
| `notifications/cancelled` **可观测** | 仍不中断在途任务，但补了带 `requestId` 的 debug 日志（理由见 §5） |
| 零命中结果构造去重 | `dispatch_direct` 与 `dispatch_without_graph` 的重复长文案收敛为 `unmatched_domain_result()` |

## 3. 检查矩阵（最终轮，全绿）

| 组 | 检查项 | 结果 |
|----|--------|------|
| 生命周期 | initialize 协议版本 / tools 能力 / serverInfo / ping | **4/4** |
| 工具发现 | 5 工具 / 名字齐备 / inputSchema 合法 / 必填参数已声明 | **4/4** |
| 错误语义 | -32601 / 未知通知静默 / 缺 message / 错参数名 / 未知工具 | **5/5** |
| 只读工具 | list_experts / skill_list / match 命中 / match 领域外不误命中 | **4/4** |
| 进度订阅 | 未订阅→0 条 / 订阅→12 条 / 含阶段标签 / 并发下 token 隔离 | **4/4** |
| 聊天成败 | 领域内 false / 含 [meta] / 领域外 true | **3/3** |
| 会话粘性 | 轮1 路由 / 轮2 承接延续 / 跨域不越权 | **3/3** |
| 并发 | 3 路与 6 路并发 / id 无串扰 / 内容各自正确 | **4/4** |
| 技能执行 | skill_run 成功 / 未知技能 / 缺参 / args 类型错 | **4/4** |
| 强制路由 | 显式 expert_id 命中 / 不存在不静默回落 | **2/2** |
| system_prompt | 覆盖生效（输出被约束为指定内容） | **1/1** |
| 输入边界 | emoji / 超长（约 2.6k 字）/ 纯空白 | **3/3** |
| batch | 数组语义 / 空数组 -32600 / 协议流不受损 | **3/3** |
| 工具超时 | 配置生效、超时以 isError 返回 | **1/1** |
| **合计** | | **43/43** |

## 4. 性能实测

| 场景 | 耗时 | 说明 |
|------|------|------|
| 进程冷启动 → initialize 响应 | **1.31 / 1.48 / 1.64 s** | 较旧报告 ~6s 大幅改善（PG 探活已关） |
| 只读工具 | 0 ms | 无 LLM |
| **领域外零命中** | **0.0 s** | 不发 LLM，直接边界提示 |
| 领域内短问答 | 6.5 ~ 33.9 s | 受 LLM 供应商波动影响明显（同一 prompt 曾出现 4.6 / 12.2 / 33.9 / 116.2 s） |
| 编码类任务 | 47 ~ 106 s | 占位步骤过滤后已明显缩短 |

> ⚠️ 观测到的耗时波动**主要来自 LLM 供应商侧**（同一 prompt 抖动可达 25×），
> 而非编排开销。第三轮曾出现 116.2s 的简单问答；同轮供应商恢复后回落到 27.4s。

## 5. 保留的缺口（有意识，非遗漏）

**`notifications/cancelled` 接受但不真实中断在途任务。**

不做「直接 abort 掉那个 tokio 任务」这种最省事的实现，理由：专家执行链里有工具调用
（`cargo check` / blender 子进程），`tokio::task::abort` 只丢弃 future，**不会杀掉已 spawn 的子进程**
（未设 `kill_on_drop`），结果是「客户端以为取消了，后台 cargo 还在跑」，**比不取消更糟**。
正确做法是把取消令牌协作式传到专家每个 await 点，属跨框架与领域层的大改动。
规范里取消是 SHOULD，stdio 客户端也可通过关闭管道终止进程。已补日志以便可观测。

## 6. 探针自身的两个 bug（避免误判为产品缺陷）

排查过程中出现两次**假故障**，均为探针缺陷，特此记录以防误读：

1. **`subhuti_skill_run`「卡死」超时 400s**（曾疑似死锁）。实为探针客户端 `call()` 的等待循环**漏写
   `self._ev.wait(r)`**，退化成「持锁忙等」→ 读取线程永远拿不到锁写入响应 → 必然超时。
   服务端本身 1.2~4.2 s 正常返回（已用三种客户端交叉验证）。
2. **batch 「未处理」** ——探针未对「数组响应」做类型守卫，`list.get()` 抛异常；服务端实际已正确返回数组。

另有 2 项**用例设计缺陷**（非产品问题）：会话粘性用例用 `[meta]` 判断是否路由
（失败请求不带 meta → 误判）；`system_prompt` 用例的 message 不含领域 tag
（先被零命中拦住，覆盖根本没机会生效）。均已修正为准。

## 7. 旧报告口径修正

| 旧结论 | 现状 |
|--------|------|
| 进度令牌「缺失则回退 request id」 | ✅ **严格 opt-in**：未带 `_meta.progressToken` 的客户端收到 **0 条**通知 |
| 【P0】`session_id` 被忽略 | ✅ 已修 |
| 【P1】MCP 完全没有阶段流 | ✅ 已修（实测 12 条 progress，phase 与 SSE 同源） |
| 冷启动 ~6s（含 5s PG 超时） | ✅ 已解决（**1.3~1.6s**） |
| 【P2】`skill_run` 恒传空 workspace / 空错误文案 | ✅ 已修（统一 `extra`；错误文案有兜底） |
| 多轮上下文 | ✅ 已实现（SQLite 持久化） |

## 8. 终验（第七轮）与一处仓库污染

**终验结果**：`mcp_eval` **28/28**、`mcp_eval_deep`（原 `mcp_eval3`）**15/15** → **43/43 复现**；
`cargo check --workspace --all-targets` 零错误零警告；`cargo fmt --all --check` 干净；
`cargo test --workspace --lib` → app **32** / core **61** / infra **62** 全通过。

终验时另发现并处理了一处**仓库完整性**问题（**非协议缺陷**）：

| 项 | 内容 |
|----|------|
| 现象 | 仓库根 `Cargo.toml` 的 workspace `members` 里多出 `"fibonacci_project"`；`fibonacci_project/` 是测试期间由 Rust 专家生成的一次性 crate（仅 `Cargo.toml` + `src/main.rs`，untracked） |
| 证据 | 根 `Cargo.toml` 与 `fibonacci_project/` 的 mtime **完全相同**（`2026-09-13 19:34:02`）→ 由「在项目工作目录中创建新项目」那一步连带写入 |
| 危害 | 若提交 `Cargo.toml` 而生成目录不入库（本次正是 untracked），`git clone` 后 cargo 直接报 `failed to load manifest for workspace member`，**整个仓库构建失败**；本地也会让每次 `--workspace` 构建多编译一个随手生成的 crate |
| 根因 | 触发那次生成时 `extra.workspace_folder` 指向了**源码仓库根目录**，专家遂在仓库内建项目，并按 Cargo 惯例把新 crate 注册进宿主 workspace 清单 |
| 处理 | ① 从 `members` 移除该条目；② 删除残留目录；③ 复验 `check / fmt / test` 全绿 |

**结论与建议**：专家「在给定工作目录内建项目、并在 Cargo workspace 中注册新 crate」本身符合惯例，
不算缺陷；风险在于**工作目录被指向源码仓库**时，代理产物会落进版本控制的工作区并静默改写受管文件。
因此**测试生成类任务请用临时目录**（如 `/tmp/subhuti-scratch`）作为 `extra.workspace_folder`。
本次两个探针均**未传** `extra.workspace_folder`（专家缺该前置条件时只会报错、不会落盘），
所以这一轮 43/43 运行未产生新的仓库污染。

# 附：修复结果（2026-09-13 02:00 已实施并复验）

| # | 项 | 状态 | 复验结果 |
|---|----|------|---------|
| 1 | `session_id` 透传 | ✅ 已修 | 传入 `multi-1` → meta 返回 `session_id=multi-1` |
| 2 | MCP `notifications/progress` | ✅ 已接 | 单次请求收到 **8 条**阶段通知，phase 齐全 |
| 3 | 去掉 PG 探活 | ✅ 已改 | 冷启动 **~6s → 2.97s** |
| 4 | `skill_run` 兜底 | ✅ 已修 | 空错误已变为可读文案 |
| 5 | 注册进 `~/.workbuddy/mcp.json` | ✅ 已加 | 需在连接器管理页点「信任」后生效 |

**代码改动**

1. `src/adapter/inbound/mcp.rs`
   - `session_id`：优先取调用方入参，为空才生成新 UUID
   - 新增 `ProgressNotifier`：`handle()` 从 `_meta.progressToken`（缺失则用 request id）取令牌，把 `StreamEvent::Step` 转成标准 `notifications/progress`（`progressToken` + 自增 `progress` + `message`）
   - `subhuti_chat` 改走 **`orchestrate_stream`**（与 HTTP SSE 同源），显式生成 `trace_id` 让 `ProgressEventBridge` 能把框架事件路由回本请求
   - `skill_run` 透传 `workspace_folder` / `session_id`，错误为空时给出可读兜底
2. `src/application/composition_root.rs`
   - PG 探活改为**默认关闭**，需 `SUBHUTI_PG_ENABLED=true` 显式开启（本机无 PG 时行为与"超时降级"完全一致，只是不再白等 5 秒）

**实测阶段通知样例**（单次 chat）

```
🔗 编排开始
⚡ 【框架】[analyze] 分析任务中...
⚡ 【框架】[run] 开始执行...
⚡ 【Blender 动画专家】[route] 🧭 匹配专家: Blender 动画专家
⚡ 【Blender 动画专家】[retrieve] 📚 检索记忆: ... (0 条)
⚡ 【Blender 动画专家】[think] 🤔 模型推理中…
⚡ 【框架】[done] 专家执行完成: Blender 动画专家
🏁 编排完成 · 1880ms
```

---

# 附 2：多轮上下文（SQLite 持久化）已实现 — 2026-09-13 02:35

原「遗留项」已关闭：多轮上下文现在**跨进程、跨重启**生效。

## 方案

会话消息落 SQLite（`sessions.sqlite`，与 traces 同目录，`SUBHUTI_SESSION_SQLITE` 可覆盖），
注入点选在 **LLM 唯一出口 `SubhutiLlmAdapter`**——因为 Blender 专家自身不读 `ctx.history`
（只拼 system+user），只有在这一层注入才能覆盖所有专家与所有路径。

| 环节 | 位置 | 说明 |
|------|------|------|
| 存储层 | `crates/subhuti-infra/src/session_store.rs`（新增） | `session_messages(id, session_id, role, content, created_at)`；照 `trace_store` 的「独立 OS 线程 + current_thread runtime + std_mpsc」同步桥，避免 runtime 内 block_on panic |
| 路径 | `crates/subhuti-infra/src/data_dir.rs` | 新增 `session_db_path()` |
| 落盘 | `orchestration_service.rs` | `orchestrate` 与 `orchestrate_stream` 均在开头写 user、成功后写 assistant |
| 注入 | `domain_expert_adapter.rs::SubhutiLlmAdapter::with_history` | 历史插在 system 之后、本轮 user 之前；**按 role+content 去重**（rust_expert 自带 `ctx.history`，内容一致时自动跳过，不会重复） |
| 装配 | `composition_root.rs` + `subhuti_framework_initializer.rs` | store 必须在**注册专家之前** `set_session_store`，否则适配器拿不到（踩过这个坑） |

条数上限 `HISTORY_INJECT_LIMIT = 6`（约最近 3 轮）。

## 复验

| 场景 | 结果 |
|------|------|
| MCP：进程A 说「我叫欧阳铁柱」→ **全新进程B** 问「我的名字是什么」 | ✅ 答「欧阳铁柱」（修复前答「你的名字是用户」） |
| HTTP：同 session_id 两轮 curl（「我叫诸葛亮」→「我的名字是什么」） | ✅ 答「诸葛亮」 |
| SQLite 落盘内容 | ✅ user/assistant 成对写入，可按 session 查询 |

`cargo test -p subhuti-app --lib` 24 passed / 0 failed。

---

**⚠️ 遗留：多轮上下文仍不生效（非 MCP 层问题）**

`session_id` 已正确透传，但第二轮仍答不出"小明"。根因：**框架层没有会话历史机制** —— 全仓 grep `session_history` / `load_history` / `get_history` 均无匹配，编排每次都是单轮调用 LLM，不会带上历史消息。

要真正支持多轮，需要新增能力（非 MCP 适配器能解决）：按 `session_id` 落/读历史消息（现成的 `session_observer` + SQLite trace 可承载），在 `orchestrate` 组装 messages 时注入最近 N 轮。建议单独立项。

**其他验证**：`cargo check --workspace --all-targets` 通过；`cargo test -p subhuti-app --lib` 24 passed / 0 failed。

---

## 一、功能验证矩阵

| # | 工具 | 结果 | 实测耗时 | 说明 |
|---|------|------|---------|------|
| ① | `subhuti_list_experts` | ✅ 正常 | 0 ms | 返回 2 个专家 + tags（Blender 动画专家 / Rust 编程专家） |
| ② | `subhuti_match_expert` | ✅ 正常 | 0 ms | "写 Rust 判断素数" → 正确命中 `rust-expert` |
| ③ | `subhuti_skill_list` | ✅ 正常 | 1 ms | 返回 12 个技能（blender 5 + rust 7） |
| ④ | `subhuti_chat` | ✅ 正常 | 0.9 s ~ 94 s | 主力链路，答案质量与 HTTP 一致，meta 带专家链/耗时/trace_id |
| ⑤ | `subhuti_skill_run` | ⚠️ 有条件可用 | 158 ms | `args` **必须是字符串**（传对象会得到空错误）；`skill_id` 必填 |

错误处理验证：

| 场景 | 返回 | 评价 |
|------|------|------|
| 未知工具名 `subhuti_nope` | `❌ 工具执行失败: 未知工具: subhuti_nope` | ✅ 正确 |
| 缺必填 `message` | `❌ 工具执行失败: 缺少必填参数 'message'` | ✅ 正确 |
| `skill_run` 缺 `skill_id` | `❌ 工具执行失败: 缺少必填参数 'skill_id'` | ✅ 正确 |
| `skill_run` 传错 `args` 类型 | `❌ 技能执行失败: `（**错误信息为空**） | ⚠️ 缺陷 C |

---

## 二、性能实测（MCP vs HTTP 同 prompt 对比）

| 场景 | MCP | HTTP(SSE) | 结论 |
|------|-----|-----------|------|
| 短问答"只回复 OK 两字" | 1.2 s（后端 1182ms） | 0.60 s（后端 598ms） | 同量级（LLM 抖动） |
| 自我介绍"你好，介绍一下你自己" | 14.7 s / 20.3 s | 15.2 s（15240ms） | **基本一致** |
| Rust 编码任务（生成+编译验证） | **94.2 s** | 同等量级 | 长任务偏慢 |
| 进程启动到就绪 | **约 6 s** | 常驻进程 | MCP 每次启动都要付 |

关键观察：

1. **MCP 与 HTTP 的编排耗时没有实质差距** —— 二者走同一个 `chat_port.orchestrate()`，耗时由 LLM 决定。
2. **MCP 进程冷启动约 6 秒**，其中 **5 秒是 PostgreSQL 连接超时等待**（日志明确：`PostgreSQL 连接超时（5秒），降级为内存数据仓库`）。stdio MCP 每次被客户端拉起都要付这笔开销 —— 这正好命中记忆里的既有债"删 PG 省 5s 启动"。
3. **并发**：服务端声明 `并发上限=2`，实测同时发 3 个 chat → 3 个**全部成功**（1.18s / 0.90s / 1.89s），即超限是**排队而非拒绝**，行为合理。

---

## 三、最重要的一条：MCP 完全没有阶段流

同一条短请求，两条链路的可观测性差异巨大：

```
HTTP(SSE)： start → step ×6 → data → step → done     ← 阶段可见（analyze/run/route/retrieve/think/done）
MCP     ： （静默等待）→ 一次性返回最终结果              ← 0 条通知
```

实测统计：整个 MCP 会话期间收到 **`notifications` 数量 = 0**。

代码层面已确认原因：

- `src/adapter/inbound/mcp.rs` 中 **没有消费** 编排层 `orchestrate_stream` 返回的 `StreamEvent` 流（含 `ProgressEventBridge` 经 EventBus 桥接、专家经 `emit_*` 发出的阶段事件），MCP 侧拿不到进度出口，事件全部丢弃
- `mcp.rs` 里也没有任何 `notifications/progress` 相关实现

**影响**：长任务（如 94 秒的编码任务）对 MCP 客户端是纯黑盒，用户/客户端看不到"匹配专家 → 推理 → 生成 → 编译验证"的过程，体感就是"卡住了"，且容易触发客户端侧超时。

**好消息**：修复成本很低 —— 阶段事件本来就由 `ProgressEventBridge` 从 EventBus 统一产出（`agent_matched/llm_calling/tool_calling/memory_retrieved`），MCP 侧只需注册一个 sender，把 step 事件转成标准 `notifications/progress`（带 `progressToken`）或 `notifications/message`（logging）即可，**与 SSE 同源，不需要改编排层**。前端刚做的 phase/time/duration 语义也正好可以复用。

---

## 四、缺陷清单

### 【P0】`session_id` 被忽略 → 多轮对话上下文断裂

- **位置**：`src/adapter/inbound/mcp.rs:271`
  ```rust
  session_id: Some(uuid::Uuid::new_v4().to_string()),  // ← 忽略调用方传入的 session_id
  ```
- **schema 却声明** `session_id: 会话 ID，用于多轮上下文（可选）`，属于**文档与实现不符**
- **实测复现**（同一 session 连问两轮）：
  ```
  传入 session_id = fixed-session-123
  第1轮 → meta session_id = 74f8ce34-…（随机）
  第2轮 → meta session_id = 6dd14ca2-…（又随机，两轮不一致）
  第2轮问"我叫什么名字？"（第1轮已告知"我叫小明"）→ 回答："你的名字是'用户'"  ← 上下文丢失
  ```
- **修复**：改为 `args.get("session_id").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| Uuid::new_v4().to_string())`

### 【P1】MCP 无阶段流（详见第三节）

### 【P2】`skill_run` 两处小问题

1. `workspace_folder` / `session_id` 恒传空串（`mcp.rs:354` → `execute_skill(skill_id, args_str, "", "")`），导致 `rust-coding` 这类**依赖工作目录的技能无法工作**
2. `resp.error` 为 `None` 时错误文案为空（实测出现 `❌ 技能执行失败: ` 空消息），建议兜底文案

---

## 五、与浏览器链路的能力对照

| 能力 | 浏览器(HTTP SSE) | MCP(stdio) |
|------|------------------|------------|
| 编排/专家路由 | ✅ | ✅（一致） |
| 最终答案质量 | ✅ | ✅（一致） |
| 阶段流（phase/source/tool/retrieve） | ✅ 完整 | ❌ 无 |
| 单步耗时 / 总耗时 | ✅ `duration_ms` | ✅ meta 里有（仅总计） |
| 多轮上下文 | ✅ | ❌ session_id 被忽略 |
| 技能执行 | — | ⚠️ 无工作目录 |
| 冷启动成本 | 常驻 | 每次 ~6s（含 5s PG 超时） |

---

## 六、建议的下一步（按性价比排序）

1. **修 `session_id`**（1 行改动，立刻让 MCP 支持多轮）
2. **给 MCP 接 `notifications/progress`**：复用 `ProgressEventBridge`（EventBus→ProgressEvent）与编排层 `StreamEvent` 流，把 step 事件映射为 MCP 通知；前端刚定义的 phase 语义可直接复用
3. **去掉 PG 探活**（省 5 秒冷启动，对 stdio MCP 收益明显）—— 既有债，正好在此闭环
4. **`skill_run` 透传 `workspace_folder`/`session_id`** + 空错误兜底
5. 修完后把 subhuti 注册进 `~/.workbuddy/mcp.json`（当前只注册了 blender-mcp），即可在 WorkBuddy 里直接当 MCP 工具用：
   ```json
   "subhuti": {
     "command": "/Users/hezenghui/RustroverProjects/subhuti-app/target/debug/subhuti",
     "args": ["mcp"]
   }
   ```
   （注意：`.env` 只在项目目录内被读到，从别处启动会退回默认数据目录，建议用 `cwd` 或显式 `env` 指定 `SUBHUTI_DATA_DIR`）
