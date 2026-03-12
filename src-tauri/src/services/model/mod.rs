mod downloader;
mod path;
mod status;

use std::path::Path;

pub use downloader::{cancel_model_download, start_model_download};
pub use path::{open_model_dir, resolve_model_dir};
pub use status::{ModelStatusResponse, get_model_status};

pub(crate) const REQUIRED_MODEL_FILES: [&str; 4] = [
    "encoder-model.onnx",
    "encoder-model.onnx.data",
    "decoder_joint-model.onnx",
    "vocab.txt",
];

pub(crate) const MODEL_DOWNLOAD_FILES: [(&str, &str, u64); 5] = [
    (
        "decoder_joint-model.onnx",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/decoder_joint-model.onnx",
        35_790_000,
    ),
    (
        "encoder-model.onnx",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/encoder-model.onnx",
        41_770_000,
    ),
    (
        "encoder-model.onnx.data",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/encoder-model.onnx.data",
        2_440_000_000,
    ),
    (
        "vocab.txt",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/vocab.txt",
        9_380,
    ),
    (
        "config.json",
        "https://modelscope.cn/models/eclipse005/parakeet-tdt-0.6b-v2-onnx/resolve/master/config.json",
        97,
    ),
];

pub(crate) fn compute_model_download_bytes(model_dir: &Path) -> (u64, u64) {
    let mut downloaded_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    for (file_name, _url, expected_size) in MODEL_DOWNLOAD_FILES {
        let target = model_dir.join(file_name);
        let part = model_dir.join(format!("{}.part", file_name));
        let current = if target.exists() {
            std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
        } else {
            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
        };
        downloaded_bytes = downloaded_bytes.saturating_add(current.min(expected_size));
        total_bytes = total_bytes.saturating_add(expected_size);
    }
    (downloaded_bytes, total_bytes)
}
