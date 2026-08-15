# Actor 标签匹配与测试

## 标签匹配机制

### 评分算法

`Actor::score(task_tags)` 在 [actor.rs](file:///Users/hezenghui/RustroverProjects/subhuti-app/crates/subhuti-core/src/orchestrator/actor.rs#L58-L75) 中实现：

```
task_tags 为空                        → 50 分（中性）
task_tags 有值，Actor 无匹配标签      → 0 分
匹配 N 个标签                         → N × 100 / task_tags.len() 分
```

例如节点 `generate` 要求 `[coding, rust, generation]`，Actor 拥有全部 3 个 → `3/3 × 100 = 100 分`

### 双向奔赴

| 角色 | 定义位置 | 内容 |
|------|----------|------|
| 图节点标签 | 图定义文件，如 `rust_programming.rs` | 用 `node_tag()` 声明任务要求 |
| Actor 标签 | 领域专家构造方法，如 `RustExpert::new()` | 专家注册时自动注入 Actor 池 |
| 匹配 | `ActorRegistry::find_best()` | 所有 Actor 自评，最高分者上台 |

### 标签映射关系

**图节点标签**（定义在 `rust_programming.rs`、`rust_edit.rs`）：

| 节点 | 标签 |
|------|------|
| plan / diff_plan | `planning`, `rust` |
| generate | `coding`, `rust`, `generation` |
| write_files / edit_files | `file_io` |
| verify | `verification`, `rust` |
| fix_errors | `debugging`, `rust` |
| analyze | `analysis`, `rust` |
| complete | `reporting` |

**RustExpert 标签**（定义在 `rust_expert.rs`）：

```
rust, code, programming, architecture,
planning, coding, generation, file_io,
verification, debugging, analysis, reporting
```

新增专家时，需确保 `tags()` 返回的标签能匹配到对应图节点的 `task_tags`，否则竞标会返回 0 分，导致 `"无 Actor 匹配节点"` 错误。

---

## 测试脚本

### 文件位置

`scripts/test/test_fn_call_trace.py`

### 功能

1. 发送 POST 请求到 `/subhuti/api/v1/orchestrate`
2. 自动打开最后一次请求的 Trace 函数调用报告

### 前提条件

- Python 3 + `requests` 库（`pip3 install requests`）
- Subhuti 服务已启动（推荐 mock 模式）

### 启动与测试

```bash
# 1. 启动服务（mock 模式，不打真实 LLM API）
make serve-mock

# 2. 运行测试
python3 scripts/test/test_fn_call_trace.py
```

### 预期输出

```
✅ 响应状态码: 200
📄 响应内容: {'success': True, 'data': {'chain': ['graph:rust_programming'], ...
✅ 已在浏览器中打开: http://127.0.0.1:8615/subhuti/api/v1/traces/last/fn_call_report
```

关键检查点：
- `success: True`
- `chain` 包含 `graph:rust_programming`（图匹配成功）
- `expert_chain` 包含所有节点名称（如 `plan → generate → write_files → verify → fix_errors → complete`）

### 常见错误及修复

| 错误 | 原因 | 修复 |
|------|------|------|
| `无 Actor 匹配节点 xxx` | Actor 标签与节点标签不匹配 | 扩展对应专家的 `tags` 字段 |
| `LLM 未配置` | `ExpertState` 未注入 LLM | 检查 `dispatch_with_context()` 是否调用了 `.optional_llm()` |
| `未匹配到合适的图` | 输入内容没有匹配到任何图 | 检查图注册和 `find_matching_graph` 逻辑 |