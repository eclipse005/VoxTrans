use serde_json::Value;

use super::error::{LlmError, LlmErrorKind};

#[derive(Debug, Clone)]
pub struct JsonResponseValidator {
    pub required_top_level_keys: Vec<String>,
}

impl JsonResponseValidator {
    pub fn with_required_keys(keys: &[&str]) -> Self {
        Self {
            required_top_level_keys: keys.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    pub fn validate(&self, value: &Value) -> Result<(), LlmError> {
        let obj = value.as_object().ok_or_else(|| {
            LlmError::new(
                LlmErrorKind::InvalidSchema,
                "schema check failed: root JSON is not object",
            )
        })?;
        for key in &self.required_top_level_keys {
            if !obj.contains_key(key) {
                return Err(LlmError::new(
                    LlmErrorKind::InvalidSchema,
                    format!("schema check failed: missing key `{key}`"),
                ));
            }
        }
        Ok(())
    }

    pub fn describe_constraints(&self) -> String {
        if self.required_top_level_keys.is_empty() {
            return "Return one valid JSON value.".to_string();
        }
        format!(
            "Return one valid JSON object containing these top-level keys: {}.",
            self.required_top_level_keys
                .iter()
                .map(|key| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
