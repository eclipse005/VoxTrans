use serde::Deserialize;

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
pub fn get_file_size(path: String) -> Result<u64, String> {
    crate::services::file::get_file_size(path)
}
