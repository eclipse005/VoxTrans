use std::path::Path;

use crate::services::pipeline::StepSource;
use crate::services::task_log::{TaskLogger, event as task_log_event};

use super::{STEP_06_FINAL_CHECK_FILE, get_task_record};
use crate::commands::workspace::log_payload::{
    step6_final_check_log_payload, task_failure_log_payload,
};

pub(super) fn log_task_failure_to_main(task_id: &str, error: &str) {
    let payload = task_failure_log_payload(error);
    let logger = match get_task_record(task_id) {
        Ok(record) if !record.item.path.trim().is_empty() => {
            TaskLogger::main_with_media(task_id.to_string(), record.item.path)
        }
        _ => TaskLogger::main(task_id.to_string()),
    };
    logger.event(task_log_event::TASK_FAILED, Some(&payload));
}

pub(super) fn remove_step6_final_check_artifact(output_dir: &Path) {
    let _ = std::fs::remove_file(output_dir.join(STEP_06_FINAL_CHECK_FILE));
}

pub(super) fn log_step6_final_check_to_main(
    task_id: &str,
    media_path: &str,
    response: &crate::commands::translate::BuildStep6FinalCheckCommandResponse,
    source: StepSource,
) {
    let payload = step6_final_check_log_payload(response, source);
    let logger = if media_path.trim().is_empty() {
        TaskLogger::main(task_id.to_string())
    } else {
        TaskLogger::main_with_media(task_id.to_string(), media_path.to_string())
    };
    logger.event(task_log_event::TASK_FINAL_CHECK, Some(&payload));
}

pub(super) fn log_step6_final_check_error_to_main(task_id: &str, media_path: &str, error: &str) {
    let payload = serde_json::json!({
        "status": "check_failed",
        "artifact": STEP_06_FINAL_CHECK_FILE,
        "error": error,
    });
    let logger = if media_path.trim().is_empty() {
        TaskLogger::main(task_id.to_string())
    } else {
        TaskLogger::main_with_media(task_id.to_string(), media_path.to_string())
    };
    logger.event(task_log_event::TASK_FINAL_CHECK, Some(&payload));
}
