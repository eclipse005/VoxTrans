use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordTaskLlmUsageRequest {
    pub task_id: String,
    pub stage: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskLlmUsageSummaryRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLlmUsageBucket {
    pub stage: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLlmUsageSummary {
    pub task_id: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub buckets: Vec<TaskLlmUsageBucket>,
}

pub async fn record_task_llm_usage(
    pool: &SqlitePool,
    request: RecordTaskLlmUsageRequest,
) -> Result<(), String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request.stage.trim().is_empty() {
        return Err("stage is required".to_string());
    }

    sqlx::query(
        "INSERT INTO task_llm_usage (
          task_id, stage,
          prompt_tokens, completion_tokens, total_tokens,
          updated_at
        ) VALUES (?, ?, ?, ?, ?, strftime('%s','now'))
        ON CONFLICT(task_id, stage) DO UPDATE SET
          prompt_tokens = task_llm_usage.prompt_tokens + excluded.prompt_tokens,
          completion_tokens = task_llm_usage.completion_tokens + excluded.completion_tokens,
          total_tokens = task_llm_usage.total_tokens + excluded.total_tokens,
          updated_at = excluded.updated_at",
    )
    .bind(request.task_id.trim())
    .bind(request.stage.trim())
    .bind(request.prompt_tokens.max(0))
    .bind(request.completion_tokens.max(0))
    .bind(request.total_tokens.max(0))
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn get_task_llm_usage_summary(
    pool: &SqlitePool,
    request: GetTaskLlmUsageSummaryRequest,
) -> Result<TaskLlmUsageSummary, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    let task_id = request.task_id.trim().to_string();

    let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT stage, prompt_tokens, completion_tokens, total_tokens, updated_at
         FROM task_llm_usage
         WHERE task_id = ?
         ORDER BY updated_at DESC, stage ASC",
    )
    .bind(&task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let buckets = rows
        .into_iter()
        .map(
            |(stage, prompt_tokens, completion_tokens, total_tokens, updated_at)| TaskLlmUsageBucket {
                stage,
                prompt_tokens,
                completion_tokens,
                total_tokens,
                updated_at,
            },
        )
        .collect::<Vec<_>>();

    let mut prompt_tokens = 0_i64;
    let mut completion_tokens = 0_i64;
    let mut total_tokens = 0_i64;
    for bucket in &buckets {
        prompt_tokens += bucket.prompt_tokens.max(0);
        completion_tokens += bucket.completion_tokens.max(0);
        total_tokens += bucket.total_tokens.max(0);
    }

    Ok(TaskLlmUsageSummary {
        task_id,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        buckets,
    })
}
