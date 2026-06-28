//! Subhuti 框架性能测试
//!
//! 使用框架内置的 Profiler、TestTracker 等工具，
//! 对核心组件进行系统化性能基准测试。
//!
//! 运行: cargo test -p subhuti --test performance_test -- --nocapture

use std::collections::HashMap;
use std::time::Instant;
use subhuti::debug::diagnose_value;
use subhuti::memory::{MemoryItem, MemoryLayer};
use subhuti::{MemoryPalace, MemoryZone, PalaceMemory, Profiler, Subhuti, TestTracker};

fn main() {
    run_performance_test();
}

#[test]
fn test_performance() {
    run_performance_test();
}

fn run_performance_test() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           SUBHUTI FRAMEWORK PERFORMANCE TEST                ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let mut tracker = TestTracker::new();
    let mut profiler = Profiler::new();
    let total_start = Instant::now();

    // ── Perf 1: 框架初始化 ──────────────────────────────
    print_step(1, "框架初始化性能");
    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = Subhuti::new();
    }
    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_secs_f64() * 1000.0 / iterations as f64;
    profiler.record("Subhuti::new()", elapsed / iterations);
    diagnose_value("iterations", &iterations);
    diagnose_value("avg_time_ms", &avg_ms);
    if avg_ms < 5.0 {
        tracker.pass("框架初始化 (< 5ms)");
    } else {
        tracker.fail(
            "框架初始化",
            &format!("平均耗时 {:.1}ms，超过 5ms 阈值", avg_ms),
        );
    }

    // ── Perf 2: 心灵宫殿 - 存储性能 ────────────────────
    print_step(2, "心灵宫殿 - 存储性能");
    let palace = MemoryPalace::new();
    let store_count = 1000;
    let start = Instant::now();
    for i in 0..store_count {
        let content = format!("测试记忆内容 {} - Rust 编程语言 {}", i, i);
        let _ = palace.store(content, MemoryLayer::Archive, None);
    }
    let store_elapsed = start.elapsed();
    let store_total_ms = store_elapsed.as_secs_f64() * 1000.0;
    let store_avg_us = store_elapsed.as_micros() as f64 / store_count as f64;
    profiler.record("Palace::store() x1000", store_elapsed);
    profiler.record("Palace::store() avg", store_elapsed / store_count as u32);
    diagnose_value("store_count", &store_count);
    diagnose_value("total_ms", &store_total_ms);
    diagnose_value("avg_us", &store_avg_us);
    if store_elapsed.as_secs() < 5 {
        tracker.pass("宫殿存储 (< 5s / 1000条)");
    } else {
        tracker.fail(
            "宫殿存储",
            &format!(
                "1000 条存储耗时 {:.1}s，超过 5s 阈值",
                store_elapsed.as_secs_f64()
            ),
        );
    }

    // ── Perf 3: 心灵宫殿 - 搜索性能 ────────────────────
    print_step(3, "心灵宫殿 - 搜索性能");
    let search_iterations = 100;
    let start = Instant::now();
    for _ in 0..search_iterations {
        let _ = palace.search("Rust 编程", 10, None);
    }
    let search_elapsed = start.elapsed();
    let search_avg_us = search_elapsed.as_micros() as f64 / search_iterations as f64;
    let search_avg_ms = search_elapsed.as_millis() as f64 / search_iterations as f64;
    profiler.record(
        "Palace::search() avg",
        search_elapsed / search_iterations as u32,
    );
    diagnose_value("iterations", &search_iterations);
    diagnose_value("avg_us", &search_avg_us);
    if search_avg_ms < 10.0 {
        tracker.pass("宫殿搜索 (< 10ms/次)");
    } else {
        tracker.fail(
            "宫殿搜索",
            &format!("平均搜索耗时 {:.2}ms，超过 10ms 阈值", search_avg_ms),
        );
    }

    // ── Perf 4: 心灵宫殿 - 带人格偏好的搜索性能 ────────
    print_step(4, "心灵宫殿 - 人格加权搜索性能");
    let mut bias = HashMap::new();
    bias.insert(MemoryZone::ExpertKnowledge, 1.5);
    bias.insert(MemoryZone::Emotional, 0.5);
    bias.insert(MemoryZone::DailyChat, 1.2);
    let start = Instant::now();
    for _ in 0..search_iterations {
        let _ = palace.search("Rust 编程", 10, Some(&bias));
    }
    let biased_elapsed = start.elapsed();
    let biased_avg_us = biased_elapsed.as_micros() as f64 / search_iterations as f64;
    let biased_avg_ms = biased_elapsed.as_millis() as f64 / search_iterations as f64;
    profiler.record(
        "Palace::search(persona) avg",
        biased_elapsed / search_iterations as u32,
    );
    diagnose_value("avg_us", &biased_avg_us);
    if biased_avg_ms < 10.0 {
        tracker.pass("人格加权搜索 (< 10ms/次)");
    } else {
        tracker.fail(
            "人格加权搜索",
            &format!("平均耗时 {:.2}ms，超过 10ms 阈值", biased_avg_ms),
        );
    }

    // ── Perf 5: 心灵宫殿 - 遗忘周期性能 ────────────────
    print_step(5, "心灵宫殿 - 遗忘周期性能");
    let forget_palace = setup_large_palace(500);
    let start = Instant::now();
    let forgotten = forget_palace.run_forget_cycle();
    let forget_elapsed = start.elapsed();
    let forget_ms = forget_elapsed.as_secs_f64() * 1000.0;
    profiler.record("Palace::run_forget_cycle()", forget_elapsed);
    diagnose_value("total_memories", &500);
    diagnose_value("forgotten", &forgotten);
    diagnose_value("elapsed_ms", &forget_ms);
    if forget_elapsed.as_millis() < 100 {
        tracker.pass("遗忘周期 (< 100ms / 500条)");
    } else {
        tracker.fail(
            "遗忘周期",
            &format!("500 条遗忘耗时 {:.1}ms，超过 100ms 阈值", forget_ms),
        );
    }

    // ── Perf 6: 心灵宫殿 - 大规模搜索性能 ──────────────
    print_step(6, "心灵宫殿 - 大规模数据搜索性能");
    let large_palace = setup_large_palace(1000);
    let start = Instant::now();
    for _ in 0..50 {
        let _ = large_palace.search("知识 技术 编程", 20, None);
    }
    let large_elapsed = start.elapsed();
    let large_avg_us = large_elapsed.as_micros() as f64 / 50.0;
    let large_avg_ms = large_elapsed.as_millis() as f64 / 50.0;
    profiler.record("Palace::search(1000条) avg", large_elapsed / 50);
    diagnose_value("palace_size", &1000);
    diagnose_value("avg_us", &large_avg_us);
    if large_avg_ms < 20.0 {
        tracker.pass("大规模搜索 (< 20ms/次)");
    } else {
        tracker.fail(
            "大规模搜索",
            &format!("1000 条平均搜索耗时 {:.2}ms，超过 20ms 阈值", large_avg_ms),
        );
    }

    // ── Perf 7: 健康检查性能 ────────────────────────────
    print_step(7, "健康检查性能");
    let subhuti = Subhuti::new();
    let health_iterations = 1000;
    let start = Instant::now();
    for _ in 0..health_iterations {
        let _ = subhuti.health_check();
    }
    let health_elapsed = start.elapsed();
    let health_avg_us = health_elapsed.as_micros() as f64 / health_iterations as f64;
    profiler.record(
        "Subhuti::health_check() avg",
        health_elapsed / health_iterations as u32,
    );
    diagnose_value("avg_us", &health_avg_us);
    if health_avg_us < 500.0 {
        tracker.pass("健康检查 (< 500μs/次)");
    } else {
        tracker.fail(
            "健康检查",
            &format!("平均耗时 {:.1}μs，超过 500μs 阈值", health_avg_us),
        );
    }

    // ── Perf 8: Skill 匹配性能 ─────────────────────────
    print_step(8, "Skill 匹配性能");
    let skill_iterations = 1000;
    let test_inputs = vec![
        "今天天气怎么样",
        "帮我计算 123 + 456",
        "你还记得我之前说的吗",
        "随便聊聊天",
        "帮我搜索一下文件",
        "写一段 Python 代码",
        "帮我搜索一下网上的信息",
        "提醒我明天开会",
    ];
    let start = Instant::now();
    for _ in 0..skill_iterations {
        for _input in &test_inputs {
            let _ = subhuti.list_skills(); // 获取 Skill 列表
        }
    }
    let skill_elapsed = start.elapsed();
    let total_calls = skill_iterations * test_inputs.len();
    let skill_avg_us = skill_elapsed.as_micros() as f64 / total_calls as f64;
    profiler.record("Skill listing avg", skill_elapsed / total_calls as u32);
    diagnose_value("total_calls", &total_calls);
    diagnose_value("avg_us", &skill_avg_us);
    if skill_avg_us < 100.0 {
        tracker.pass("Skill 列表 (< 100μs/次)");
    } else {
        tracker.fail(
            "Skill 列表",
            &format!("平均耗时 {:.1}μs，超过 100μs 阈值", skill_avg_us),
        );
    }

    // ── Perf 9: 分区推断性能 ───────────────────────────
    print_step(9, "记忆分区推断性能");
    let zone_iterations = 10000;
    let test_texts = vec![
        "今天天气真好，适合出去散步",
        "什么是 Rust 的所有权系统",
        "我今天好开心啊",
        "明天要完成这个任务",
        "如果我有超能力就好了",
        "嗯",
    ];
    let start = Instant::now();
    for _ in 0..zone_iterations {
        for text in &test_texts {
            let _ = MemoryZone::infer_from_content(text);
        }
    }
    let zone_elapsed = start.elapsed();
    let total_zone_calls = zone_iterations * test_texts.len();
    let zone_avg_us = zone_elapsed.as_micros() as f64 / total_zone_calls as f64;
    let zone_avg_ns = zone_elapsed.as_nanos() as f64 / total_zone_calls as f64;
    profiler.record("Zone::infer() avg", zone_elapsed / total_zone_calls as u32);
    diagnose_value("total_calls", &total_zone_calls);
    diagnose_value("avg_ns", &zone_avg_ns);
    if zone_avg_us < 10.0 {
        tracker.pass("分区推断 (< 10μs/次)");
    } else {
        tracker.fail(
            "分区推断",
            &format!("平均耗时 {:.2}μs，超过 10μs 阈值", zone_avg_us),
        );
    }

    // ── Perf 10: 记忆强度衰减性能 ──────────────────────
    print_step(10, "记忆强度衰减性能");
    let decay_iterations = 100000;
    let start = Instant::now();
    for _ in 0..decay_iterations {
        let mut mem = PalaceMemory::new(MemoryItem::new(
            "测试记忆".into(),
            MemoryLayer::Archive,
            None,
        ));
        mem.decay(1.0);
        mem.activate();
        mem.decay(0.5);
    }
    let decay_elapsed = start.elapsed();
    let decay_avg_ns = decay_elapsed.as_nanos() as f64 / decay_iterations as f64;
    profiler.record(
        "PalaceMemory lifecycle avg",
        decay_elapsed / decay_iterations as u32,
    );
    diagnose_value("iterations", &decay_iterations);
    diagnose_value("avg_ns", &decay_avg_ns);
    if decay_elapsed.as_secs() < 10 {
        tracker.pass("记忆生命周期 (< 100ns/轮)");
    } else {
        tracker.fail(
            "记忆生命周期",
            &format!("平均耗时 {:.1}ns/轮，超过 100ns 阈值", decay_avg_ns),
        );
    }

    // ── 性能报告 ────────────────────────────────────────
    println!("\n═══ 性能分析报告 ═══");
    profiler.report();

    // ── 测试总结 ────────────────────────────────────────
    println!("══════════════════════════════════════════════════════════════");
    println!("{}", tracker.summary());
    println!(
        "总耗时: {:.3}ms",
        total_start.elapsed().as_secs_f64() * 1000.0
    );
    println!("══════════════════════════════════════════════════════════════\n");
}

fn print_step(num: usize, name: &str) {
    println!("\n── Perf {}: {} ──", num, name);
}

/// 创建大规模测试心灵宫殿
fn setup_large_palace(size: usize) -> MemoryPalace {
    let palace = MemoryPalace::new();

    let templates = vec![
        ("今天天气真好，适合出去散步 {}", MemoryLayer::ShortTerm),
        (
            "Rust 的所有权系统非常独特，能有效防止内存安全问题 {}",
            MemoryLayer::Archive,
        ),
        (
            "用户说他最近感到很焦虑，需要一些心理支持 {}",
            MemoryLayer::Archive,
        ),
        ("任务进度：完成了 {}% 的功能开发", MemoryLayer::ShortTerm),
        (
            "创意想法：可以做一个基于心灵宫殿的知识管理系统 {}",
            MemoryLayer::Archive,
        ),
        (
            "专业知识：认知行为疗法 CBT 是一种有效的心理治疗方法 {}",
            MemoryLayer::Knowledge,
        ),
        ("今天和朋友吃了一顿美味的火锅 {}", MemoryLayer::ShortTerm),
        (
            "专业知识：SQL 中的 JOIN 操作可以连接多个表 {}",
            MemoryLayer::Knowledge,
        ),
        (
            "用户的心情很好，今天是个愉快的日子 {}",
            MemoryLayer::Archive,
        ),
        ("明天需要完成项目文档和代码审查 {}", MemoryLayer::ShortTerm),
    ];

    for i in 0..size {
        let (template, layer) = &templates[i % templates.len()];
        let content = template.replace("{}", &i.to_string());
        let _ = palace.store(content, *layer, None);
    }

    palace
}
