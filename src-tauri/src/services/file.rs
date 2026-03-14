use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSrtRequest {
    pub output_path: String,
    pub content: String,
}

pub fn save_srt(request: SaveSrtRequest) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&request.output_path).parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(&request.output_path, request.content).map_err(|err| err.to_string())
}

pub fn get_file_size(path: String) -> Result<u64, String> {
    let metadata = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    Ok(metadata.len())
}
