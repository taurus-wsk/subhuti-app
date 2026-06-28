# Subhuti 调试工具使用指南 v2.0

> 适用于 AI Agent 和人类开发者的调试工具包

---

## 📖 概览

Subhuti 提供了一套完整的调试工具，帮助你快速定位问题、测量性能、监控系统状态。

### 工具清单

| 工具 | 类型 | 适用场景 | 性能影响 |
|------|------|----------|----------|
| `diagnose!` / `diagnose_value()` | 诊断 | 查看变量值和类型 | 无 |
| `time_it!` / `measure_time()` | 性能 | 测量代码块执行时间 | 极低 |
| `debug_struct!` / `debug_print()` | 结构 | 打印复杂数据结构 | 无 |
| `assert_that!` / `assert_with_context()` | 断言 | 带上下文的断言验证 | 无 |
| `HealthReport` | 系统 | 一键检查所有组件状态 | 极低 |
| `TestTracker` | 测试 | 结构化测试结果追踪 | 无 |
| `Profiler` | 性能 | 累积性能数据分析 | 低 |
| `LockDetector` | 调试 | 锁竞争和死锁诊断 | 低 |

---

## 🛠️ 快速开始

### 在代码中使用

```rust
use subhuti::{diagnose, time_it, HealthReport, TestTracker};

fn main() {
    // 1. 诊断变量
    let data = fetch_data();
    diagnose!(data = data);
    
    // 2. 测量性能
    let result = time_it!("Data processing" => {
        process(data)
    });
    
    // 3. 健康检查
    let subhuti = Subhuti::new();
    subhuti.print_health_report();
}
```

### 在测试中使用

```rust
// tests/integration_test.rs
use subhuti::debug::{diagnose_value, measure_time, debug_print};

fn test_example() {
    // 函数版本，在 tests/ 目录可用
    diagnose_value("variable_name", &variable);
    
    let result = measure_time("Operation name", || {
        // 代码块
    });
}
```

---

## 📝 详细使用说明

### 1. 诊断宏 `diagnose!`

#### 宏版本（推荐在 lib.rs 和内部模块使用）

```rust
let x = 42;
diagnose!(x);
// 输出: [DIAGNOSE] src/main.rs:10: x = 42 (type: i32)

let name = "Alice";
diagnose!(name = name);
// 输出: [DIAGNOSE] src/main.rs:12: name = "Alice" (type: &str)
```

#### 函数版本（推荐在 tests/ 和 examples/ 使用）

```rust
use subhuti::debug::diagnose_value;

let data = vec![1, 2, 3];
diagnose_value("my_vector", &data);
// 输出: [DIAGNOSE] my_vector = [1, 2, 3] (type: alloc::vec::Vec<i32>)
```

**适用场景**：
- 变量值不确定时
- 需要了解变量类型时
- 调试表达式结果时
- 快速了解函数返回值时

---

### 2. 计时宏 `time_it!`

#### 宏版本

```rust
let result = time_it!("Database query" => {
    std::thread::sleep(std::time::Duration::from_millis(100));
    42
});
// 输出: [TIMING] Database query: src/main.rs:15 took 100.123ms
```

#### 函数版本

```rust
use subhuti::debug::measure_time;

let result = measure_time("Expensive operation", || {
    expensive_computation()
});
// 输出: [TIMING] Expensive operation took 1.234s
```

**适用场景**：
- 测量 API 调用耗时
- 定位性能瓶颈
- 验证优化效果
- 代码审查时评估复杂度

---

### 3. 结构调试 `debug_struct!`

#### 宏版本

```rust
use subhuti::debug::debug_struct;

#[derive(Debug)]
struct User {
    id: u32,
    name: String,
    scores: Vec<u32>,
}

let user = User {
    id: 1,
    name: "Alice".to_string(),
    scores: vec![95, 87, 92],
};

debug_struct!(user, user);
/*
输出:
[DEBUG] src/main.rs:25: user =
User {
    id: 1,
    name: "Alice",
    scores: [
        95,
        87,
        92,
    ],
}
*/
```

#### 函数版本

```rust
use subhuti::debug::debug_print;

debug_print("config_data", &config);
// 输出完整的 Debug 格式化内容
```

**适用场景**：
- 查看复杂数据结构的完整内容
- 调试嵌套结构
- 分析配置对象
- 验证数据结构是否符合预期

---

### 4. 断言宏 `assert_that!`

#### 宏版本

```rust
use subhuti::assert_that;

// 带消息的断言
assert_that!(result.is_ok(), "Operation should succeed");

// 简洁断言
assert_that!(count > 0);
assert_that!(!vec.is_empty(), "Vector should not be empty");
```

#### 函数版本

```rust
use subhuti::debug::assert_with_context;

assert_with_context(
    condition,
    "Expected condition to be true",
    file!(),
    line!()
);
```

**适用场景**：
- 单元测试中的断言
- 验证前置条件
- 验证后置条件
- 防御性编程

---

### 5. 健康检查 `HealthReport`

#### 基础使用

```rust
use subhuti::{Subhuti, HealthReport};

let subhuti = Subhuti::new();

// 获取健康报告
let report = subhuti.health_check();

// 编程式使用
if !report.overall_healthy {
    for component in &report.components {
        if !component.healthy {
            eprintln!("❌ {} failed: {:?}", component.name, component.details);
        }
    }
}

// 打印格式化报告
subhuti.print_health_report();
```

#### HTTP API

```bash
# 简单健康状态
curl http://localhost:8080/subhuti/api/v1/health

# 详细组件状态
curl http://localhost:8080/subhuti/api/v1/health/detailed
```

#### 输出示例

```
╔══════════════════════════════════════════════════════════════╗
║                    SYSTEM HEALTH REPORT                       ║
╚══════════════════════════════════════════════════════════════╝

✅ MemoryPalace - OK
   ├─ total_memories: 15
   ├─ short_term: 3
   ├─ archive: 10
   └─ knowledge: 2

🟡 Database - OPTIONAL
   ├─ enabled: false
   └─ reason: Not configured (optional component)

✅ SoulLayer - OK
   ├─ persona_version: 1
   ├─ persona_name: Subhuti
   └─ total_interactions: 42

══════════════════════════════════════════════════════════════════
Overall: ✅ ALL SYSTEMS HEALTHY
Timestamp: 2026-06-27 18:00:00
```

**适用场景**：
- 系统启动检查
- 定期健康监控
- 故障诊断
- 运维监控

---

### 6. 测试追踪器 `TestTracker`

#### 基本使用

```rust
use subhuti::TestTracker;

fn main() {
    let mut tracker = TestTracker::new();
    
    // 运行测试
    if test_something() {
        tracker.pass("test_something");
    } else {
        tracker.fail("test_something", "Assertion failed");
    }
    
    if test_another_thing() {
        tracker.pass("test_another_thing");
    }
    
    // 输出总结
    println!("{}", tracker.summary());
}
```

#### 输出示例

```
[TEST OK] test_something
[TEST FAIL] test_connection - Connection timeout
[TEST OK] test_authentication

❌ 2/3 tests passed in 0.234s
Failed tests:
test_connection: Connection timeout
```

**适用场景**：
- 集成测试的测试管理
- 批量测试的结果追踪
- 测试套件的健康度检查
- CI/CD 流程中的测试报告

---

### 7. 性能分析器 `Profiler`

```rust
use subhuti::Profiler;

let mut profiler = Profiler::new();

// 记录性能数据
let start = std::time::Instant::now();
let result = fetch_data();
profiler.record("Data fetch", start.elapsed());

let start = std::time::Instant::now();
let result = process_data();
profiler.record("Data process", start.elapsed());

// 输出性能报告
profiler.report();
```

#### 输出示例

```
📊 PERFORMANCE PROFILE
═════════════════════════════════════

Data fetch:
  ├─ Calls: 100
  ├─ Total: 234.567ms
  ├─ Avg:   2.346ms
  ├─ Min:   0.123ms
  └─ Max:   15.678ms

Data process:
  ├─ Calls: 100
  ├─ Total: 123.456ms
  ├─ Avg:   1.235ms
  ├─ Min:   0.234ms
  └─ Max:   8.901ms
```

**适用场景**：
- 性能回归测试
- 关键路径分析
- 优化效果验证
- 性能基准测试

---

### 8. 锁竞争检测器 `LockDetector`

```rust
use subhuti::LockDetector;

let detector = LockDetector::new();

// 在获取锁之前
detector.record_lock("RwLock");

// ... 使用锁 ...

// 释放锁之后
detector.release_lock("RwLock");

// 检查当前持有的锁
let held = detector.get_held_locks();
if !held.is_empty() {
    eprintln!("⚠️  Potential lock contention: {:?}", held);
}
```

**适用场景**：
- 死锁诊断
- 锁竞争分析
- 并发调试
- 性能调优

---

## 💡 最佳实践

### 1. 调试时使用 `diagnose!`

```rust
fn complex_function(data: Vec<String>) -> Result<String> {
    diagnose!(data.len());  // 快速查看长度
    
    let filtered: Vec<_> = data.iter()
        .filter(|s| s.len() > 3)
        .collect();
    diagnose!(filtered.len());  // 验证过滤效果
    
    Ok(filtered.join(", "))
}
```

### 2. 性能关键路径使用 `time_it!`

```rust
pub async fn search_memories(&self, query: &str) -> Vec<Memory> {
    time_it!("Memory search" => {
        let results = self.palace.search(query, 10, None);
        results
    })
}
```

### 3. 系统级调试使用 `HealthReport`

```rust
// 在应用启动时
let report = subhuti.health_check();
if !report.overall_healthy {
    eprintln!("⚠️  System started with unhealthy components");
    for component in &report.components {
        if !component.healthy {
            eprintln!("  - {}: {:?}", component.name, component.details);
        }
    }
}
```

### 4. 测试中使用 `TestTracker`

```rust
#[tokio::test]
async fn test_integration() {
    let subhuti = Subhuti::new();
    let mut tracker = TestTracker::new();
    
    // 健康检查
    let report = subhuti.health_check();
    if report.overall_healthy {
        tracker.pass("Health check");
    } else {
        tracker.fail("Health check", "System unhealthy");
    }
    
    // 更多测试...
    
    assert!(tracker.summary().contains("passed"));
}
```

---

## 🎯 调试场景

### 场景 1: 死锁诊断

```rust
use subhuti::debug::{diagnose_value, LockDetector};

pub fn search(&self, query: &str) -> Vec<Result> {
    diagnose_value("Acquiring lock", &true);
    let data = self.data.read().unwrap();
    diagnose_value("Lock acquired", &true);
    
    // 如果在这里卡住，说明可能有死锁
    let results = self.process(data, query);
    
    diagnose_value("Releasing lock", &true);
    drop(data);
    
    results
}
```

### 场景 2: 性能瓶颈

```rust
pub async fn process_items(&self, items: Vec<Item>) -> Vec<Result> {
    let mut profiler = Profiler::new();
    let mut results = Vec::new();
    
    for item in items {
        let start = std::time::Instant::now();
        let result = self.process_item(item).await;
        profiler.record("Item processing", start.elapsed());
        
        if start.elapsed() > std::time::Duration::from_secs(1) {
            eprintln!("⚠️  Slow item detected");
        }
        
        results.push(result);
    }
    
    profiler.report();
    results
}
```

### 场景 3: 集成测试

```rust
// tests/integration_test.rs
use subhuti::debug::{diagnose_value, measure_time, TestTracker};

fn test_full_system() {
    let mut tracker = TestTracker::new();
    
    // 初始化
    let start = std::time::Instant::now();
    let subhuti = Subhuti::new();
    measure_time("Subhuti init", || subhuti);
    
    // 健康检查
    let report = subhuti.health_check();
    if report.overall_healthy {
        tracker.pass("Health check");
    } else {
        tracker.fail("Health check", "System unhealthy");
    }
    
    // 更多测试...
    
    println!("{}", tracker.summary());
}
```

---

## 📊 性能数据

### 调试工具的性能开销

| 工具 | 单次调用开销 | 说明 |
|------|------------|------|
| `diagnose!` | ~1-5μs | 极低，主要是字符串格式化 |
| `time_it!` | ~1-3μs | 极低，主要是计时 |
| `debug_struct!` | ~10-50μs | 取决于结构体复杂度 |
| `HealthReport` | ~100-500μs | 取决于组件数量 |
| `TestTracker` | ~1-10μs | 极低，主要是计数 |

**结论**：调试工具的性能开销可以忽略不计，可以放心使用。

---

## 🔧 宏版本 vs 函数版本

| 特性 | 宏版本 | 函数版本 |
|------|--------|----------|
| 使用场景 | lib.rs 和内部模块 | tests/ 和 examples/ |
| 变量名推断 | ✅ 自动推断 | ❌ 需手动传入 |
| 类型推断 | ✅ 自动推断 | ✅ 自动推断 |
| 文件/行号 | ✅ 自动记录 | ❌ 需手动传入 |
| 编译时展开 | ✅ 是 | ❌ 否 |

**推荐**：
- 在 **lib.rs** 和 **内部模块**：使用宏版本（`diagnose!`）
- 在 **tests/** 和 **examples/**：使用函数版本（`diagnose_value()`）

---

## 📚 相关文档

- [DEBUG_TOOLS_SUMMARY.md](../DEBUG_TOOLS_SUMMARY.md) - 调试工具实践总结
- [INTEGRATION_TEST_REPORT.md](../INTEGRATION_TEST_REPORT.md) - 集成测试报告
- [src/debug.rs](../../crates/subhuti/src/debug.rs) - 调试工具源码
- [tests/integration_test.rs](../../crates/subhuti/tests/integration_test.rs) - 集成测试示例
- [tests/performance_test.rs](../../crates/subhuti/tests/performance_test.rs) - 性能基准测试

---

## 🧪 测试文件说明

Subhuti 框架包含完整的测试体系，分为单元测试、集成测试和性能测试。

### 单元测试（49 个）

散布在各源码模块的 `#[cfg(test)]` 中，覆盖所有核心组件：

| 模块 | 测试内容 |
|------|----------|
| `lib.rs` | 基础创建、配置 |
| `soul/palace.rs` | 分区推断、重要性估计、存储搜索、激活衰减、遗忘周期 |
| `memory/mod.rs` | 记忆存储、搜索、过期机制 |
| `memory/embedding.rs` | Embedding 服务 |
| `skill/mod.rs` | Skill 注册、匹配、关键词索引 |
| `flow/mod.rs` | Flow 管理、注册、执行 |
| `expert/mod.rs` | 专家插件生命周期、钩子调用 |
| `expert/planning.rs` | 规划系统 |
| `runtime/` | LLM 客户端、重试机制、约束护栏 |
| `observe/trace.rs` | Trace 追踪、Span 树 |

运行所有单元测试：
```bash
cargo test -p subhuti
```

### 集成测试（10 + 9 个）

| 文件 | 内容 | 运行命令 |
|------|------|----------|
| `tests/integration_test.rs` | 10 个子测试：初始化、健康检查、心灵宫殿存储/搜索/遗忘/分区、人格系统、Skills、专家插件 | `cargo test -p subhuti --test integration_test -- --nocapture` |
| `tests/test_debug_tools.rs` | 9 个测试：诊断宏、断言宏、计时宏、TestTracker、HealthReport、Palace 诊断 | `cargo test -p subhuti --test test_debug_tools -- --nocapture` |

### 性能测试（10 个基准）

`tests/performance_test.rs` 使用框架内置的 `Profiler`、`TestTracker` 等工具，对核心组件进行系统化性能基准测试。

| 测试项 | 基准阈值 | 说明 |
|--------|----------|------|
| 框架初始化 | < 5ms | `Subhuti::new()` 平均耗时 |
| 宫殿存储 | < 5s / 1000条 | `MemoryPalace::store()` 批量写入 |
| 宫殿搜索 | < 10ms/次 | 普通关键词搜索 |
| 人格加权搜索 | < 10ms/次 | 带分区偏好的搜索 |
| 遗忘周期 | < 100ms / 500条 | `run_forget_cycle()` 清理弱记忆 |
| 大规模搜索 | < 20ms/次 | 1000 条记忆下的搜索 |
| 健康检查 | < 500μs/次 | `health_check()` 组件状态检查 |
| Skill 列表 | < 100μs/次 | `list_skills()` 获取 |
| 分区推断 | < 10μs/次 | `MemoryZone::infer_from_content()` |
| 记忆生命周期 | < 10s / 10万轮 | `PalaceMemory` 衰减+激活循环 |

运行性能测试：
```bash
cargo test -p subhuti --test performance_test -- --nocapture
```

### 同步测试

`src/bin/sync_test.rs` 用于测试 LLM 同步调用（需要配置 API Key）：
```bash
cargo run --bin sync_test
```

### 运行所有测试

```bash
# 运行所有测试（单元 + 集成 + 调试工具 + 性能）
cargo test -p subhuti

# 查看测试输出
cargo test -p subhuti -- --nocapture

# 运行特定测试
cargo test -p subhuti --test integration_test
cargo test -p subhuti --test test_debug_tools
cargo test -p subhuti --test performance_test
```

---

## ✅ 总结

Subhuti 的调试工具系统提供了：

1. **快速诊断** - diagnose! 了解变量状态和类型
2. **性能分析** - time_it! 定位性能瓶颈
3. **结构查看** - debug_struct! 完整查看复杂数据
4. **系统监控** - HealthReport 实时掌握系统状态
5. **测试追踪** - TestTracker 结构化测试结果
6. **性能分析** - Profiler 累积性能数据统计
7. **锁诊断** - LockDetector 死锁和竞争检测

这套工具让 Subhuti 框架的调试更加高效、系统化，帮助你快速定位和解决问题。

**使用建议**：在开发过程中积极使用这些工具，保持代码的可维护性和可靠性。
