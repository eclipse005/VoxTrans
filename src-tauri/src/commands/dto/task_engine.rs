#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueTaskCommandRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub settings_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterTaskUploadCommandRequest {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskRunsCommandRequest {
    pub intent: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskRunCommandRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTasksCommandRequest {
    pub media_path: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskRunCommandRequest {
    pub task_id: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchItemCommand {
    pub task_id: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchCommandRequest {
    pub items: Vec<ExecuteTaskBatchItemCommand>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAndExecuteTaskBatchItemCommand {
    pub id: String,
    pub media_path: String,
    pub name: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub intent: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub settings_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAndExecuteTaskBatchCommandRequest {
    pub items: Vec<EnqueueAndExecuteTaskBatchItemCommand>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchFailureCommand {
    pub task_id: String,
    pub error: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskBatchCommandResponse {
    pub succeeded_task_ids: Vec<String>,
    pub failed: Vec<ExecuteTaskBatchFailureCommand>,
}

pub fn to_service_enqueue_task(
    request: EnqueueTaskCommandRequest,
) -> crate::services::task_engine::EnqueueTaskRequest {
    crate::services::task_engine::EnqueueTaskRequest {
        id: request.id,
        media_path: request.media_path,
        name: request.name,
        media_kind: request.media_kind,
        size_bytes: request.size_bytes,
        intent: request.intent,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        max_retries: request.max_retries,
        settings_snapshot: request.settings_snapshot,
    }
}

pub fn to_service_register_task_upload(
    request: RegisterTaskUploadCommandRequest,
) -> crate::services::task_engine::RegisterTaskUploadRequest {
    crate::services::task_engine::RegisterTaskUploadRequest {
        id: request.id,
        media_path: request.media_path,
        name: request.name,
        media_kind: request.media_kind,
        size_bytes: request.size_bytes,
    }
}

pub fn to_service_list_task_runs(
    request: ListTaskRunsCommandRequest,
) -> crate::services::task_engine::ListTaskRunsRequest {
    crate::services::task_engine::ListTaskRunsRequest {
        intent: request.intent,
        limit: request.limit,
    }
}

pub fn to_service_get_task_run(
    request: GetTaskRunCommandRequest,
) -> crate::services::task_engine::GetTaskRunRequest {
    crate::services::task_engine::GetTaskRunRequest {
        task_id: request.task_id,
    }
}

pub fn to_service_delete_tasks(
    request: DeleteTasksCommandRequest,
) -> crate::services::task_engine::DeleteTasksRequest {
    crate::services::task_engine::DeleteTasksRequest {
        media_path: request.media_path,
        task_id: request.task_id,
    }
}

pub fn to_service_execute_task_run(
    request: ExecuteTaskRunCommandRequest,
) -> crate::services::task_executor::ExecuteTaskRunRequest {
    crate::services::task_executor::ExecuteTaskRunRequest {
        task_id: request.task_id,
        intent: request.intent,
    }
}

pub fn to_service_execute_task_batch(
    request: ExecuteTaskBatchCommandRequest,
) -> crate::services::task_executor::ExecuteTaskBatchRequest {
    crate::services::task_executor::ExecuteTaskBatchRequest {
        items: request
            .items
            .into_iter()
            .map(
                |item| crate::services::task_executor::ExecuteTaskBatchItem {
                    task_id: item.task_id,
                    intent: item.intent,
                },
            )
            .collect(),
    }
}

pub fn to_service_enqueue_and_execute_task_batch(
    request: EnqueueAndExecuteTaskBatchCommandRequest,
) -> crate::services::task_executor::EnqueueAndExecuteTaskBatchRequest {
    crate::services::task_executor::EnqueueAndExecuteTaskBatchRequest {
        items: request
            .items
            .into_iter()
            .map(
                |item| crate::services::task_executor::EnqueueAndExecuteTaskBatchItem {
                    id: item.id,
                    media_path: item.media_path,
                    name: item.name,
                    media_kind: item.media_kind,
                    size_bytes: item.size_bytes,
                    intent: item.intent,
                    source_lang: item.source_lang,
                    target_lang: item.target_lang,
                    max_retries: item.max_retries,
                    settings_snapshot: item.settings_snapshot,
                },
            )
            .collect(),
    }
}

pub fn from_service_execute_batch_response(
    response: crate::services::task_executor::ExecuteTaskBatchResponse,
) -> ExecuteTaskBatchCommandResponse {
    ExecuteTaskBatchCommandResponse {
        succeeded_task_ids: response.succeeded_task_ids,
        failed: response
            .failed
            .into_iter()
            .map(|item| ExecuteTaskBatchFailureCommand {
                task_id: item.task_id,
                error: item.error,
            })
            .collect(),
    }
}
