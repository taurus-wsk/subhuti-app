//! # Tool Infrastructure
//!
//! 工具系统的具体实现，对接外部工具和服务。

use async_trait::async_trait;
use serde_json;
use std::sync::Arc;
use subhuti_core::runtime::tools::{Tool, ToolInfo, ToolResult};

pub struct CalculatorTool;

impl CalculatorTool {
    pub fn new() -> Self {
        Self
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for CalculatorTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "calculator".to_string(),
            description: "执行数学计算".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "数学表达式，如 '2 + 3 * 4'"
                    }
                },
                "required": ["expression"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let expression = args["expression"].as_str().unwrap_or("");
        match eval_expression(expression) {
            Ok(result) => ToolResult {
                success: true,
                result: serde_json::json!({ "result": result }),
                error: None,
            },
            Err(e) => ToolResult {
                success: false,
                result: serde_json::Value::Null,
                error: Some(e),
            },
        }
    }
}

fn eval_expression(expr: &str) -> Result<f64, String> {
    let expr = expr.replace(" ", "");
    let chars: Vec<char> = expr.chars().collect();
    let mut result = 0.0;
    let mut current_num = 0.0;
    let mut operator = '+';
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_digit(10) || c == '.' {
            let mut num_str = String::new();
            while i < chars.len() && (chars[i].is_digit(10) || chars[i] == '.') {
                num_str.push(chars[i]);
                i += 1;
            }
            current_num = num_str.parse().map_err(|_| "无效数字")?;
            continue;
        } else if "+-*/".contains(c) {
            result = apply_operator(result, current_num, operator)?;
            operator = c;
            current_num = 0.0;
        } else {
            return Err(format!("未知字符: {}", c));
        }
        i += 1;
    }

    Ok(apply_operator(result, current_num, operator)?)
}

fn apply_operator(a: f64, b: f64, op: char) -> Result<f64, String> {
    match op {
        '+' => Ok(a + b),
        '-' => Ok(a - b),
        '*' => Ok(a * b),
        '/' => {
            if b == 0.0 {
                Err("除数不能为零".to_string())
            } else {
                Ok(a / b)
            }
        }
        _ => Err(format!("未知运算符: {}", op)),
    }
}

pub struct WeatherTool {
    #[allow(dead_code)]
    api_key: Option<String>,
}

impl WeatherTool {
    pub fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }

    pub fn arc(api_key: Option<String>) -> Arc<Self> {
        Arc::new(Self::new(api_key))
    }
}

#[async_trait]
impl Tool for WeatherTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "weather".to_string(),
            description: "查询天气信息".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {
                        "type": "string",
                        "description": "城市名称，如 '北京'"
                    }
                },
                "required": ["city"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let city = args["city"].as_str().unwrap_or("");
        if city.is_empty() {
            return ToolResult {
                success: false,
                result: serde_json::Value::Null,
                error: Some("城市名称不能为空".to_string()),
            };
        }

        let mock_result = serde_json::json!({
            "city": city,
            "temperature": 25.0,
            "weather": "晴朗",
            "humidity": 60,
            "wind_speed": 10.0,
            "description": format!("{}今天天气晴朗，温度25度，适合出行", city)
        });

        ToolResult {
            success: true,
            result: mock_result,
            error: None,
        }
    }
}

pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self {
        Self
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "web_search".to_string(),
            description: "执行网页搜索".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return ToolResult {
                success: false,
                result: serde_json::Value::Null,
                error: Some("搜索关键词不能为空".to_string()),
            };
        }

        let mock_results = serde_json::json!({
            "query": query,
            "results": [
                {
                    "title": "搜索结果1",
                    "url": "https://example.com/1",
                    "snippet": "这是关于'".to_string() + query + "'的搜索结果摘要1..."
                },
                {
                    "title": "搜索结果2",
                    "url": "https://example.com/2",
                    "snippet": "这是关于'".to_string() + query + "'的搜索结果摘要2..."
                }
            ]
        });

        ToolResult {
            success: true,
            result: mock_results,
            error: None,
        }
    }
}

pub struct FileReadTool;

impl FileReadTool {
    pub fn new() -> Self {
        Self
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "file_read".to_string(),
            description: "读取文件内容".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "文件路径"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("");
        match std::fs::read_to_string(path) {
            Ok(content) => ToolResult {
                success: true,
                result: serde_json::json!({ "content": content }),
                error: None,
            },
            Err(e) => ToolResult {
                success: false,
                result: serde_json::Value::Null,
                error: Some(e.to_string()),
            },
        }
    }
}

pub struct FileWriteTool;

impl FileWriteTool {
    pub fn new() -> Self {
        Self
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "file_write".to_string(),
            description: "写入文件内容".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "文件路径"
                    },
                    "content": {
                        "type": "string",
                        "description": "文件内容"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        match std::fs::write(path, content) {
            Ok(_) => ToolResult {
                success: true,
                result: serde_json::json!({ "path": path, "written": content.len() }),
                error: None,
            },
            Err(e) => ToolResult {
                success: false,
                result: serde_json::Value::Null,
                error: Some(e.to_string()),
            },
        }
    }
}
