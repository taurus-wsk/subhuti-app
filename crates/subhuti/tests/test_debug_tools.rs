//! 调试工具演示和测试
//!
//! 运行: cargo test --test test_debug_tools

use subhuti::*;

#[test]
fn test_diagnose_macro() {
    let x = 42;
    let result = diagnose!(x);
    assert_eq!(*result, 42);

    let name = "Alice";
    diagnose!(name);
    assert_eq!(name, "Alice");
}

#[test]
fn test_assert_that_macro() {
    assert_that!(true, "This should pass");
    assert_that!(1 + 1 == 2);

    // 测试会失败的情况（注释掉以避免测试失败）
    // assert_that!(false, "This will panic");
}

#[test]
fn test_time_it_macro() {
    let result = time_it!("Quick operation" => {
        std::thread::sleep(std::time::Duration::from_millis(10));
        42
    });

    assert_eq!(result, 42);
}

#[test]
fn test_debug_struct_macro() {
    let user = User {
        id: 1,
        name: "Alice".to_string(),
        active: true,
    };

    debug_struct!(user, user);
    assert!(true); // 如果能执行到这里就说明没panic
}

#[test]
fn test_test_tracker() {
    let mut tracker = TestTracker::new();

    tracker.pass("test1");
    tracker.pass("test2");
    tracker.fail("test3", "Assertion failed");
    tracker.fail("test4", "Timeout");

    let summary = tracker.summary();
    assert!(summary.contains("2/4 tests passed"));
    assert!(summary.contains("test3: Assertion failed"));
    assert!(summary.contains("test4: Timeout"));
}

#[test]
fn test_health_status() {
    let healthy = HealthStatus::healthy("ComponentA")
        .with_detail("version", "1.0")
        .with_detail("uptime", "24h");

    assert!(healthy.healthy);
    assert_eq!(healthy.details.get("version").unwrap(), "1.0");

    let unhealthy = HealthStatus::unhealthy("ComponentB", "Connection failed");
    assert!(!unhealthy.healthy);
    assert_eq!(
        unhealthy.details.get("reason").unwrap(),
        "Connection failed"
    );
}

#[test]
fn test_health_report() {
    let mut report = HealthReport::new();

    report.add_component(HealthStatus::healthy("Component1").with_detail("count", 10));
    report.add_component(HealthStatus::healthy("Component2").with_detail("status", "ready"));
    report.add_component(HealthStatus::unhealthy("Component3", "Error"));

    assert!(!report.overall_healthy);
    assert_eq!(report.components.len(), 3);

    // 测试打印功能（输出到 stderr）
    report.print();
    assert!(true); // 如果能执行到这里就说明没panic
}

#[test]
fn test_subhuti_health_check() {
    let subhuti = Subhuti::new();
    let report = subhuti.health_check();

    assert!(report.components.len() >= 5); // 至少5个组件

    // 打印健康报告
    subhuti.print_health_report();
    assert!(true);
}

#[test]
fn test_palace_diagnose() {
    use subhuti::memory::MemoryLayer;
    use subhuti::MemoryPalace;

    let palace = MemoryPalace::new();

    // 存储一些测试记忆
    let id1 = palace
        .store("测试记忆1：编程".to_string(), MemoryLayer::Archive, None)
        .unwrap();
    let id2 = palace
        .store("测试记忆2：情感".to_string(), MemoryLayer::Archive, None)
        .unwrap();

    // 使用 diagnose 调试
    diagnose!(id1);
    diagnose!(id2);

    let stats = palace.stats();
    diagnose!(stats);

    // 搜索测试
    let results = palace.search("编程", 5, None);
    let count = results.len();
    diagnose!(count);

    assert!(!results.is_empty());
}

// 辅助结构
#[derive(Debug)]
#[allow(dead_code)]
struct User {
    id: u32,
    name: String,
    active: bool,
}

fn main() {
    println!("Running debug tools tests...");
}
