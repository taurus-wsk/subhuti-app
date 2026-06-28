//! 配置加载模块
//!
//! 从 TOML 文件加载配置，支持环境变量覆盖。
//! 优先级：环境变量 > Subhuti.toml > 代码默认值

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct AppConfig {
    pub llm: LlmConfig,
    pub database: DatabaseConfig,
    pub http: HttpConfig,
    pub logging: LoggingConfig,
    pub memory: MemoryConfig,
    pub soul: SoulConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub api_url: String,
    pub temperature: f64,
    pub max_tokens: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    pub addr: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct LoggingConfig {
    pub level: String,
    pub dir: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct MemoryConfig {
    pub short_term_limit: usize,
    pub archive_threshold: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SoulConfig {
    pub evolve_interval: usize,
    pub importance_threshold: f64,
}

impl AppConfig {
    /// 从 TOML 文件加载配置，应用环境变量覆盖
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = find_config_file();

        let config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let mut config: AppConfig = toml::from_str(&content)?;
            eprintln!("📄 配置文件: {}", config_path.display());

            // 应用环境变量覆盖
            apply_env_overrides(&mut config);

            config
        } else {
            eprintln!("⚠️  未找到配置文件，使用默认配置");
            default_config()
        };

        Ok(config)
    }
}

/// 查找配置文件
fn find_config_file() -> PathBuf {
    // 优先级：环境变量 > 当前目录 > 项目根目录
    if let Ok(path) = std::env::var("SUBHUTI_CONFIG") {
        return PathBuf::from(path);
    }

    let current = PathBuf::from("config/Subhuti.toml");
    if current.exists() {
        return current;
    }

    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/Subhuti.toml");
    if project.exists() {
        return project;
    }

    PathBuf::from("Subhuti.toml")
}

/// 应用环境变量覆盖
fn apply_env_overrides(config: &mut AppConfig) {
    // LLM 配置
    if let Ok(val) = std::env::var("LLM_PROVIDER") {
        config.llm.provider = val;
    }
    if let Ok(val) = std::env::var("LLM_MODEL") {
        config.llm.model = val;
    }
    if let Ok(val) = std::env::var("LLM_API_URL") {
        config.llm.api_url = val;
    }
    if let Ok(_val) = std::env::var("DOUBAO_API_KEY") {
        // API key 通过 .env 加载，不需要特殊处理
    }

    // 数据库配置
    if let Ok(val) = std::env::var("DB_HOST") {
        config.database.host = val;
    }
    if let Ok(val) = std::env::var("DB_PORT") {
        if let Ok(port) = val.parse() {
            config.database.port = port;
        }
    }
    if let Ok(val) = std::env::var("DB_DATABASE") {
        config.database.database = val;
    }
    if let Ok(val) = std::env::var("DB_USERNAME") {
        config.database.username = val;
    }
    if let Ok(val) = std::env::var("DB_PASSWORD") {
        config.database.password = val;
    }

    // HTTP 配置
    if let Ok(val) = std::env::var("HTTP_ADDR") {
        config.http.addr = val;
    }

    // 日志配置
    if let Ok(val) = std::env::var("RUST_LOG") {
        config.logging.level = val;
    }
}

/// 默认配置（当找不到配置文件时使用）
pub fn default_config() -> AppConfig {
    AppConfig {
        llm: LlmConfig {
            provider: "doubao".to_string(),
            model: "doubao-seed-2-0-lite-260215".to_string(),
            api_url: "https://ark.cn-beijing.volces.com/api/v3/responses".to_string(),
            temperature: 0.7,
            max_tokens: 2048,
        },
        database: DatabaseConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            username: "postgres".to_string(),
            password: "123456".to_string(),
            max_connections: 10,
        },
        http: HttpConfig {
            addr: "0.0.0.0:8080".to_string(),
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            dir: "./logs".to_string(),
        },
        memory: MemoryConfig {
            short_term_limit: 50,
            archive_threshold: 100,
        },
        soul: SoulConfig {
            evolve_interval: 50,
            importance_threshold: 0.6,
        },
    }
}
