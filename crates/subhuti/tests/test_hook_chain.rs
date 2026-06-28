//! 专家插件 Hook 链测试
//!
//! 测试钩子执行顺序、链式中断、输入/响应修改、权限检查、
//! 多插件共享钩子、完整生命周期等场景
//!
//! 运行: cargo test -p subhuti --test test_hook_chain -- --nocapture

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use subhuti::expert::{
    Author, ExpertPlugin, HookContext, HookPoint, HookRegistry, HookResult, PluginCategory,
    PluginManager, PluginManifest, PluginPermissions, PluginState, SandboxConfig,
};
use subhuti::TestTracker;

fn main() {
    run_tests();
}

#[test]
fn test_hook_chain() {
    run_tests();
}

fn run_tests() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           EXPERT HOOK CHAIN TEST - 钩子链验证                  ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let mut tracker = TestTracker::new();
    let total_start = std::time::Instant::now();

    // ── Test 1: Hook 执行顺序 ─────────────────────────────
    print_step(1, "Hook 执行顺序测试");
    match test_hook_execution_order() {
        Ok(msg) => {
            tracker.pass("执行顺序");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("执行顺序", &e),
    }

    // ── Test 2: Hook 链中断 ───────────────────────────────
    print_step(2, "Hook 链中断测试");
    match test_hook_chain_block() {
        Ok(msg) => {
            tracker.pass("链中断");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("链中断", &e),
    }

    // ── Test 3: Hook 输入修改 ─────────────────────────────
    print_step(3, "Hook 输入修改传递测试");
    match test_hook_input_modification() {
        Ok(msg) => {
            tracker.pass("输入修改");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("输入修改", &e),
    }

    // ── Test 4: Hook 响应修改 ─────────────────────────────
    print_step(4, "Hook 响应修改传递测试");
    match test_hook_response_modification() {
        Ok(msg) => {
            tracker.pass("响应修改");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("响应修改", &e),
    }

    // ── Test 5: 多插件共享钩子点 ──────────────────────────
    print_step(5, "多插件共享钩子点测试");
    match test_multiple_plugins_shared_hook() {
        Ok(msg) => {
            tracker.pass("多插件共享");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("多插件共享", &e),
    }

    // ── Test 6: 权限检查 ──────────────────────────────────
    print_step(6, "插件权限检查测试");
    match test_permission_checks() {
        Ok(msg) => {
            tracker.pass("权限检查");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("权限检查", &e),
    }

    // ── Test 7: 完整生命周期 + Hook ───────────────────────
    print_step(7, "完整生命周期 + Hook 测试");
    match test_full_lifecycle_with_hooks() {
        Ok(msg) => {
            tracker.pass("完整生命周期");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("完整生命周期", &e),
    }

    // ── Test 8: Hook 上下文传递 ───────────────────────────
    print_step(8, "Hook 上下文传递测试");
    match test_hook_context_passing() {
        Ok(msg) => {
            tracker.pass("上下文传递");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("上下文传递", &e),
    }

    // ── Test 9: 沙箱速率限制 ──────────────────────────────
    print_step(9, "沙箱速率限制测试");
    match test_sandbox_rate_limit_with_hooks() {
        Ok(msg) => {
            tracker.pass("速率限制");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("速率限制", &e),
    }

    // ── Test 10: 未注册钩子点默认行为 ─────────────────────
    print_step(10, "未注册钩子点默认行为测试");
    match test_unregistered_hook_default() {
        Ok(msg) => {
            tracker.pass("默认行为");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("默认行为", &e),
    }

    // ── 测试总结 ──────────────────────────────────────────
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

// ════════════════════════════════════════════════════════════
// 辅助测试插件
// ════════════════════════════════════════════════════════════

/// 记录型插件 - 记录被调用的钩子
struct RecordingPlugin {
    id: String,
    name: String,
    keywords: Vec<String>,
    hooks: Vec<HookPoint>,
    permissions: PluginPermissions,
    call_log: Arc<AtomicUsize>,
}

impl RecordingPlugin {
    fn new(id: &str, name: &str, hooks: Vec<HookPoint>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            keywords: vec![],
            hooks,
            permissions: PluginPermissions::default(),
            call_log: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn with_permissions(mut self, perms: PluginPermissions) -> Self {
        self.permissions = perms;
        self
    }

    fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    fn get_call_count(&self) -> usize {
        self.call_log.load(Ordering::SeqCst)
    }
}

impl ExpertPlugin for RecordingPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: self.id.clone(),
            name: self.name.clone(),
            description: format!("{} 测试插件", self.name),
            version: "1.0.0".into(),
            author: Some(Author {
                name: "Test".into(),
                email: None,
                url: None,
            }),
            category: PluginCategory::Development,
            keywords: self.keywords.clone(),
            permissions: self.permissions.clone(),
            hooks: self.hooks.clone(),
            sandbox: SandboxConfig::default(),
            ..Default::default()
        }
    }

    fn handle_hook(&self, point: HookPoint, _ctx: HookContext) -> HookResult {
        self.call_log.fetch_add(1, Ordering::SeqCst);
        println!(
            "  ├─ [{}] hook {:?} called (total: {})",
            self.id,
            point,
            self.get_call_count()
        );
        HookResult::continue_()
    }

    fn matches(&self, input: &str) -> f32 {
        let lower = input.to_lowercase();
        let mut score = 0.0f32;
        for kw in &self.keywords {
            if lower.contains(&kw.to_lowercase()) {
                score += 0.3;
            }
        }
        score.min(1.0)
    }
}

/// 阻断型插件 - 返回 block
struct BlockingPlugin {
    id: String,
    block_at: HookPoint,
    call_log: Arc<AtomicUsize>,
}

impl BlockingPlugin {
    fn new(id: &str, block_at: HookPoint) -> Self {
        Self {
            id: id.to_string(),
            block_at,
            call_log: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ExpertPlugin for BlockingPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: self.id.clone(),
            name: "阻断插件".into(),
            description: "在指定钩子点阻断执行链".into(),
            version: "1.0.0".into(),
            category: PluginCategory::Development,
            keywords: vec![],
            permissions: PluginPermissions::default(),
            hooks: vec![self.block_at],
            sandbox: SandboxConfig::default(),
            ..Default::default()
        }
    }

    fn handle_hook(&self, point: HookPoint, _ctx: HookContext) -> HookResult {
        self.call_log.fetch_add(1, Ordering::SeqCst);
        if point == self.block_at {
            HookResult::block(&format!("{} 阻断了 {}", self.id, point))
        } else {
            HookResult::continue_()
        }
    }
}

/// 修改型插件 - 修改输入或响应
struct ModifyingPlugin {
    id: String,
    hooks: Vec<HookPoint>,
}

impl ModifyingPlugin {
    fn new(id: &str, hooks: Vec<HookPoint>) -> Self {
        Self {
            id: id.to_string(),
            hooks,
        }
    }
}

impl ExpertPlugin for ModifyingPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: self.id.clone(),
            name: format!("修改插件 {}", self.id),
            description: "修改输入或响应".into(),
            version: "1.0.0".into(),
            category: PluginCategory::Development,
            keywords: vec![],
            permissions: PluginPermissions::default(),
            hooks: self.hooks.clone(),
            sandbox: SandboxConfig::default(),
            ..Default::default()
        }
    }

    fn handle_hook(&self, point: HookPoint, ctx: HookContext) -> HookResult {
        match point {
            HookPoint::BeforeRequest => {
                HookResult::modify_input(format!("[{}-modified] {}", self.id, ctx.input))
            }
            HookPoint::AfterResponse => {
                let mut result = HookResult::continue_();
                result.modified_response = Some(format!("[{}-appended]", self.id));
                result
            }
            _ => HookResult::continue_(),
        }
    }
}

// ════════════════════════════════════════════════════════════
// 测试函数
// ════════════════════════════════════════════════════════════

/// Test 1: Hook 执行顺序 - 多个钩子按注册顺序执行
fn test_hook_execution_order() -> Result<String, String> {
    let mut registry = HookRegistry::new();

    let order = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    // 注册 3 个钩子到 BeforeResponse
    for i in 1..=3 {
        let order_clone = order.clone();
        let name = format!("hook_{}", i);
        let n = name.clone();
        registry.register(
            HookPoint::BeforeResponse,
            Box::new(move |_ctx| {
                order_clone.lock().unwrap().push(n.clone());
                HookResult::continue_()
            }),
        );
    }

    let ctx = HookContext::new("user1", "session1", "test");
    let result = registry.execute(&HookPoint::BeforeResponse, ctx);

    assert!(result.should_continue);
    let executed = order.lock().unwrap();
    assert_eq!(*executed, vec!["hook_1", "hook_2", "hook_3"]);

    Ok(format!("3 个钩子按注册顺序执行: {:?}", *executed))
}

/// Test 2: Hook 链中断 - block 后后续钩子不再执行
fn test_hook_chain_block() -> Result<String, String> {
    let mut registry = HookRegistry::new();

    let counter = Arc::new(AtomicUsize::new(0));

    // 第一个钩子：正常
    let c1 = counter.clone();
    registry.register(
        HookPoint::BeforeRequest,
        Box::new(move |_| {
            c1.fetch_add(1, Ordering::SeqCst);
            HookResult::continue_()
        }),
    );

    // 第二个钩子：阻断
    let c2 = counter.clone();
    registry.register(
        HookPoint::BeforeRequest,
        Box::new(move |_| {
            c2.fetch_add(1, Ordering::SeqCst);
            HookResult::block("权限不足，拒绝执行")
        }),
    );

    // 第三个钩子：不应被执行
    let c3 = counter.clone();
    registry.register(
        HookPoint::BeforeRequest,
        Box::new(move |_| {
            c3.fetch_add(1, Ordering::SeqCst);
            HookResult::continue_()
        }),
    );

    let ctx = HookContext::new("user1", "session1", "test");
    let result = registry.execute(&HookPoint::BeforeRequest, ctx);

    // 应该被阻断
    assert!(!result.should_continue);
    assert_eq!(result.error, Some("权限不足，拒绝执行".to_string()));
    // 只执行了前 2 个钩子
    assert_eq!(counter.load(Ordering::SeqCst), 2);

    Ok("第2个钩子阻断了链，第3个钩子未执行".to_string())
}

/// Test 3: Hook 输入修改 - 第一个钩子修改输入，第二个可以看到
fn test_hook_input_modification() -> Result<String, String> {
    // 注意：当前 HookRegistry 实现是传递同一个 ctx.clone()，
    // 不会链式传递 modified_input。这里测试的是合并行为。
    let mut registry = HookRegistry::new();

    // 第一个钩子修改输入
    registry.register(
        HookPoint::BeforeRequest,
        Box::new(move |_ctx| HookResult::modify_input("[modified-by-hook1] 原始输入".to_string())),
    );

    // 第二个钩子也修改输入（会覆盖前一个）
    registry.register(
        HookPoint::BeforeRequest,
        Box::new(move |_ctx| HookResult::modify_input("[modified-by-hook2] 最终输入".to_string())),
    );

    let ctx = HookContext::new("user1", "session1", "原始输入");
    let result = registry.execute(&HookPoint::BeforeRequest, ctx);

    assert!(result.should_continue);
    assert_eq!(
        result.modified_input,
        Some("[modified-by-hook2] 最终输入".to_string())
    );

    Ok("输入被最后一个钩子修改覆盖".to_string())
}

/// Test 4: Hook 响应修改
fn test_hook_response_modification() -> Result<String, String> {
    let mut registry = HookRegistry::new();

    registry.register(
        HookPoint::AfterResponse,
        Box::new(move |_| HookResult::modify_response("修改后的响应 v1".to_string())),
    );

    registry.register(
        HookPoint::AfterResponse,
        Box::new(move |_| HookResult::modify_response("修改后的响应 v2（最终）".to_string())),
    );

    let ctx = HookContext::new("user1", "session1", "test");
    let result = registry.execute(&HookPoint::AfterResponse, ctx);

    assert!(result.should_continue);
    assert_eq!(
        result.modified_response,
        Some("修改后的响应 v2（最终）".to_string())
    );

    Ok("响应修改正确传递，最后一个钩子的修改生效".to_string())
}

/// Test 5: 多插件共享钩子点 - 两个插件同时注册 BeforeResponse
fn test_multiple_plugins_shared_hook() -> Result<String, String> {
    let mut manager = PluginManager::new();

    let plugin_a = RecordingPlugin::new(
        "plugin-a",
        "插件A",
        vec![HookPoint::BeforeResponse, HookPoint::AfterResponse],
    );
    let plugin_b = RecordingPlugin::new(
        "plugin-b",
        "插件B",
        vec![HookPoint::BeforeResponse, HookPoint::BeforeLlmCall],
    );

    manager.install(plugin_a).unwrap();
    manager.install(plugin_b).unwrap();
    manager.enable("plugin-a").unwrap();
    manager.enable("plugin-b").unwrap();

    // 执行 BeforeResponse（两个插件都注册了）
    let ctx = HookContext::new("user1", "session1", "test");
    let result = manager.execute_hook(HookPoint::BeforeResponse, ctx);
    assert!(result.should_continue);

    // 执行 BeforeLlmCall（只有 plugin-b 注册了）
    let ctx2 = HookContext::new("user1", "session1", "test");
    let result2 = manager.execute_hook(HookPoint::BeforeLlmCall, ctx2);
    assert!(result2.should_continue);

    // 执行 BeforeRequest（没有插件注册）
    let ctx3 = HookContext::new("user1", "session1", "test");
    let result3 = manager.execute_hook(HookPoint::BeforeRequest, ctx3);
    assert!(result3.should_continue);

    Ok("两个插件共享 BeforeResponse 钩子点，执行正常".to_string())
}

/// Test 6: 权限检查
fn test_permission_checks() -> Result<String, String> {
    let mut manager = PluginManager::new();

    // 插件A：有限权限
    let plugin_a = RecordingPlugin::new("restricted", "受限插件", vec![HookPoint::BeforeResponse])
        .with_permissions(PluginPermissions {
            file_read: true,
            network: false,
            ..Default::default()
        });

    // 插件B：全部权限
    let plugin_b = RecordingPlugin::new("full-access", "全权插件", vec![HookPoint::AfterResponse])
        .with_permissions(PluginPermissions::allow_all());

    manager.install(plugin_a).unwrap();
    manager.install(plugin_b).unwrap();

    // 检查权限
    assert!(manager.check_permission("restricted", "file_read"));
    assert!(!manager.check_permission("restricted", "network"));
    assert!(!manager.check_permission("restricted", "code_execution"));
    assert!(manager.check_permission("full-access", "network"));
    assert!(manager.check_permission("full-access", "code_execution"));
    assert!(manager.check_permission("full-access", "modify_soul"));

    // 不存在的插件
    assert!(!manager.check_permission("nonexistent", "file_read"));

    Ok("受限/全权/不存在插件的权限检查均正确".to_string())
}

/// Test 7: 完整生命周期 + Hook
fn test_full_lifecycle_with_hooks() -> Result<String, String> {
    let mut manager = PluginManager::new();

    let plugin = RecordingPlugin::new(
        "lifecycle-test",
        "生命周期插件",
        vec![
            HookPoint::BeforeRequest,
            HookPoint::BeforeSkillMatch,
            HookPoint::BeforeLlmCall,
            HookPoint::AfterLlmCall,
            HookPoint::BeforeResponse,
            HookPoint::AfterResponse,
        ],
    )
    .with_keywords(vec!["心理".into(), "咨询".into()]);

    // 1. 安装
    manager.install(plugin).unwrap();
    assert_eq!(
        manager.get_metadata("lifecycle-test").unwrap().state,
        PluginState::Installed
    );

    // 2. 启用（此时注册钩子）
    manager.enable("lifecycle-test").unwrap();
    assert_eq!(
        manager.get_metadata("lifecycle-test").unwrap().state,
        PluginState::Enabled
    );

    // 3. 激活
    let _expert = manager.activate("lifecycle-test").unwrap();
    assert_eq!(
        manager.get_metadata("lifecycle-test").unwrap().state,
        PluginState::Activated
    );
    assert_eq!(
        manager.get_active_expert_id(),
        Some("lifecycle-test".to_string())
    );

    // 4. 模拟完整请求流程的钩子链
    let hooks_to_fire = vec![
        HookPoint::BeforeRequest,
        HookPoint::BeforeSkillMatch,
        HookPoint::BeforeLlmCall,
        HookPoint::AfterLlmCall,
        HookPoint::BeforeResponse,
        HookPoint::AfterResponse,
    ];

    for hook in &hooks_to_fire {
        let ctx = HookContext::new("user1", "session1", "我想做心理咨询");
        let result = manager.execute_hook(*hook, ctx);
        assert!(result.should_continue, "Hook {:?} should continue", hook);
    }

    // 5. 停用
    manager.deactivate().unwrap();
    assert_eq!(
        manager.get_metadata("lifecycle-test").unwrap().state,
        PluginState::Enabled
    );

    // 6. 再次执行钩子（应该仍然生效，因为只是 deactivate 而非 disable）
    let ctx = HookContext::new("user1", "session1", "test");
    let result = manager.execute_hook(HookPoint::BeforeRequest, ctx);
    assert!(result.should_continue);

    // 7. 停用插件
    manager.disable("lifecycle-test").unwrap();
    assert_eq!(
        manager.get_metadata("lifecycle-test").unwrap().state,
        PluginState::Disabled
    );

    // 8. 卸载
    manager.uninstall("lifecycle-test").unwrap();

    Ok(
        "完整生命周期 install→enable→activate→hooks→deactivate→disable→uninstall 全部通过"
            .to_string(),
    )
}

/// Test 8: Hook 上下文传递 - 验证 ctx 中的字段
fn test_hook_context_passing() -> Result<String, String> {
    let mut registry = HookRegistry::new();

    registry.register(
        HookPoint::BeforeRequest,
        Box::new(move |ctx| {
            assert_eq!(ctx.user_id, "user_42");
            assert_eq!(ctx.session_id, "session_abc");
            assert_eq!(ctx.input, "你好世界");
            assert!(ctx.current_expert.is_none());
            assert!(!ctx.request_id.is_empty());
            HookResult::continue_()
        }),
    );

    let ctx = HookContext::new("user_42", "session_abc", "你好世界");
    let result = registry.execute(&HookPoint::BeforeRequest, ctx);

    assert!(result.should_continue);
    Ok("上下文字段 user_id/session_id/input/request_id 全部正确传递".to_string())
}

/// Test 9: 沙箱速率限制 + Hook
fn test_sandbox_rate_limit_with_hooks() -> Result<String, String> {
    let mut manager = PluginManager::new();

    // 创建一个每日限制为 3 次的插件
    struct LimitedPlugin;
    impl ExpertPlugin for LimitedPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "limited".into(),
                name: "受限次数插件".into(),
                description: "测试速率限制".into(),
                version: "1.0.0".into(),
                category: PluginCategory::Development,
                keywords: vec![],
                permissions: PluginPermissions::default(),
                hooks: vec![HookPoint::BeforeResponse],
                sandbox: SandboxConfig {
                    enabled: true,
                    daily_request_limit: Some(3),
                    ..Default::default()
                },
                ..Default::default()
            }
        }
    }

    manager.install(LimitedPlugin).unwrap();
    manager.enable("limited").unwrap();

    // 前 3 次激活应该成功
    for i in 0..3 {
        manager.activate("limited").unwrap();
        manager.deactivate().unwrap();
        println!("  ├─ 第 {} 次激活/停用成功", i + 1);
    }

    // 第 4 次应该被限制
    let result = manager.activate("limited");
    match result {
        Ok(_) => panic!("第4次激活应该被拒绝"),
        Err(e) => {
            println!("  ├─ 第 4 次激活被拒绝: {}", e);
        }
    }

    Ok("速率限制正确：3 次后拒绝，符合沙箱配置".to_string())
}

/// Test 10: 未注册钩子点默认行为
fn test_unregistered_hook_default() -> Result<String, String> {
    let registry = HookRegistry::new();

    // 没有任何钩子注册，执行应该返回默认 continue
    let ctx = HookContext::new("user1", "session1", "test");
    let result = registry.execute(&HookPoint::BeforeToolCall, ctx);

    assert!(result.should_continue);
    assert!(result.modified_input.is_none());
    assert!(result.modified_response.is_none());
    assert!(result.error.is_none());

    Ok("未注册钩子点返回默认 continue，无修改".to_string())
}
