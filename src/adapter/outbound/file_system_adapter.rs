//! # 本地文件系统适配器
//!
//! 实现 `FileSystemPort` 端口，使用 tokio::fs 提供异步文件操作。
//! 所有路径相对于项目工作目录（workspace_folder）。

use crate::domain::ports::FileSystemPort;

/// 本地文件系统适配器
///
/// 提供文件读写、目录列表、文件搜索等能力。
/// 路径处理：相对路径相对于当前工作目录，绝对路径直接使用。
pub struct LocalFileSystemAdapter;

impl LocalFileSystemAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 将路径解析为绝对路径
    fn resolve_path(path: &str) -> std::path::PathBuf {
        let p = std::path::Path::new(path);
        if p.is_relative() {
            // 相对于当前工作目录
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(p)
        } else {
            p.to_path_buf()
        }
    }
}

impl FileSystemPort for LocalFileSystemAdapter {
    fn read_file(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> {
        let path = Self::resolve_path(path);
        Box::pin(async move {
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("读取文件 {} 失败: {}", path.display(), e))
        })
    }

    fn write_file(
        &self,
        path: &str,
        content: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>> {
        let path = Self::resolve_path(path);
        let content = content.to_string();
        Box::pin(async move {
            // 幂等写：文件已存在且内容完全相同则跳过，避免重复/覆盖产生多余产物
            if let Ok(existing) = tokio::fs::read_to_string(&path).await {
                if existing == content {
                    return Ok(());
                }
            }
            // 自动创建父目录
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
            }
            tokio::fs::write(&path, &content)
                .await
                .map_err(|e| format!("写入文件 {} 失败: {}", path.display(), e))
        })
    }

    fn list_dir(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, String>> + Send>>
    {
        let path = Self::resolve_path(path);
        Box::pin(async move {
            let mut entries = Vec::new();
            let mut read_dir = tokio::fs::read_dir(&path)
                .await
                .map_err(|e| format!("读取目录 {} 失败: {}", path.display(), e))?;
            while let Some(entry) = read_dir
                .next_entry()
                .await
                .map_err(|e| format!("读取目录项失败: {}", e))?
            {
                entries.push(entry.file_name().to_string_lossy().to_string());
            }
            entries.sort();
            Ok(entries)
        })
    }

    fn search_files(
        &self,
        pattern: &str,
        root: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, String>> + Send>>
    {
        let root = Self::resolve_path(root);
        let pattern = pattern.to_string();
        Box::pin(async move {
            let mut results = Vec::new();
            let mut dirs = vec![root.clone()];

            // 简单 glob 模式匹配：支持 "**/*.rs"、"*.toml" 等
            // 将模式拆分为文件名通配符部分，并确定是否递归搜索
            let is_recursive = pattern.contains("**/");
            let file_pattern = if is_recursive {
                pattern.replace("**/", "")
            } else {
                pattern.clone()
            };
            // 从模式中提取扩展名（如 ".rs"）或文件名通配符（如 "*.toml"）
            let file_suffix = if file_pattern.starts_with("*.") {
                file_pattern.trim_start_matches('*')
            } else {
                &file_pattern
            };

            // 递归遍历目录
            while let Some(dir) = dirs.pop() {
                let mut read_dir = match tokio::fs::read_dir(&dir).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                while let Some(entry) = read_dir.next_entry().await.unwrap_or(None) {
                    let path = entry.path();
                    if path.is_dir() {
                        if is_recursive {
                            dirs.push(path);
                        }
                    } else if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy();
                        if is_recursive {
                            // 递归模式：匹配文件名后缀
                            if name_str.ends_with(file_suffix) {
                                results.push(path.to_string_lossy().to_string());
                            }
                        } else {
                            // 非递归模式：匹配完整文件名
                            let glob_pattern = pattern.trim_start_matches("*.");
                            if name_str == pattern || name_str.ends_with(glob_pattern) {
                                results.push(path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }

            results.sort();
            Ok(results)
        })
    }

    fn exists(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> {
        let path = Self::resolve_path(path);
        Box::pin(async move { tokio::fs::try_exists(&path).await.unwrap_or(false) })
    }

    fn create_dir(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>> {
        let path = Self::resolve_path(path);
        Box::pin(async move {
            tokio::fs::create_dir_all(&path)
                .await
                .map_err(|e| format!("创建目录 {} 失败: {}", path.display(), e))
        })
    }

    fn delete_file(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>> {
        let path = Self::resolve_path(path);
        Box::pin(async move {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| format!("删除文件 {} 失败: {}", path.display(), e))
        })
    }
}
