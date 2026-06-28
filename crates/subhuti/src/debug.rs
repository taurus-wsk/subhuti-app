//! # 调试工具模块
//!
//! 提供框架诊断、测试辅助和日志增强功能
//!
//! ## 主要功能
//!
//! - **诊断宏**：`diagnose!` - 打印变量类型和值
//! - **断言宏**：`assert_that!` - 带上下文的断言
//! - **计时宏**：`time_it!` - 代码块执行计时
//! - **健康检查**：`health_check()` - 系统各组件状态

use std::collections::HashMap;
use std::time::Instant;

/// 打印诊断信息（宏形式，方便开发调试）
#[macro_export]
macro_rules! diagnose {
    ($expr:expr) => {{
        let val = &$expr;
        let type_name = std::any::type_name_of_val(val);
        eprintln!(
            concat!("[DIAGNOSE] {}:{}: {} = {:?} (type: {})"),
            file!(),
            line!(),
            stringify!($expr),
            val,
            type_name
        );
        val
    }};
    ($name:ident = $expr:expr) => {{
        let val = &$expr;
        let type_name = std::any::type_name_of_val(val);
        eprintln!(
            concat!("[DIAGNOSE] {}:{}: {} = {:?} (type: {})"),
            file!(),
            line!(),
            stringify!($name),
            val,
            type_name
        );
        val
    }};
}

/// 诊断函数版本（不依赖宏导出，在 tests/ 和 examples/ 中可用）
pub fn diagnose_value<T: std::fmt::Debug>(name: &str, value: &T) {
    let type_name = std::any::type_name_of_val(value);
    eprintln!("[DIAGNOSE] {} = {:?} (type: {})", name, value, type_name);
}

/// 带上下文的断言
#[macro_export]
macro_rules! assert_that {
    ($condition:expr, $msg:expr) => {{
        if !($condition) {
            panic!(
                concat!("[ASSERT FAILED] {}:{}: Assertion \"{}\" failed: {}"),
                file!(),
                line!(),
                stringify!($condition),
                $msg
            );
        }
    }};
    ($condition:expr) => {{
        assert_that!($condition, "no message")
    }};
}

/// 带上下文的断言（函数版本）
pub fn assert_with_context(condition: bool, message: &str, file: &str, line: u32) {
    if !condition {
        eprintln!(
            "[ASSERT FAILED] {}:{}: Assertion failed: {}",
            file, line, message
        );
        panic!("Assertion failed: {}", message);
    }
}

/// 代码块执行计时
#[macro_export]
macro_rules! time_it {
    ($name:expr => $block:block) => {{
        let start = std::time::Instant::now();
        let result = $block;
        let elapsed = start.elapsed();
        eprintln!("[TIMING] {}: {} took {:.3?}", $name, file!(), elapsed);
        result
    }};
}

/// 计时函数版本（不依赖宏导出）
pub fn measure_time<F, R>(name: &str, f: F) -> R
where
    F: FnOnce() -> R,
{
    let start = std::time::Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    eprintln!("[TIMING] {} took {:.3?}", name, elapsed);
    result
}

/// 打印数据结构
#[macro_export]
macro_rules! debug_struct {
    ($name:ident, $value:expr) => {{
        let value = &$value;
        eprintln!(
            concat!("[DEBUG] {}:{}: {} =\n{:#?}"),
            file!(),
            line!(),
            stringify!($name),
            value
        );
    }};
}

/// 调试打印函数版本（不依赖宏导出）
pub fn debug_print<T: std::fmt::Debug>(name: &str, value: &T) {
    eprintln!("[DEBUG] {} =\n{:#?}", name, value);
}

/// 测试结果追踪器
pub struct TestTracker {
    passed: usize,
    failed: usize,
    failed_tests: Vec<String>,
    start_time: Instant,
}

impl TestTracker {
    pub fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
            failed_tests: Vec::new(),
            start_time: Instant::now(),
        }
    }

    pub fn pass(&mut self, name: &str) {
        self.passed += 1;
        eprintln!("[TEST OK] {}", name);
    }

    pub fn fail(&mut self, name: &str, reason: &str) {
        self.failed += 1;
        self.failed_tests.push(format!("{}: {}", name, reason));
        eprintln!("[TEST FAIL] {} - {}", name, reason);
    }

    pub fn summary(&self) -> String {
        let elapsed = self.start_time.elapsed();
        let total = self.passed + self.failed;

        if self.failed == 0 {
            format!(
                "✅ All {} tests passed in {:.3}s",
                total,
                elapsed.as_secs_f32()
            )
        } else {
            format!(
                "❌ {}/{} tests passed in {:.3}s\nFailed tests:\n{}",
                self.passed,
                total,
                elapsed.as_secs_f32(),
                self.failed_tests.join("\n")
            )
        }
    }
}

impl Default for TestTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// 组件健康状态
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub name: String,
    pub healthy: bool,
    /// 是否为可选组件（可选组件未启用不算故障）
    pub optional: bool,
    pub details: HashMap<String, String>,
}

impl HealthStatus {
    pub fn healthy(name: &str) -> Self {
        Self {
            name: name.to_string(),
            healthy: true,
            optional: false,
            details: HashMap::new(),
        }
    }

    pub fn unhealthy(name: &str, reason: &str) -> Self {
        let mut details = HashMap::new();
        details.insert("reason".to_string(), reason.to_string());
        Self {
            name: name.to_string(),
            healthy: false,
            optional: false,
            details,
        }
    }

    pub fn optional(name: &str, enabled: bool) -> Self {
        let mut details = HashMap::new();
        details.insert("enabled".to_string(), enabled.to_string());
        Self {
            name: name.to_string(),
            healthy: enabled,
            optional: true,
            details,
        }
    }

    pub fn with_detail(mut self, key: &str, value: impl ToString) -> Self {
        self.details.insert(key.to_string(), value.to_string());
        self
    }

    pub fn optional_(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }
}

/// 系统健康检查结果
#[derive(Debug)]
pub struct HealthReport {
    pub overall_healthy: bool,
    pub components: Vec<HealthStatus>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl HealthReport {
    pub fn new() -> Self {
        Self {
            overall_healthy: true,
            components: Vec::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn add_component(&mut self, status: HealthStatus) {
        // 可选组件不影响整体健康状态
        if !status.healthy && !status.optional {
            self.overall_healthy = false;
        }
        self.components.push(status);
    }

    pub fn print(&self) {
        println!("\n╔══════════════════════════════════════════════════════════════╗");
        println!("║                    SYSTEM HEALTH REPORT                       ║");
        println!("╚══════════════════════════════════════════════════════════════╝");

        for comp in &self.components {
            let status = if comp.healthy {
                if comp.optional {
                    "🟡"
                } else {
                    "✅"
                }
            } else {
                "❌"
            };
            let state = if comp.healthy {
                if comp.optional {
                    "OPTIONAL"
                } else {
                    "OK"
                }
            } else {
                "FAIL"
            };
            println!("\n{} {} - {}", status, comp.name, state);
            for (key, value) in &comp.details {
                println!("   ├─ {}: {}", key, value);
            }
        }

        let overall = if self.overall_healthy {
            "✅ ALL SYSTEMS HEALTHY"
        } else {
            "❌ SOME SYSTEMS FAILED"
        };
        println!("\n══════════════════════════════════════════════════════════════════");
        println!("Overall: {}", overall);
        println!(
            "Timestamp: {}\n",
            self.timestamp.format("%Y-%m-%d %H:%M:%S")
        );
    }
}

impl Default for HealthReport {
    fn default() -> Self {
        Self::new()
    }
}

/// 简化的性能分析器
pub struct Profiler {
    timings: HashMap<String, Vec<std::time::Duration>>,
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            timings: HashMap::new(),
        }
    }

    pub fn record(&mut self, name: &str, duration: std::time::Duration) {
        self.timings
            .entry(name.to_string())
            .or_default()
            .push(duration);
    }

    pub fn report(&self) {
        println!("\n📊 PERFORMANCE PROFILE");
        println!("═════════════════════════════════════");

        let mut names: Vec<_> = self.timings.keys().collect();
        names.sort();

        for name in names {
            let times = &self.timings[name];
            if times.is_empty() {
                continue;
            }

            let total: std::time::Duration = times.iter().sum();
            let avg = total / times.len() as u32;
            let min = *times.iter().min().unwrap();
            let max = *times.iter().max().unwrap();

            println!("\n{}:", name);
            println!("  ├─ Calls: {}", times.len());
            println!("  ├─ Total: {:.3}ms", total.as_secs_f64() * 1000.0);
            println!("  ├─ Avg:   {:.3}ms", avg.as_secs_f64() * 1000.0);
            println!("  ├─ Min:   {:.3}ms", min.as_secs_f64() * 1000.0);
            println!("  └─ Max:   {:.3}ms", max.as_secs_f64() * 1000.0);
        }
        println!();
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

/// 锁竞争检测器（用于诊断死锁问题）
pub struct LockDetector {
    locks_held: std::sync::Mutex<Vec<String>>,
}

impl LockDetector {
    pub fn new() -> Self {
        Self {
            locks_held: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn record_lock(&self, lock_name: &str) {
        let mut locks = self.locks_held.lock().unwrap();
        locks.push(format!("{} @ {}:{}", lock_name, file!(), line!()));
    }

    pub fn release_lock(&self, lock_name: &str) {
        let mut locks = self.locks_held.lock().unwrap();
        locks.retain(|l| !l.starts_with(lock_name));
    }

    pub fn get_held_locks(&self) -> Vec<String> {
        self.locks_held.lock().unwrap().clone()
    }
}

impl Default for LockDetector {
    fn default() -> Self {
        Self::new()
    }
}
