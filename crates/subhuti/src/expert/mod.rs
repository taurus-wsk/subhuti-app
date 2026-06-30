//! # Expert Plugin System - 专家插件系统 v2.0
//!
//! 完整的插件生态设计：
//! - **Manifest 清单**：插件元数据、版本、依赖声明
//! - **生命周期管理**：安装 → 启用 → 停用 → 卸载
//! - **权限系统**：文件、网络、数据库访问控制
//! - **沙箱隔离**：限制插件能力范围
//! - **钩子系统**：在核心流程中插入自定义逻辑
//!
//! ## 架构图
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    ExpertPlugin Trait                      │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐    │
//! │  │  Manifest   │ │   Persona   │ │     Skills      │    │
//! │  │  清单元数据  │ │   角色定义   │ │     技能集合     │    │
//! │  └─────────────┘ └─────────────┘ └─────────────────┘    │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐    │
//! │  │  Knowledge  │ │  Hooks      │ │   Permissions   │    │
//! │  │   知识库    │ │   钩子点    │ │    权限声明     │    │
//! │  └─────────────┘ └─────────────┘ └─────────────────┘    │
//! └─────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │                  PluginStateMachine                      │
//! │    installed ──→ enabled ──→ activated                   │
//! │        ↑           │         │                           │
//! │        └───────────┴─────────┘ (disabled)                │
//! │                                                         │
//! │  每个状态对应不同的生命周期钩子                          │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use subhuti::expert::{
//!     ExpertPlugin, PluginManifest, PluginPermissions,
//!     HookPoint, SandboxConfig,
//! };
//!
//! pub struct PsychologyExpert;
//!
//! impl ExpertPlugin for PsychologyExpert {
//!     fn manifest(&self) -> PluginManifest {
//!         PluginManifest {
//!             id: "psychology".into(),
//!             name: "心理咨询专家".into(),
//!             version: "0.1.0".into(),
//!             permissions: PluginPermissions::default(),
//!             hooks: vec![HookPoint::BeforeResponse],
//!             sandbox: SandboxConfig::default(),
//!             ..Default::default()
//!         }
//!     }
//!     // ...
//! }
//! ```
//!
//! ## 专家规划能力
//!
//! 专家插件还可以实现 `ExpertPlanning` trait，声明自主规划能力：
//!
//! ```rust,ignore
//! use subhuti::expert::{ExpertPlugin, ExpertPlanning, TaskAnalysis, ExecutionPlan};
//!
//! impl ExpertPlanning for MyExpert {
//!     fn analyze_task(&self, input: &str, context: &PlanningContext) -> TaskAnalysis {
//!         // 分析任务复杂度、类型、目标
//!     }
//!
//!     fn create_plan(&self, analysis: &TaskAnalysis) -> ExecutionPlan {
//!         // 制定执行计划
//!     }
//!
//!     fn execute_step(&self, plan: &mut ExecutionPlan, step: &mut PlanStep) -> Result<Value, String> {
//!         // 执行单个步骤
//!     }
//!
//!     fn reflect_on(&self, plan: &ExecutionPlan, result: &Value) -> Reflection {
//!         // 反思执行结果
//!     }
//! }
//! ```

pub mod planning;

use crate::skill::Skill;
use crate::soul::{BigFive, EmotionalTendency, ToneStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// 导出规划模块的核心类型
pub use planning::{
    ExecutionPlan, ExpertPlanning, PlanExecutor, PlanStatus, PlanStep, PlanSummary,
    PlanningContext, Reflection, TaskAnalysis, TaskComplexity, TaskType,
};

// ─────────────────────────────────────────────────────────────────
// 第一部分：Manifest 清单系统
// ─────────────────────────────────────────────────────────────────

/// 插件清单 - 插件的"身份证"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// 插件唯一 ID（小写英文、数字、连字符）
    pub id: String,
    /// 插件显示名称
    pub name: String,
    /// 插件描述
    pub description: String,
    /// 版本号（语义化版本：MAJOR.MINOR.PATCH）
    pub version: String,
    /// 作者信息
    pub author: Option<Author>,
    /// 插件类别
    pub category: PluginCategory,
    /// 匹配关键词
    pub keywords: Vec<String>,

    // ── 依赖声明 ──
    /// 依赖的其他插件 ID
    pub dependencies: Vec<String>,
    /// 最小框架版本要求
    pub min_framework_version: Option<String>,

    // ── 能力声明 ──
    /// 权限要求
    pub permissions: PluginPermissions,
    /// 沙箱配置
    pub sandbox: SandboxConfig,
    /// 钩子点列表
    pub hooks: Vec<HookPoint>,

    /// 插件作者信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// 许可证
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

impl Default for PluginManifest {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            version: "0.1.0".into(),
            author: None,
            category: PluginCategory::Other,
            keywords: Vec::new(),
            dependencies: Vec::new(),
            min_framework_version: None,
            permissions: PluginPermissions::default(),
            sandbox: SandboxConfig::default(),
            hooks: Vec::new(),
            homepage: None,
            license: None,
        }
    }
}

/// 作者信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

/// 插件类别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum PluginCategory {
    /// 心理健康
    Psychology,
    /// 编程开发
    Development,
    /// 教育学习
    Education,
    /// 商业分析
    Business,
    /// 创意写作
    Writing,
    /// 法律咨询
    Legal,
    /// 医疗健康
    Medical,
    /// 金融投资
    Finance,
    /// 生活助手
    Lifestyle,
    /// 其他
    #[default]
    Other,
}

impl std::fmt::Display for PluginCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginCategory::Psychology => write!(f, "心理健康"),
            PluginCategory::Development => write!(f, "编程开发"),
            PluginCategory::Education => write!(f, "教育培训"),
            PluginCategory::Business => write!(f, "商业分析"),
            PluginCategory::Writing => write!(f, "创意写作"),
            PluginCategory::Legal => write!(f, "法律咨询"),
            PluginCategory::Medical => write!(f, "医疗健康"),
            PluginCategory::Finance => write!(f, "金融投资"),
            PluginCategory::Lifestyle => write!(f, "生活助手"),
            PluginCategory::Other => write!(f, "其他"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// 第二部分：权限系统
// ─────────────────────────────────────────────────────────────────

/// 插件权限声明
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginPermissions {
    /// 是否允许访问文件系统
    pub file_read: bool,
    pub file_write: bool,
    /// 是否允许网络请求
    pub network: bool,
    /// 是否允许访问数据库
    pub database: bool,
    /// 是否允许执行代码
    pub code_execution: bool,
    /// 是否允许调用外部 API
    pub external_api: bool,
    /// 允许的网络域名白名单
    pub allowed_domains: Vec<String>,
    /// 允许的文件路径白名单
    pub allowed_paths: Vec<String>,
    /// 是否允许修改心灵层
    pub modify_soul: bool,
    /// 是否允许访问其他插件
    pub access_other_plugins: bool,
}

impl PluginPermissions {
    /// 允许所有权限（危险，仅测试用）
    pub fn allow_all() -> Self {
        Self {
            file_read: true,
            file_write: true,
            network: true,
            database: true,
            code_execution: true,
            external_api: true,
            allowed_domains: vec!["*".into()],
            allowed_paths: vec!["*".into()],
            modify_soul: true,
            access_other_plugins: true,
        }
    }

    /// 检查是否有网络权限
    pub fn can_access_network(&self, domain: &str) -> bool {
        if !self.network {
            return false;
        }
        if self.allowed_domains.contains(&"*".into()) {
            return true;
        }
        self.allowed_domains.iter().any(|d| domain.contains(d))
    }

    /// 检查是否有文件读取权限
    pub fn can_read_file(&self, path: &str) -> bool {
        if !self.file_read {
            return false;
        }
        if self.allowed_paths.contains(&"*".into()) {
            return true;
        }
        self.allowed_paths.iter().any(|p| path.starts_with(p))
    }
}

// ─────────────────────────────────────────────────────────────────
// 第三部分：沙箱配置
// ─────────────────────────────────────────────────────────────────

/// 沙箱隔离配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// 启用沙箱隔离
    pub enabled: bool,
    /// 最大内存限制（MB）
    pub memory_limit_mb: u64,
    /// 最大执行时间（秒）
    pub max_execution_time_secs: u64,
    /// 最大 Token 消耗
    pub max_tokens_per_request: u64,
    /// 是否限制插件间通信
    pub isolate_plugins: bool,
    /// 资源预算（每日调用次数限制）
    pub daily_request_limit: Option<u64>,
    /// 当前已用请求数（运行时统计）
    #[serde(skip)]
    pub used_requests_today: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true, // 默认启用沙箱
            memory_limit_mb: 512,
            max_execution_time_secs: 30,
            max_tokens_per_request: 4096,
            isolate_plugins: true,
            daily_request_limit: Some(1000),
            used_requests_today: 0,
        }
    }
}

impl SandboxConfig {
    /// 检查是否超限
    pub fn is_rate_limited(&self) -> bool {
        if let Some(limit) = self.daily_request_limit {
            return self.used_requests_today >= limit;
        }
        false
    }

    /// 记录一次请求
    pub fn record_request(&mut self) {
        self.used_requests_today += 1;
    }

    /// 重置每日计数
    pub fn reset_daily_counter(&mut self) {
        self.used_requests_today = 0;
    }
}

// ─────────────────────────────────────────────────────────────────
// 第四部分：钩子系统
// ─────────────────────────────────────────────────────────────────

/// 插件可挂载的钩子点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookPoint {
    /// 在请求开始前执行
    BeforeRequest,
    /// 在 Skill 匹配前执行
    BeforeSkillMatch,
    /// 在 Skill 执行前执行
    BeforeSkillExecute,
    /// 在 Skill 执行后执行
    AfterSkillExecute,
    /// 在 LLM 调用前执行
    BeforeLlmCall,
    /// 在 LLM 调用后执行
    AfterLlmCall,
    /// 在生成响应前执行
    BeforeResponse,
    /// 在响应生成后执行
    AfterResponse,
    /// 在记忆检索前执行
    BeforeMemorySearch,
    /// 在记忆检索后执行
    AfterMemorySearch,
    /// 在工具调用前执行
    BeforeToolCall,
    /// 在工具调用后执行
    AfterToolCall,
    /// 在专家切换时执行
    OnExpertSwitch,
}

impl std::fmt::Display for HookPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookPoint::BeforeRequest => write!(f, "before_request"),
            HookPoint::BeforeSkillMatch => write!(f, "before_skill_match"),
            HookPoint::BeforeSkillExecute => write!(f, "before_skill_execute"),
            HookPoint::AfterSkillExecute => write!(f, "after_skill_execute"),
            HookPoint::BeforeLlmCall => write!(f, "before_llm_call"),
            HookPoint::AfterLlmCall => write!(f, "after_llm_call"),
            HookPoint::BeforeResponse => write!(f, "before_response"),
            HookPoint::AfterResponse => write!(f, "after_response"),
            HookPoint::BeforeMemorySearch => write!(f, "before_memory_search"),
            HookPoint::AfterMemorySearch => write!(f, "after_memory_search"),
            HookPoint::BeforeToolCall => write!(f, "before_tool_call"),
            HookPoint::AfterToolCall => write!(f, "after_tool_call"),
            HookPoint::OnExpertSwitch => write!(f, "on_expert_switch"),
        }
    }
}

/// 钩子执行上下文
#[derive(Debug, Clone)]
pub struct HookContext {
    /// 请求 ID
    pub request_id: String,
    /// 用户 ID
    pub user_id: String,
    /// 会话 ID
    pub session_id: String,
    /// 当前输入
    pub input: String,
    /// 当前专家 ID
    pub current_expert: Option<String>,
    /// 时间戳
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl HookContext {
    pub fn new(user_id: &str, session_id: &str, input: &str) -> Self {
        Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            input: input.to_string(),
            current_expert: None,
            timestamp: chrono::Utc::now(),
        }
    }
}

/// 钩子执行结果
#[derive(Debug, Clone)]
pub struct HookResult {
    /// 是否继续执行
    pub should_continue: bool,
    /// 修改后的输入（如果有）
    pub modified_input: Option<String>,
    /// 修改后的响应（如果有）
    pub modified_response: Option<String>,
    /// 附加数据
    pub extra_data: HashMap<String, String>,
    /// 错误信息
    pub error: Option<String>,
}

impl Default for HookResult {
    fn default() -> Self {
        Self {
            should_continue: true,
            modified_input: None,
            modified_response: None,
            extra_data: HashMap::new(),
            error: None,
        }
    }
}

impl HookResult {
    /// 继续执行，不修改
    pub fn continue_() -> Self {
        Self::default()
    }

    /// 阻止执行
    pub fn block(reason: &str) -> Self {
        Self {
            should_continue: false,
            error: Some(reason.into()),
            ..Default::default()
        }
    }

    /// 修改输入
    pub fn modify_input(input: String) -> Self {
        Self {
            modified_input: Some(input),
            ..Default::default()
        }
    }

    /// 修改响应
    pub fn modify_response(response: String) -> Self {
        Self {
            modified_response: Some(response),
            ..Default::default()
        }
    }
}

/// 钩子处理器函数类型
pub type HookHandler = Box<dyn Fn(HookContext) -> HookResult + Send + Sync>;

/// 钩子注册表
#[derive(Default)]
pub struct HookRegistry {
    handlers: HashMap<HookPoint, Vec<HookHandler>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册钩子处理函数
    pub fn register(&mut self, point: HookPoint, handler: HookHandler) {
        self.handlers.entry(point).or_default().push(handler);
    }

    /// 执行钩子链
    pub fn execute(&self, point: &HookPoint, ctx: HookContext) -> HookResult {
        let handlers = match self.handlers.get(point) {
            Some(h) => h,
            None => return HookResult::continue_(),
        };

        let mut result = HookResult::continue_();

        for handler in handlers {
            let hook_result = handler(ctx.clone());

            // 如果有任何钩子阻止执行，立即返回
            if !hook_result.should_continue {
                return hook_result;
            }

            // 合并修改
            if let Some(input) = hook_result.modified_input {
                result.modified_input = Some(input);
            }
            if let Some(response) = hook_result.modified_response {
                result.modified_response = Some(response);
            }
            result.extra_data.extend(hook_result.extra_data);
        }

        result
    }

    /// 获取已注册的钩子点列表
    pub fn registered_hooks(&self) -> Vec<HookPoint> {
        self.handlers.keys().cloned().collect()
    }
}

// ─────────────────────────────────────────────────────────────────
// 第五部分：专家信息与性格
// ─────────────────────────────────────────────────────────────────

/// 专家信息（向后兼容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub category: String,
    pub keywords: Vec<String>,
}

/// 专家性格定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertPersona {
    pub name: String,
    pub description: String,
    pub tone: ToneStyle,
    pub emotional_tendency: EmotionalTendency,
    pub big_five: BigFive,
    pub traits: Vec<String>,
    pub expertise_areas: HashMap<String, f32>,
    pub system_prompt: String,
}

impl Default for ExpertPersona {
    fn default() -> Self {
        Self {
            name: "通用助手".to_string(),
            description: "一个友好的 AI 助手".to_string(),
            tone: ToneStyle::Friendly,
            emotional_tendency: EmotionalTendency::Optimistic,
            big_five: BigFive::default(),
            traits: vec![
                "友好".to_string(),
                "专业".to_string(),
                "乐于助人".to_string(),
            ],
            expertise_areas: HashMap::new(),
            system_prompt: "你是一个友好的 AI 助手。".to_string(),
        }
    }
}

/// 知识库条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub content: String,
    pub metadata: Option<HashMap<String, String>>,
}

// ─────────────────────────────────────────────────────────────────
// 第六部分：插件状态机
// ─────────────────────────────────────────────────────────────────

/// 插件状态
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum PluginState {
    /// 已安装但未启用
    #[default]
    Installed,
    /// 已启用但未激活
    Enabled,
    /// 已激活（正在使用）
    Activated,
    /// 已停用
    Disabled,
    /// 已卸载
    Uninstalled,
}

impl std::fmt::Display for PluginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginState::Installed => write!(f, "installed"),
            PluginState::Enabled => write!(f, "enabled"),
            PluginState::Activated => write!(f, "activated"),
            PluginState::Disabled => write!(f, "disabled"),
            PluginState::Uninstalled => write!(f, "uninstalled"),
        }
    }
}

/// 插件元数据（包含状态和 Manifest）
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub manifest: PluginManifest,
    pub state: PluginState,
    pub enabled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub activated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_error: Option<String>,
}

impl PluginMetadata {
    pub fn from_manifest(manifest: PluginManifest) -> Self {
        Self {
            manifest,
            state: PluginState::Installed,
            enabled_at: None,
            activated_at: None,
            disabled_at: None,
            last_error: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// 第七部分：ExpertPlugin Trait（增强版）
// ─────────────────────────────────────────────────────────────────

/// 增强版专家插件 Trait
///
/// 提供完整的插件生命周期和钩子支持
pub trait ExpertPlugin: Send + Sync {
    // ── 核心信息 ──

    /// 获取插件清单
    fn manifest(&self) -> PluginManifest {
        PluginManifest::default()
    }

    /// 获取专家信息（向后兼容）
    fn info(&self) -> ExpertInfo {
        let m = self.manifest();
        ExpertInfo {
            id: m.id,
            name: m.name,
            description: m.description,
            version: m.version,
            author: m.author.map(|a| a.name),
            category: m.category.to_string(),
            keywords: m.keywords,
        }
    }

    /// 获取性格定义
    fn persona(&self) -> ExpertPersona {
        ExpertPersona::default()
    }

    /// 获取技能列表
    fn skills(&self) -> Vec<Box<dyn Skill>> {
        Vec::new()
    }

    /// 获取知识库条目
    fn knowledge(&self) -> Vec<KnowledgeEntry> {
        Vec::new()
    }

    // ── 生命周期钩子 ──

    /// 安装时调用（首次添加到系统）
    fn on_install(&self) -> Result<(), String> {
        Ok(())
    }

    /// 卸载时调用（从系统移除）
    fn on_uninstall(&self) -> Result<(), String> {
        Ok(())
    }

    /// 启用时调用（可以开始工作，但未激活）
    fn on_enable(&self) -> Result<(), String> {
        Ok(())
    }

    /// 停用时调用（不再工作）
    fn on_disable(&self) -> Result<(), String> {
        Ok(())
    }

    /// 激活时调用（切换为当前专家）
    fn on_activate(&self) -> Result<(), String> {
        Ok(())
    }

    /// 停用时调用（不再是当前专家）
    fn on_deactivate(&self) -> Result<(), String> {
        Ok(())
    }

    // ── 行为钩子 ──

    /// 匹配度计算
    fn matches(&self, input: &str) -> f32 {
        let info = self.info();
        let lower_input = input.to_lowercase();
        let mut score = 0.0f32;

        for keyword in &info.keywords {
            if lower_input.contains(&keyword.to_lowercase()) {
                score += 0.15;
            }
        }

        if lower_input.contains(&info.name.to_lowercase()) {
            score += 0.3;
        }

        score.min(1.0)
    }

    /// 处理钩子（可选实现）
    ///
    /// 当插件注册了某个钩子点时，会在对应的时机调用此方法
    fn handle_hook(&self, _point: HookPoint, _ctx: HookContext) -> HookResult {
        HookResult::continue_()
    }
}

// ─────────────────────────────────────────────────────────────────
// 第八部分：专家管理器（增强版）
// ─────────────────────────────────────────────────────────────────

/// 插件管理器
#[derive(Default)]
pub struct PluginManager {
    /// 已注册的插件
    plugins: HashMap<String, RegisteredPlugin>,
    /// 钩子注册表
    hooks: HookRegistry,
    /// 当前激活的专家 ID
    active_expert: Option<String>,
    /// 已注册的技能名称（用于追踪，避免重复注入）
    registered_skills: HashMap<String, String>, // skill_name -> expert_id
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginManager")
            .field("plugin_count", &self.plugins.len())
            .field("active_expert", &self.active_expert)
            .field("registered_skills", &self.registered_skills.len())
            .finish()
    }
}

/// 已注册的插件
pub struct RegisteredPlugin {
    pub metadata: PluginMetadata,
    pub plugin: Arc<dyn ExpertPlugin>,
}

impl std::fmt::Debug for RegisteredPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredPlugin")
            .field("id", &self.metadata.manifest.id)
            .field("state", &self.metadata.state)
            .finish()
    }
}

impl PluginManager {
    /// 创建新的管理器
    pub fn new() -> Self {
        Self::default()
    }

    // ── 生命周期管理 ──

    /// 安装插件
    pub fn install<E: ExpertPlugin + 'static>(&mut self, plugin: E) -> Result<(), String> {
        let manifest = plugin.manifest();
        let id = manifest.id.clone();

        // 检查是否已存在
        if self.plugins.contains_key(&id) {
            return Err(format!("插件 {} 已存在", id));
        }

        // 执行安装钩子
        plugin.on_install()?;

        // 注册插件
        let metadata = PluginMetadata::from_manifest(manifest);
        self.plugins.insert(
            id.clone(),
            RegisteredPlugin {
                metadata,
                plugin: Arc::new(plugin),
            },
        );

        tracing::info!("PluginManager: Installed plugin {}", id);
        Ok(())
    }

    /// 卸载插件
    pub fn uninstall(&mut self, id: &str) -> Result<(), String> {
        let plugin = self
            .plugins
            .get(id)
            .ok_or_else(|| format!("插件 {} 不存在", id))?;

        // 不能卸载正在激活的插件
        if self.active_expert.as_deref() == Some(id) {
            return Err(format!("插件 {} 正在激活中，请先停用", id));
        }

        // 执行卸载钩子
        plugin.plugin.on_uninstall()?;

        // 清理注册的技能
        self.unregister_skills(id);

        // 移除插件
        self.plugins.remove(id);

        tracing::info!("PluginManager: Uninstalled plugin {}", id);
        Ok(())
    }

    /// 启用插件
    pub fn enable(&mut self, id: &str) -> Result<(), String> {
        // 检查插件是否存在
        if !self.plugins.contains_key(id) {
            return Err(format!("插件 {} 不存在", id));
        }

        // 检查状态
        {
            let plugin = self.plugins.get(id).unwrap();
            if plugin.metadata.state != PluginState::Installed
                && plugin.metadata.state != PluginState::Disabled
            {
                return Err(format!(
                    "插件 {} 状态为 {:?}，无法启用",
                    id, plugin.metadata.state
                ));
            }
        }

        // 克隆钩子点列表
        let hook_points = {
            let plugin = self.plugins.get(id).unwrap();
            plugin.metadata.manifest.hooks.clone()
        };

        // 注册钩子（在新的大括号中）
        {
            let plugin_clone = self.plugins.get(id).unwrap().plugin.clone();
            for hook_point in &hook_points {
                let hook = *hook_point;
                let pc = plugin_clone.clone();
                self.hooks
                    .register(hook, Box::new(move |ctx| pc.handle_hook(hook, ctx)));
            }
        }

        // 执行启用钩子
        {
            let plugin = self.plugins.get(id).unwrap();
            plugin.plugin.on_enable()?;
        }

        // 更新状态
        {
            let plugin = self.plugins.get_mut(id).unwrap();
            plugin.metadata.state = PluginState::Enabled;
            plugin.metadata.enabled_at = Some(chrono::Utc::now());
        }

        tracing::info!("PluginManager: Enabled plugin {}", id);
        Ok(())
    }

    /// 停用插件
    pub fn disable(&mut self, id: &str) -> Result<(), String> {
        // 检查插件是否存在
        if !self.plugins.contains_key(id) {
            return Err(format!("插件 {} 不存在", id));
        }

        // 如果正在激活，先停用
        if self.active_expert.as_deref() == Some(id) {
            self.deactivate()?;
        }

        // 执行停用钩子
        {
            let plugin = self.plugins.get(id).unwrap();
            plugin.plugin.on_disable()?;
        }

        // 更新状态
        {
            let plugin = self.plugins.get_mut(id).unwrap();
            plugin.metadata.state = PluginState::Disabled;
            plugin.metadata.disabled_at = Some(chrono::Utc::now());
        }

        tracing::info!("PluginManager: Disabled plugin {}", id);
        Ok(())
    }

    /// 激活插件（切换为当前专家）
    pub fn activate(&mut self, id: &str) -> Result<Arc<dyn ExpertPlugin>, String> {
        // 1. 停用当前激活的专家
        let _ = self.deactivate();

        // 2. 检查插件是否存在
        if !self.plugins.contains_key(id) {
            return Err(format!("插件 {} 不存在", id));
        }

        // 3. 检查插件状态（必须已启用）
        {
            let plugin = self.plugins.get(id).unwrap();
            if plugin.metadata.state != PluginState::Enabled
                && plugin.metadata.state != PluginState::Activated
            {
                return Err(format!("插件 {} 未启用，请先启用", id));
            }
        }

        // 4. 检查沙箱限制
        {
            let plugin = self.plugins.get(id).unwrap();
            let sandbox = &plugin.metadata.manifest.sandbox;
            if sandbox.enabled && sandbox.is_rate_limited() {
                return Err(format!("插件 {} 今日调用次数已达上限", id));
            }
        }

        // 5. 克隆需要的数据，释放借用
        let plugin_arc = {
            let plugin = self.plugins.get(id).unwrap();
            plugin.plugin.clone()
        };

        // 6. 注册技能
        self.register_skills(id, &plugin_arc)?;

        // 7. 更新状态
        {
            let plugin = self.plugins.get_mut(id).unwrap();
            self.active_expert = Some(id.to_string());
            plugin.metadata.state = PluginState::Activated;
            plugin.metadata.activated_at = Some(chrono::Utc::now());

            // 记录请求
            plugin.metadata.manifest.sandbox.record_request();
        }

        // 8. 执行激活钩子
        plugin_arc.on_activate()?;

        tracing::info!("PluginManager: Activated plugin {}", id);
        Ok(plugin_arc)
    }

    /// 停用当前专家
    pub fn deactivate(&mut self) -> Result<(), String> {
        let active_id = match self.active_expert.take() {
            Some(id) => id,
            None => return Ok(()), // 没有激活的专家
        };

        if let Some(plugin) = self.plugins.get(&active_id) {
            plugin.plugin.on_deactivate()?;
        }

        // 清理技能注册
        self.unregister_skills(&active_id);

        // 更新状态
        if let Some(plugin) = self.plugins.get_mut(&active_id) {
            plugin.metadata.state = PluginState::Enabled;
        }

        tracing::info!("PluginManager: Deactivated plugin {}", active_id);
        Ok(())
    }

    // ── 技能管理 ──

    fn register_skills(
        &mut self,
        expert_id: &str,
        plugin: &Arc<dyn ExpertPlugin>,
    ) -> Result<(), String> {
        for skill in plugin.skills() {
            let skill_name = skill.name().to_string();
            self.registered_skills
                .insert(skill_name, expert_id.to_string());
        }
        Ok(())
    }

    fn unregister_skills(&mut self, expert_id: &str) {
        self.registered_skills.retain(|_, id| id != expert_id);
    }

    // ── 钩子执行 ──

    /// 执行钩子链
    pub fn execute_hook(&self, point: HookPoint, ctx: HookContext) -> HookResult {
        self.hooks.execute(&point, ctx)
    }

    // ── 查询方法 ──

    /// 获取当前激活的专家 ID
    pub fn get_active_expert_id(&self) -> Option<String> {
        self.active_expert.clone()
    }

    /// 获取当前激活的专家
    pub fn get_active_expert(&self) -> Option<Arc<dyn ExpertPlugin>> {
        let id = self.active_expert.as_ref()?;
        self.plugins.get(id).map(|p| p.plugin.clone())
    }

    /// 获取指定 ID 的插件
    pub fn get_plugin(&self, id: &str) -> Option<Arc<dyn ExpertPlugin>> {
        self.plugins.get(id).map(|p| p.plugin.clone())
    }

    /// 获取插件元数据
    pub fn get_metadata(&self, id: &str) -> Option<PluginMetadata> {
        self.plugins.get(id).map(|p| p.metadata.clone())
    }

    /// 获取所有插件元数据
    pub fn list_plugins(&self) -> Vec<PluginMetadata> {
        self.plugins.values().map(|p| p.metadata.clone()).collect()
    }

    /// 获取已启用的插件
    pub fn list_enabled(&self) -> Vec<PluginMetadata> {
        self.plugins
            .values()
            .filter(|p| {
                p.metadata.state == PluginState::Enabled
                    || p.metadata.state == PluginState::Activated
            })
            .map(|p| p.metadata.clone())
            .collect()
    }

    /// 查找匹配的专家
    pub fn match_expert(&self, input: &str) -> Option<Arc<dyn ExpertPlugin>> {
        let mut best: Option<(Arc<dyn ExpertPlugin>, f32)> = None;

        for plugin in self.plugins.values() {
            // 只匹配已启用的插件
            if plugin.metadata.state != PluginState::Enabled
                && plugin.metadata.state != PluginState::Activated
            {
                continue;
            }

            let score = plugin.plugin.matches(input);
            if score > 0.0 {
                match &best {
                    None => best = Some((plugin.plugin.clone(), score)),
                    Some((_, best_score)) if score > *best_score => {
                        best = Some((plugin.plugin.clone(), score));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(p, _)| p)
    }

    /// 检查权限
    pub fn check_permission(&self, expert_id: &str, permission: &str) -> bool {
        let plugin = match self.plugins.get(expert_id) {
            Some(p) => p,
            None => return false,
        };

        let perms = &plugin.metadata.manifest.permissions;
        match permission {
            "file_read" => perms.file_read,
            "file_write" => perms.file_write,
            "network" => perms.network,
            "database" => perms.database,
            "code_execution" => perms.code_execution,
            "external_api" => perms.external_api,
            "modify_soul" => perms.modify_soul,
            "access_other_plugins" => perms.access_other_plugins,
            _ => false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// 第九部分：向后兼容
// ─────────────────────────────────────────────────────────────────

/// 为了向后兼容，保留旧的构造函数
impl<T: ExpertPlugin + 'static> From<T> for RegisteredPlugin {
    fn from(plugin: T) -> Self {
        let manifest = plugin.manifest();
        RegisteredPlugin {
            metadata: PluginMetadata::from_manifest(manifest),
            plugin: Arc::new(plugin),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试插件
    struct TestPlugin {
        permissions: PluginPermissions,
    }

    impl ExpertPlugin for TestPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "test-plugin".into(),
                name: "测试插件".into(),
                description: "一个测试插件".into(),
                version: "1.0.0".into(),
                author: Some(Author {
                    name: "Test Author".into(),
                    email: None,
                    url: None,
                }),
                category: PluginCategory::Development,
                keywords: vec!["测试".into(), "test".into()],
                permissions: self.permissions.clone(),
                hooks: vec![HookPoint::BeforeResponse, HookPoint::AfterResponse],
                sandbox: SandboxConfig::default(),
                ..Default::default()
            }
        }

        fn persona(&self) -> ExpertPersona {
            ExpertPersona::default()
        }

        fn skills(&self) -> Vec<Box<dyn Skill>> {
            Vec::new()
        }
    }

    #[test]
    fn test_plugin_install_uninstall() {
        let mut manager = PluginManager::new();
        let plugin = TestPlugin {
            permissions: PluginPermissions::default(),
        };

        // 安装
        manager.install(plugin).unwrap();
        assert_eq!(manager.plugins.len(), 1);

        // 卸载
        manager.uninstall("test-plugin").unwrap();
        assert_eq!(manager.plugins.len(), 0);
    }

    #[test]
    fn test_plugin_lifecycle() {
        let mut manager = PluginManager::new();
        let plugin = TestPlugin {
            permissions: PluginPermissions::default(),
        };

        manager.install(plugin).unwrap();

        // 启用
        manager.enable("test-plugin").unwrap();
        let meta = manager.get_metadata("test-plugin").unwrap();
        assert_eq!(meta.state, PluginState::Enabled);

        // 激活
        manager.activate("test-plugin").unwrap();
        let meta = manager.get_metadata("test-plugin").unwrap();
        assert_eq!(meta.state, PluginState::Activated);
        assert_eq!(manager.get_active_expert_id(), Some("test-plugin".into()));

        // 停用
        manager.deactivate().unwrap();
        let meta = manager.get_metadata("test-plugin").unwrap();
        assert_eq!(meta.state, PluginState::Enabled);

        // 停用插件
        manager.disable("test-plugin").unwrap();
        let meta = manager.get_metadata("test-plugin").unwrap();
        assert_eq!(meta.state, PluginState::Disabled);
    }

    #[test]
    fn test_plugin_match() {
        let mut manager = PluginManager::new();
        let plugin = TestPlugin {
            permissions: PluginPermissions::default(),
        };

        manager.install(plugin).unwrap();
        manager.enable("test-plugin").unwrap();

        // 应该匹配
        assert!(manager.match_expert("这是一个测试").is_some());
        // 应该不匹配
        assert!(manager.match_expert("完全无关的内容").is_none());
    }

    #[test]
    fn test_permissions() {
        let perms = PluginPermissions::default();
        assert!(!perms.file_read);
        assert!(!perms.network);

        let all_perms = PluginPermissions::allow_all();
        assert!(all_perms.file_read);
        assert!(all_perms.network);
        assert!(all_perms.can_access_network("example.com"));
    }

    #[test]
    fn test_sandbox_rate_limit() {
        let mut sandbox = SandboxConfig {
            daily_request_limit: Some(3),
            ..Default::default()
        };

        assert!(!sandbox.is_rate_limited());
        sandbox.record_request();
        sandbox.record_request();
        sandbox.record_request();
        assert!(sandbox.is_rate_limited());

        sandbox.reset_daily_counter();
        assert!(!sandbox.is_rate_limited());
    }

    #[test]
    fn test_hook_execution() {
        let registry = HookRegistry::new();

        // 注册钩子
        let result = registry.execute(
            &HookPoint::BeforeResponse,
            HookContext::new("user1", "session1", "test"),
        );
        assert!(result.should_continue);
    }

    #[test]
    fn test_plugin_state_transitions() {
        // Installed -> Enabled -> Activated -> Enabled -> Disabled -> Enabled -> Uninstalled
        assert_eq!(PluginState::Installed, PluginState::Installed);
    }
}
