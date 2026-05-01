use crate::services::task_log::{TaskLogger, event as task_log_event};

use super::get_task_record;
use crate::commands::workspace::log_payload::task_failure_log_payload;

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
