use serde_json::Value;

pub(super) fn task_failure_log_payload(error: &str) -> Value {
    serde_json::json!({
        "status": "error",
        "error": error,
    })
}

#[cfg(test)]
mod tests {
    use super::task_failure_log_payload;

    #[test]
    fn task_failure_log_payload_preserves_error_reason() {
        let payload =
            task_failure_log_payload("step_04_translation failed: missing translation id 20");

        assert_eq!(payload["status"], "error");
        assert_eq!(
            payload["error"],
            "step_04_translation failed: missing translation id 20"
        );
    }
}
