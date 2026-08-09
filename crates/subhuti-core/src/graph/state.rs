//! # 图状态管理
//!
//! 支持类型安全的 State Schema 和自定义 Reducer。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// 状态 Reducer：定义同名 key 的合并策略
pub type StateReducer = Arc<dyn Fn(&Value, &Value) -> Value + Send + Sync>;

/// 图状态容器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphState {
    /// 状态数据
    data: HashMap<String, Value>,
}

impl GraphState {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    /// 从 HashMap 创建
    pub fn from_map(data: HashMap<String, Value>) -> Self {
        Self { data }
    }

    /// 设置值
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.data.insert(key.into(), value.into());
    }

    /// 获取字符串值
    pub fn get(&self, key: &str) -> Option<String> {
        self.data
            .get(key)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    /// 获取原始 JSON 值
    pub fn get_value(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    /// 获取所有数据
    pub fn data(&self) -> &HashMap<String, Value> {
        &self.data
    }

    /// 合并另一个状态（默认策略：覆盖）
    pub fn merge(&mut self, other: HashMap<String, Value>) {
        for (k, v) in other {
            self.data.insert(k, v);
        }
    }

    /// 使用 Reducer 合并
    pub fn merge_with_reducers(
        &mut self,
        other: HashMap<String, Value>,
        reducers: &HashMap<String, StateReducer>,
    ) {
        for (k, v) in other {
            if let Some(reducer) = reducers.get(&k) {
                if let Some(existing) = self.data.get(&k) {
                    self.data.insert(k.clone(), reducer(existing, &v));
                } else {
                    self.data.insert(k, v);
                }
            } else {
                self.data.insert(k, v);
            }
        }
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// 转为 JSON 字符串
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(&self.data)
    }
}

impl Default for GraphState {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for GraphState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_json().unwrap_or_else(|_| "{}".into()))
    }
}

/// 内置 Reducer
pub mod reducers {
    use super::*;

    /// 追加策略：用于 Vec
    pub fn append() -> StateReducer {
        Arc::new(|old: &Value, new: &Value| {
            let mut result = match old {
                Value::Array(arr) => arr.clone(),
                _ => vec![old.clone()],
            };
            match new {
                Value::Array(arr) => result.extend(arr.iter().cloned()),
                _ => result.push(new.clone()),
            }
            Value::Array(result)
        })
    }

    /// 覆盖策略（默认）
    pub fn overwrite() -> StateReducer {
        Arc::new(|_old: &Value, new: &Value| new.clone())
    }

    /// 取最大值（用于计数器）
    pub fn max() -> StateReducer {
        Arc::new(|old: &Value, new: &Value| {
            let old_num = old.as_i64().or_else(|| old.as_f64().map(|f| f as i64));
            let new_num = new.as_i64().or_else(|| new.as_f64().map(|f| f as i64));
            match (old_num, new_num) {
                (Some(o), Some(n)) => Value::from(o.max(n)),
                _ => new.clone(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_basic() {
        let mut state = GraphState::new();
        state.set("input", "hello");
        state.set("count", 42);

        assert_eq!(state.get("input"), Some("hello".to_string()));
        assert_eq!(state.get_value("count").unwrap().as_i64(), Some(42));
    }

    #[test]
    fn test_state_merge() {
        let mut state = GraphState::new();
        state.set("a", 1);

        let mut updates = HashMap::new();
        updates.insert("a".to_string(), Value::from(2));
        updates.insert("b".to_string(), Value::from(3));

        state.merge(updates);

        assert_eq!(state.get_value("a").unwrap().as_i64(), Some(2));
        assert_eq!(state.get_value("b").unwrap().as_i64(), Some(3));
    }

    #[test]
    fn test_reducer_append() {
        let reducer = reducers::append();

        let old = Value::Array(vec![Value::from("a"), Value::from("b")]);
        let new = Value::Array(vec![Value::from("c")]);

        let result = reducer(&old, &new);
        assert_eq!(
            result,
            Value::Array(vec![Value::from("a"), Value::from("b"), Value::from("c"),])
        );
    }
}
