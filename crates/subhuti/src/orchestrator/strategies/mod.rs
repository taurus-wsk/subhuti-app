//! # 调度策略模块
//!
//! 包含 5 种调度策略的具体实现：
//! - SimpleDispatch: 单专家直连
//! - Pipeline: 串行流水线
//! - MapReduce: 并行发散-汇总
//! - ManagerWorker: 主管-工人模式
//! - CritiqueRevise: 评审迭代模式

pub mod critique_revise;
pub mod manager_worker;
pub mod map_reduce;
pub mod pipeline;
pub mod simple_dispatch;
