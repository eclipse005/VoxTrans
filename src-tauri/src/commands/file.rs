use tauri::State;
use serde::Deserialize;

use crate::app_state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSrtCommandRequest {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub media_path: Option<String>,
    pub output_path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSrtCommandRequest {
    pub task_id: String,
    pub target_dir: String,
    #[serde(default)]
    pub task_name: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportTaskSrtItem {
    Source,
    Target,
    BilingualSourceFirst,
    BilingualTargetFirst,
}

impl From<ExportTaskSrtItem> for crate::services::file::ExportSrtItem {
    fn from(value: ExportTaskSrtItem) -> Self {
        match value {
            ExportTaskSrtItem::Source => Self::Source,
            ExportTaskSrtItem::Target => Self::Target,
            ExportTaskSrtItem::BilingualSourceFirst => Self::BilingualSourceFirst,
            ExportTaskSrtItem::BilingualTargetFirst => Self::BilingualTargetFirst,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTaskSrtsCommandRequest {
    pub task_id: String,
    pub target_dir: String,
    #[serde(default)]
    pub task_name: Option<String>,
    pub items: Vec<ExportTaskSrtItem>,
}

#[tauri::command]
pub fn save_srt(request: SaveSrtCommandRequest) -> Result<(), String> {
    crate::services::file::save_srt(crate::services::file::SaveSrtRequest {
        task_id: request.task_id,
        media_path: request.media_path,
        output_path: request.output_path,
        content: request.content,
    })
}

#[tauri::command]
pub fn export_srt(request: ExportSrtCommandRequest) -> Result<String, String> {
    crate::services::file::export_srt(crate::services::file::ExportSrtRequest {
        task_id: request.task_id,
        target_dir: request.target_dir,
        task_name: request.task_name,
        content: request.content,
    })
}

#[tauri::command]
pub async fn export_task_srts(
    state: State<'_, AppState>,
    request: ExportTaskSrtsCommandRequest,
) -> Result<Vec<String>, String> {
    crate::services::file::export_task_srts(
        &state.pool,
        crate::services::file::ExportTaskSrtsRequest {
            task_id: request.task_id,
            target_dir: request.target_dir,
            task_name: request.task_name,
            items: request.items.into_iter().map(Into::into).collect(),
        },
    )
    .await
}

#[tauri::command]
pub fn get_file_size(path: String) -> Result<u64, String> {
    crate::services::file::get_file_size(path)
}
