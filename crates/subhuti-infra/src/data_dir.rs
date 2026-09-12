//! # 统一数据目录
//!
//! 项目里所有运行时落盘的数据（trace 链路库、藏经阁记忆库等）都放在**同一个目录**下，
//! 避免"文件散落在 cwd 相对路径 / 系统临时目录 / 内存"各处、谁也说不清在哪的问题。
//!
//! ## 解析规则
//!
//! 1. 环境变量 `SUBHUTI_DATA_DIR`（优先）；
//! 2. 否则 `~/.subhuti/data`（macOS / Linux）。
//!
//! ## 为什么默认不是 `./data`
//!
//! MCP server 常被 WorkBuddy 等客户端从**任意工作目录**拉起，用相对路径会导致
//! 每次落点不同（数据"漂移"）。`~/.subhuti/data` 与 cwd 无关，HTTP 进程与 MCP
//! 进程必定解析到同一个绝对路径，和之前放临时目录的汇聚效果一致，且不会被系统清理。
//!
//! ## 覆盖单个文件
//!
//! 仍保留细粒度环境变量，便于测试或分盘存放：
//! - `SUBHUTI_TRACE_SQLITE`：trace 链路库完整路径
//! - `SUBHUTI_SUTRA_SQLITE`：藏经阁记忆库完整路径

use std::path::PathBuf;

/// 环境变量：统一数据目录
pub const ENV_DATA_DIR: &str = "SUBHUTI_DATA_DIR";

/// 默认子目录名（位于用户主目录下）
const DEFAULT_DIR_NAME: &str = ".subhuti/data";

/// 解析数据目录（**绝对路径**，不依赖 cwd）
///
/// - `SUBHUTI_DATA_DIR` 存在 → 直接用（相对路径会被原样返回，由调用方自行 canonicalize）
/// - 否则 `~/.subhuti/data`；`HOME` 缺失时兜底 `./data`
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(ENV_DATA_DIR) {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim();
        if !home.is_empty() {
            return PathBuf::from(home).join(DEFAULT_DIR_NAME);
        }
    }

    PathBuf::from("data")
}

/// 确保数据目录存在（不存在则递归创建），返回该目录
pub fn ensure_data_dir() -> std::io::Result<PathBuf> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// trace 链路库路径：`SUBHUTI_TRACE_SQLITE` > `<data_dir>/traces.sqlite`
pub fn trace_db_path() -> String {
    if let Ok(p) = std::env::var("SUBHUTI_TRACE_SQLITE") {
        let p = p.trim();
        if !p.is_empty() {
            return p.to_string();
        }
    }
    data_dir()
        .join("traces.sqlite")
        .to_string_lossy()
        .to_string()
}

/// 藏经阁记忆库路径：`SUBHUTI_SUTRA_SQLITE` > `<data_dir>/sutra_library.sqlite`
pub fn sutra_db_path() -> String {
    if let Ok(p) = std::env::var("SUBHUTI_SUTRA_SQLITE") {
        let p = p.trim();
        if !p.is_empty() {
            return p.to_string();
        }
    }
    data_dir()
        .join("sutra_library.sqlite")
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dir_is_under_home() {
        // 该断言依赖 HOME；极端环境（无 HOME）下退化为 ./data，跳过
        if std::env::var("HOME").is_ok() {
            let d = data_dir();
            assert!(
                d.ends_with(".subhuti/data"),
                "默认数据目录应以 .subhuti/data 结尾，实际: {:?}",
                d
            );
        }
    }

    #[test]
    fn derived_paths_are_absolute_and_under_data_dir() {
        if std::env::var("HOME").is_err() {
            return;
        }
        let dir = data_dir();
        let trace = PathBuf::from(trace_db_path());
        let sutra = PathBuf::from(sutra_db_path());
        assert!(trace.is_absolute(), "trace 路径应为绝对路径: {:?}", trace);
        assert!(sutra.is_absolute(), "sutra 路径应为绝对路径: {:?}", sutra);
        assert!(trace.starts_with(&dir));
        assert!(sutra.starts_with(&dir));
        assert_ne!(trace, sutra);
    }
}
