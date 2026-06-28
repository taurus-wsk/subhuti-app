# Subhuti 调试工具使用指南

## 📖 概述

本指南介绍 Subhuti 框架提供的调试工具，帮助开发者快速定位和解决问题。

## 🛠️ 核心工具

### 1. 诊断宏 `diagnose!`

打印变量的详细信息，包括变量名、值和类型。

```rust
use subhuti::diagnose;

let x = 42;
diagnose!(x);  // 输出: [DIAGNOSE] file.rs:10: x = 42 (type: i32)

let name = "Alice";
diagnose!(name = name);  // 输出: [DIAGNOSE] file.rs:12: name = "Alice" (type: &str)
```

**适用场景**：
- 变量值不确定时
- 需要查看变量类型时
- 调试表达式结果时

### 2. 断言宏 `assert_that!`

带上下文的断言，失败时提供更清晰的错误信息。

```rust
use subhuti::assert_that;

assert_that!(result.is_ok(), "Operation should succeed");
assert_that!(count > 0);
```

**优势**：
- 显示断言所在文件和行号
- 显示断言的完整表达式
- 自定义错误消息

### 3. 计时宏 `time_it!`

测量代码块执行时间。

```rust
use subhuti::time_it;

let result = time_it!("Database query" => {
    // 你的代码
    std::thread::sleep(std::time::Duration::from_millis(100));
    42
});
// 输出: [TIMING] Database query: file.rs:15 took 0.103s
```

### 4. 结构调试 `debug_struct!`

打印复杂数据结构的完整信息。

```rust
use subhuti::debug_struct;

#[derive(Debug)]
struct User {
    id: u32,
    name: String,
}

let user = User { id: 1, name: "Alice".to_string() };
debug_struct!(user, user);
// 输出完整的 Debug 格式化输出
```

## 🏥 健康检查系统

### 基本使用

```rust
use subhuti::{Subhuti, HealthReport};

let subhuti = Subhuti::new();

// 获取健康报告
let report = subhuti.health_check();

// 打印到控制台
subhuti.print_health_report();

// 编程式使用
if !report.overall_healthy {
    for component in &report.components {
        if !component.healthy {
            eprintln!("❌ {} failed: {:?}", component.name, component.details);
        }
    }
}
```

### HTTP API 端点

- `GET /subhuti/api/v1/health` - 简单健康状态
- `GET /subhuti/api/v1/health/detailed` - 详细组件状态

### 健康报告示例输出

```
╔══════════════════════════════════════════════════════════════╗
║                    SYSTEM HEALTH REPORT                       ║
╚══════════════════════════════════════════════════════════════╝

✅ MemoryPalace - OK
   ├─ total_memories: 15
   ├─ short_term: 3
   ├─ archive: 10
   └─ knowledge: 2

✅ Database - OK
   └─ connected: true

✅ SoulLayer - OK
   ├─ persona_version: 1
   ├─ persona_name: Subhuti
   ├─ total_interactions: 42
   └─ interactions_since_evolve: 12

✅ ExpertPlugins - OK
   ├─ plugin_count: 2
   └─ active_expert: psychology

✅ Skills - OK
   └─ skill_count: 5

══════════════════════════════════════════════════════════════════
Overall: ✅ ALL SYSTEMS HEALTHY
Timestamp: 2024-01-15 10:30:45
```

## 📊 测试追踪器

### 基本使用

```rust
use subhuti::{TestTracker, assert_that};

fn main() {
    let mut tracker = TestTracker::new();
    
    // 运行测试
    if test_something() {
        tracker.pass("test_something");
    } else {
        tracker.fail("test_something", "Assertion failed");
    }
    
    // 输出总结
    println!("{}", tracker.summary());
}

fn test_something() -> bool {
    true
}
```

### 输出示例

```
[TEST OK] test_something
[TEST FAIL] test_connection - Connection timeout
[TEST OK] test_authentication

❌ 2/3 tests passed in 0.234s
Failed tests:
test_connection: Connection timeout
```

## 🔍 性能分析器

```rust
use subhuti::Profiler;

let mut profiler = Profiler::new();

// 记录性能数据
let start = std::time::Instant::now();
// ... 执行操作 ...
profiler.record("search", start.elapsed());

let start = std::time::Instant::now();
// ... 执行另一个操作 ...
profiler.record("store", start.elapsed());

// 输出性能报告
profiler.report();
```

### 输出示例

```
📊 PERFORMANCE PROFILE
═════════════════════════════════════

search:
  ├─ Calls: 100
  ├─ Total: 234.567ms
  ├─ Avg:   2.346ms
  ├─ Min:   0.123ms
  └─ Max:   15.678ms

store:
  ├─ Calls: 50
  ├─ Total: 123.456ms
  ├─ Avg:   2.469ms
  ├─ Min:   0.234ms
  └─ Max:   8.901ms
```

## 🧵 锁竞争检测器

用于诊断死锁问题。

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

## 💡 最佳实践

### 1. 调试时使用 `diagnose!`

```rust
fn complex_function(data: Vec<String>) -> Result<String> {
    diagnose!(data.len());
    
    let filtered: Vec<_> = data.iter().filter(|s| s.len() > 3).collect();
    diagnose!(filtered.len());
    
    Ok(filtered.join(", "))
}
```

### 2. 性能关键路径使用 `time_it!`

```rust
async fn search_memories(&self, query: &str) -> Vec<Memory> {
    time_it!("Memory search" => {
        let results = self.palace.search(query, 10, None);
        results
    })
}
```

### 3. 定期健康检查

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

### 4. 测试中的错误定位

```rust
#[test]
fn test_palace_search() {
    let palace = MemoryPalace::new();
    
    // 存储测试数据
    let id = palace.store("test content".to_string(), MemoryLayer::Archive, None).unwrap();
    diagnose!(id = id);
    
    // 搜索
    let results = palace.search("test", 10, None);
    diagnose!(results.len() = results.len());
    
    // 带上下文的断言
    assert_that!(!results.is_empty(), "Should find stored memory");
    assert_that!(
        results[0].memory.base.content.contains("test"),
        "Content should match"
    );
}
```

## 🎯 调试场景

### 场景 1: 死锁诊断

```rust
use subhuti::diagnose;

// 在关键位置添加诊断
pub fn search(&self, query: &str) -> Vec<Result> {
    diagnose!("Acquiring lock");
    let data = self.data.read().unwrap();
    diagnose!("Lock acquired");
    
    // ... 搜索逻辑 ...
    
    diagnose!("Releasing lock");
    // 如果在 "Lock acquired" 和 "Releasing lock" 之间卡住
    // 说明存在死锁或长任务
}
```

### 场景 2: 性能瓶颈

```rust
pub async fn process_items(&self, items: Vec<Item>) -> Vec<Result> {
    let mut results = Vec::new();
    
    for item in items {
        let start = std::time::Instant::now();
        
        let result = self.process_item(item).await;
        
        // 使用 time_it! 测量每个项的处理时间
        time_it!("Item processing" => {
            results.push(result);
        });
        
        if start.elapsed() > std::time::Duration::from_secs(1) {
            eprintln!("⚠️  Slow item detected: {:?}", start.elapsed());
        }
    }
    
    results
}
```

### 场景 3: 状态追踪

```rust
use subhuti::debug_struct;

pub fn update_state(&mut self, new_state: State) {
    debug_struct!(old_state, &self.state);
    
    self.state = new_state;
    
    debug_struct!(new_state, &self.state);
    
    // 验证状态转换
    assert_that!(
        self.is_valid_state(),
        "State transition should be valid"
    );
}
```

## 📝 总结

Subhuti 框架提供的调试工具涵盖：

- ✅ **快速诊断** - 了解变量状态和类型
- ✅ **性能分析** - 定位性能瓶颈  
- ✅ **健康检查** - 监控系统组件状态
- ✅ **测试辅助** - 清晰的测试报告
- ✅ **锁竞争检测** - 诊断死锁问题

这些工具帮助开发者快速定位问题，提高调试效率。建议在开发过程中灵活使用这些工具，保持代码的可维护性。
