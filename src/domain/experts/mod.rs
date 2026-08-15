//! # 领域专家开发基地
//!
//! 在这里开发领域专家，每个专家实现 `DomainExpert` trait。
//!
//! ## 开发模式
//!
//! 1. 在此目录下创建新文件（如 `blender.rs`）
//! 2. 实现 `DomainExpert` trait（纯领域接口，不依赖框架）
//! 3. 通过 `DomainLlm` 接口获取 LLM 能力（依赖注入）
//! 4. 在 `application/app_service.rs` 中注册到 ExpertRepositoryPort
//!
//! ## 转 WASM 插件
//!
//! 领域专家代码可整体提取为独立 crate：
//! 1. 将专家代码移到 `crates/subhuti-expert-xxx-wasm/`
//! 2. 实现 WASM 插件入口
//! 3. 编译为 `wasm32-wasip1` 目标
//! 4. 通过适配器加载到平台
//!
//! 因为领域专家只依赖领域层自身定义的接口，
//! 提取时无需修改核心逻辑。

pub mod blender;
pub mod rust_expert;

use std::sync::Arc;

use crate::domain::traits::DomainExpert;

/// 创建所有领域专家实例
///
/// 返回领域层接口的专家列表，由应用层负责注册到 ExpertRepositoryPort。
pub fn create_all_experts() -> Vec<Arc<dyn DomainExpert>> {
    vec![
        Arc::new(blender::BlenderExpert::new()),
        Arc::new(rust_expert::RustExpert::new()),
    ]
}
