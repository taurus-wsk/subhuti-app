use async_trait::async_trait;
use serde_json::{self, Value};
use std::sync::Arc;

use super::node::{NodeFn, NodeResult};
use super::state::GraphState;
use crate::Error;

#[async_trait]
pub trait INodeValidator: Send + Sync {
    async fn validate(&self, state: &GraphState, output: &NodeResult) -> Result<(), String>;
}

pub struct NodeFixRunner {
    max_fix_times: u32,
}

impl NodeFixRunner {
    pub fn new(max_fix_times: u32) -> Self {
        Self { max_fix_times }
    }

    pub async fn execute_with_fix(
        &self,
        node: &NodeFn,
        mut state: GraphState,
        validator: &Arc<dyn INodeValidator>,
    ) -> Result<NodeResult, Error> {
        let mut last_error = None;

        for attempt in 0..self.max_fix_times {
            let result = node.call(state.clone()).await;

            match validator.validate(&state, &result).await {
                Ok(_) => return Ok(result),
                Err(reason) => {
                    last_error = Some(reason.clone());
                    tracing::warn!(
                        "节点校验失败，第 {}/{} 次尝试，原因: {}",
                        attempt + 1,
                        self.max_fix_times,
                        reason
                    );

                    let fix_hint = serde_json::json!({
                        "attempt": attempt + 1,
                        "error": reason,
                        "max_attempts": self.max_fix_times,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });

                    state.set("fix_hint", fix_hint);
                }
            }
        }

        Err(Error::FixFailed(last_error.unwrap_or_default()))
    }
}

pub struct JsonFormatValidator;

#[async_trait]
impl INodeValidator for JsonFormatValidator {
    async fn validate(&self, _state: &GraphState, output: &NodeResult) -> Result<(), String> {
        if !output.success {
            return Err(output
                .error
                .clone()
                .unwrap_or_else(|| "节点执行失败".to_string()));
        }

        if output.output.is_empty() {
            return Err("输出为空".to_string());
        }

        match serde_json::from_str::<Value>(&output.output) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("JSON 格式错误: {}", e)),
        }
    }
}

pub struct RequiredFieldsValidator {
    fields: Vec<String>,
}

impl RequiredFieldsValidator {
    pub fn new(fields: Vec<String>) -> Self {
        Self { fields }
    }
}

#[async_trait]
impl INodeValidator for RequiredFieldsValidator {
    async fn validate(&self, _state: &GraphState, output: &NodeResult) -> Result<(), String> {
        if !output.success {
            return Err(output
                .error
                .clone()
                .unwrap_or_else(|| "节点执行失败".to_string()));
        }

        if output.output.is_empty() {
            return Err("输出为空".to_string());
        }

        match serde_json::from_str::<Value>(&output.output) {
            Ok(Value::Object(obj)) => {
                let missing_fields: Vec<&str> = self
                    .fields
                    .iter()
                    .filter(|field| !obj.contains_key(field.as_str()))
                    .map(|s| s.as_str())
                    .collect();

                if missing_fields.is_empty() {
                    Ok(())
                } else {
                    Err(format!("缺少必填字段: {}", missing_fields.join(", ")))
                }
            }
            Ok(_) => Err("输出不是 JSON 对象".to_string()),
            Err(e) => Err(format!("JSON 格式错误: {}", e)),
        }
    }
}

pub struct CompositeValidator {
    validators: Vec<Arc<dyn INodeValidator>>,
}

impl CompositeValidator {
    pub fn new(validators: Vec<Arc<dyn INodeValidator>>) -> Self {
        Self { validators }
    }
}

#[async_trait]
impl INodeValidator for CompositeValidator {
    async fn validate(&self, state: &GraphState, output: &NodeResult) -> Result<(), String> {
        for (i, validator) in self.validators.iter().enumerate() {
            if let Err(e) = validator.validate(state, output).await {
                return Err(format!("校验器[{}]失败: {}", i, e));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_json_format_validator_valid() {
        let validator = JsonFormatValidator;
        let result = NodeResult::ok(r#"{"key": "value"}"#);
        assert!(validator
            .validate(&GraphState::new(), &result)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_json_format_validator_invalid() {
        let validator = JsonFormatValidator;
        let result = NodeResult::ok("not a json");
        assert!(validator
            .validate(&GraphState::new(), &result)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_required_fields_validator() {
        let validator = RequiredFieldsValidator::new(vec!["name".to_string(), "age".to_string()]);
        let result = NodeResult::ok(r#"{"name": "test", "age": 18}"#);
        assert!(validator
            .validate(&GraphState::new(), &result)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_required_fields_validator_missing() {
        let validator = RequiredFieldsValidator::new(vec!["name".to_string(), "age".to_string()]);
        let result = NodeResult::ok(r#"{"name": "test"}"#);
        let err = validator.validate(&GraphState::new(), &result).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("age"));
    }

    #[tokio::test]
    async fn test_composite_validator() {
        let validators: Vec<Arc<dyn INodeValidator>> = vec![
            Arc::new(JsonFormatValidator) as Arc<dyn INodeValidator>,
            Arc::new(RequiredFieldsValidator::new(vec!["id".to_string()]))
                as Arc<dyn INodeValidator>,
        ];
        let validator = CompositeValidator::new(validators);

        let result = NodeResult::ok(r#"{"id": "123"}"#);
        assert!(validator
            .validate(&GraphState::new(), &result)
            .await
            .is_ok());

        let invalid_result = NodeResult::ok(r#"{"name": "test"}"#);
        assert!(validator
            .validate(&GraphState::new(), &invalid_result)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_node_fix_runner_success() {
        let attempt_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempt_clone = attempt_count.clone();

        let node_fn = NodeFn::new(move |mut state| {
            let count = attempt_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if count == 0 {
                    state.set("fix_hint", serde_json::json!({"attempt": 1}));
                    NodeResult::ok("invalid json")
                } else {
                    NodeResult::ok(r#"{"valid": true}"#)
                }
            }
        });

        let runner = NodeFixRunner::new(2);
        let validator: Arc<dyn INodeValidator> = Arc::new(JsonFormatValidator);
        let result = runner
            .execute_with_fix(&node_fn, GraphState::new(), &validator)
            .await;

        assert!(result.is_ok());
        assert_eq!(attempt_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_node_fix_runner_max_attempts() {
        let node_fn = NodeFn::new(move |_state| async move { NodeResult::ok("invalid") });

        let runner = NodeFixRunner::new(2);
        let validator: Arc<dyn INodeValidator> = Arc::new(JsonFormatValidator);
        let result = runner
            .execute_with_fix(&node_fn, GraphState::new(), &validator)
            .await;

        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), Error::FixFailed(_)));
    }
}
