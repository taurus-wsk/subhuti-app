//! Subhuti 框架完整集成测试
//!
//! 使用调试工具测试系统各组件的整体表现
//!
//! 运行: cargo test -p subhuti --test integration_test -- --nocapture

use std::collections::HashMap;
use std::time::Instant;
use subhuti::debug::{debug_print, diagnose_value, measure_time};
use subhuti::memory::MemoryLayer;
use subhuti::{MemoryPalace, MemoryZone, Subhuti, TestTracker};

fn main() {
    run_integration_test();
}

#[test]
fn test_full_integration() {
    run_integration_test();
}

fn run_integration_test() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           SUBHUTI FRAMEWORK INTEGRATION TEST                  ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let mut tracker = TestTracker::new();
    let total_start = Instant::now();

    // ── Test 1: 基础初始化 ──────────────────────────────
    print_step(1, "基础初始化测试");
    let t1 = Instant::now();
    match test_basic_initialization() {
        Ok(_) => {
            tracker.pass("基础初始化");
            println!(
                "  ✅ 初始化成功 ({:.3}ms)",
                t1.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("基础初始化", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 2: 健康检查 ────────────────────────────────
    print_step(2, "健康检查测试");
    let t2 = Instant::now();
    match test_health_check() {
        Ok(components) => {
            tracker.pass("健康检查");
            println!(
                "  ✅ {} 个组件健康 ({:.3}ms)",
                components,
                t2.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("健康检查", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 3: 心灵宫殿 - 存储 ─────────────────────────
    print_step(3, "心灵宫殿 - 记忆存储测试");
    let t3 = Instant::now();
    match test_palace_store() {
        Ok(count) => {
            tracker.pass("心灵宫殿存储");
            println!(
                "  ✅ 存储了 {} 条记忆 ({:.3}ms)",
                count,
                t3.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("心灵宫殿存储", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 4: 心灵宫殿 - 搜索 ─────────────────────────
    print_step(4, "心灵宫殿 - 记忆搜索测试");
    let t4 = Instant::now();
    match test_palace_search() {
        Ok((count, persona_time)) => {
            tracker.pass("心灵宫殿搜索");
            println!(
                "  ✅ 搜索到 {} 条结果 (人格加权: {:.3}ms)",
                count,
                persona_time.as_secs_f64() * 1000.0
            );
            println!("     耗时: {:.3}ms", t4.elapsed().as_secs_f64() * 1000.0);
        }
        Err(e) => {
            tracker.fail("心灵宫殿搜索", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 5: 心灵宫殿 - 遗忘机制 ─────────────────────
    print_step(5, "心灵宫殿 - 遗忘机制测试");
    let t5 = Instant::now();
    match test_palace_forget() {
        Ok(forgotten) => {
            tracker.pass("心灵宫殿遗忘");
            println!(
                "  ✅ 遗忘了 {} 条弱记忆 ({:.3}ms)",
                forgotten,
                t5.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("心灵宫殿遗忘", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 6: 心灵宫殿 - 分区统计 ─────────────────────
    print_step(6, "心灵宫殿 - 分区统计测试");
    let t6 = Instant::now();
    match test_palace_zones() {
        Ok(zones) => {
            tracker.pass("心灵宫殿分区");
            println!(
                "  ✅ {} 个分区有记忆 ({:.3}ms)",
                zones.len(),
                t6.elapsed().as_secs_f64() * 1000.0
            );
            for (zone, count) in &zones {
                println!("     ├─ {:?}: {} 条", zone, count);
            }
        }
        Err(e) => {
            tracker.fail("心灵宫殿分区", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 7: 心灵层 - 人格获取 ───────────────────────
    print_step(7, "心灵层 - 人格系统测试");
    let t7 = Instant::now();
    match test_soul_persona() {
        Ok((version, interactions)) => {
            tracker.pass("心灵层人格");
            println!(
                "  ✅ 人格版本 v{}, 互动次数 {} ({:.3}ms)",
                version,
                interactions,
                t7.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("心灵层人格", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 8: 心灵层 - 分区偏好 ───────────────────────
    print_step(8, "心灵层 - 人格分区偏好测试");
    let t8 = Instant::now();
    match test_persona_zone_bias() {
        Ok(biases) => {
            tracker.pass("人格分区偏好");
            println!(
                "  ✅ {} 个分区偏好 ({:.3}ms)",
                biases.len(),
                t8.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("人格分区偏好", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 9: Skills 系统 ─────────────────────────────
    print_step(9, "Skills 系统测试");
    let t9 = Instant::now();
    match test_skills_system() {
        Ok(count) => {
            tracker.pass("Skills系统");
            println!(
                "  ✅ 注册了 {} 个技能 ({:.3}ms)",
                count,
                t9.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("Skills系统", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── Test 10: 专家插件 ───────────────────────────────
    print_step(10, "专家插件系统测试");
    let t10 = Instant::now();
    match test_expert_plugins() {
        Ok((count, active)) => {
            tracker.pass("专家插件");
            println!(
                "  ✅ {} 个插件, 活跃: {} ({:.3}ms)",
                count,
                active,
                t10.elapsed().as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            tracker.fail("专家插件", &e.to_string());
            println!("  ❌ 失败: {}", e);
        }
    }

    // ── 最终健康检查 ────────────────────────────────────
    println!("\n═══ 最终系统状态 ═══");
    let subhuti = Subhuti::new();
    subhuti.print_health_report();

    // ── 测试总结 ────────────────────────────────────────
    println!("\n══════════════════════════════════════════════════════════════");
    println!("{}", tracker.summary());
    println!(
        "总耗时: {:.3}ms",
        total_start.elapsed().as_secs_f64() * 1000.0
    );
    println!("══════════════════════════════════════════════════════════════\n");
}

fn print_step(num: usize, name: &str) {
    println!("\n── Test {}: {} ──", num, name);
}

// ─── 测试函数 ───────────────────────────────────────────

fn test_basic_initialization() -> Result<(), String> {
    let _subhuti = Subhuti::new();
    Ok(())
}

fn test_health_check() -> Result<usize, String> {
    let subhuti = Subhuti::new();
    let report = subhuti.health_check();

    diagnose_value("overall_healthy", &report.overall_healthy);
    diagnose_value("component_count", &report.components.len());

    if !report.overall_healthy {
        return Err(format!(
            "系统不健康: {} 个组件失败",
            report.components.iter().filter(|c| !c.healthy).count()
        ));
    }

    Ok(report.components.len())
}

fn test_palace_store() -> Result<usize, String> {
    let palace = MemoryPalace::new();

    let test_memories = vec![
        ("今天天气真好，适合出去散步", MemoryLayer::ShortTerm),
        (
            "Rust 的所有权系统非常独特，能有效防止内存安全问题",
            MemoryLayer::Archive,
        ),
        (
            "用户说他最近感到很焦虑，需要一些心理支持",
            MemoryLayer::Archive,
        ),
        ("任务进度：完成了 80% 的功能开发", MemoryLayer::ShortTerm),
        (
            "创意想法：可以做一个基于心灵宫殿的知识管理系统",
            MemoryLayer::Archive,
        ),
        ("用户的生日是 1990 年 5 月 20 日", MemoryLayer::Archive),
        (
            "专业知识：认知行为疗法 CBT 是一种有效的心理治疗方法",
            MemoryLayer::Knowledge,
        ),
        ("今天和朋友吃了一顿美味的火锅", MemoryLayer::ShortTerm),
        ("任务：明天需要完成项目文档", MemoryLayer::ShortTerm),
        (
            "专业知识：SQL 中的 JOIN 操作可以连接多个表",
            MemoryLayer::Knowledge,
        ),
    ];

    for (content, layer) in &test_memories {
        measure_time("Store memory", || {
            palace
                .store(content.to_string(), *layer, None)
                .map_err(|e| format!("存储记忆失败: {}", e))
        })
        .unwrap();
    }

    let stats = palace.stats();
    diagnose_value("total_memories", &stats.total_count);

    Ok(test_memories.len())
}

fn test_palace_search() -> Result<(usize, std::time::Duration), String> {
    let palace = setup_test_palace();

    // 普通搜索
    let results = palace.search("知识", 10, None);
    diagnose_value("search_results", &results.len());

    if results.is_empty() {
        return Err("搜索结果为空".to_string());
    }

    // 带人格偏好的搜索
    let mut persona_bias = HashMap::new();
    persona_bias.insert(MemoryZone::ExpertKnowledge, 1.2);
    persona_bias.insert(MemoryZone::Emotional, 0.8);

    let start = Instant::now();
    let persona_results = palace.search("知识", 5, Some(&persona_bias));
    let persona_time = start.elapsed();

    diagnose_value("persona_search_count", &persona_results.len());

    Ok((results.len(), persona_time))
}

fn test_palace_forget() -> Result<usize, String> {
    let palace = setup_test_palace();

    let before = palace.stats().total_count;
    diagnose_value("before_forget", &before);

    // 模拟时间流逝（通过多次搜索来改变记忆强度）
    for _ in 0..5 {
        palace.search("天气", 5, None);
        palace.search("知识", 5, None);
    }

    let forgotten = palace.run_forget_cycle();

    let after = palace.stats().total_count;
    diagnose_value("after_forget", &after);
    diagnose_value("forgotten_count", &forgotten);

    Ok(forgotten)
}

fn test_palace_zones() -> Result<HashMap<MemoryZone, usize>, String> {
    let palace = setup_test_palace();

    let stats = palace.stats();
    let mut zones = HashMap::new();

    for (zone, count) in &stats.zone_counts {
        if *count > 0 {
            zones.insert(*zone, *count);
        }
    }

    debug_print("zone_stats", &zones);

    Ok(zones)
}

fn test_soul_persona() -> Result<(u32, u32), String> {
    let subhuti = Subhuti::new();

    let version = subhuti.persona_version();
    let interactions = subhuti.total_interactions();

    diagnose_value("persona_version", &version);
    diagnose_value("total_interactions", &interactions);

    Ok((version, interactions))
}

fn test_persona_zone_bias() -> Result<HashMap<MemoryZone, f32>, String> {
    let subhuti = Subhuti::new();
    let bias = subhuti.persona_zone_bias();

    debug_print("zone_bias", &bias);

    if bias.is_empty() {
        return Err("人格分区偏好为空".to_string());
    }

    Ok(bias)
}

fn test_skills_system() -> Result<usize, String> {
    let subhuti = Subhuti::new();

    let count = subhuti.skill_count();
    diagnose_value("skill_count", &count);

    if count == 0 {
        return Err("没有注册任何技能".to_string());
    }

    Ok(count)
}

fn test_expert_plugins() -> Result<(usize, String), String> {
    let subhuti = Subhuti::new();

    let count = subhuti.expert_plugin_count();
    let active = subhuti
        .active_expert_id()
        .unwrap_or_else(|| "none".to_string());

    diagnose_value("plugin_count", &count);
    diagnose_value("active_expert", &active);

    Ok((count, active))
}

fn setup_test_palace() -> MemoryPalace {
    let palace = MemoryPalace::new();

    let memories = vec![
        ("今天天气真好，适合出去散步", MemoryLayer::ShortTerm),
        ("Rust 的所有权系统非常独特", MemoryLayer::Archive),
        ("用户说他最近感到很焦虑", MemoryLayer::Archive),
        ("任务进度：完成了 80%", MemoryLayer::ShortTerm),
        ("创意想法：心灵宫殿知识管理", MemoryLayer::Archive),
        ("CBT 认知行为疗法", MemoryLayer::Knowledge),
        ("SQL JOIN 操作", MemoryLayer::Knowledge),
        ("和朋友吃了火锅", MemoryLayer::ShortTerm),
        ("明天需要完成文档", MemoryLayer::ShortTerm),
        ("心理学知识：情绪调节技巧", MemoryLayer::Knowledge),
    ];

    for (content, layer) in memories {
        let _ = palace.store(content.to_string(), layer, None);
    }

    palace
}
