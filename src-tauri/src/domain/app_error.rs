//! Application-level error type for Tauri commands outside the workspace
//! pipeline subtree.
//!
//! The workspace pipeline already has [`super::error::WorkspaceError`], which
//! serializes to a stable `{ code, message }` JSON payload. This module
//! provides the same shape for the remaining command families (system,
//! youtube, updater, model, translate, subtitle, demucs, file, download) so
//! the frontend can localize every backend error via the `code` field.
//!
//! All `#[error]` strings are English on purpose: the frontend owns
//! localization. Dynamic detail (e.g. an underlying OS error) is preserved in
//! the message for diagnostics.

use super::error::CommandErrorPayload;

/// Error categories for non-workspace commands.
///
/// Variants are deliberately coarse — each maps to a stable `code` string the
/// frontend uses as an i18n key suffix (`errors:code.<CODE>`). The inner
/// `String` carries English detail (often a wrapped lower-level message).
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("failed to read system fonts: {0}")]
    SystemFont(String),

    #[error("YouTube error: {0}")]
    Youtube(String),

    #[error("yt-dlp error: {0}")]
    YtDlp(String),

    #[error("updater error: {0}")]
    Updater(String),

    #[error("download error: {0}")]
    Download(String),

    #[error("model error: {0}")]
    Model(String),

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("subtitle error: {0}")]
    Subtitle(String),

    #[error("demucs error: {0}")]
    Demucs(String),

    #[error("file error: {0}")]
    File(String),

    /// Catch-all for command-level validation/rejections that don't fit a
    /// more specific category. Prefer a specific variant when possible.
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Stable error code used by the frontend as an i18n key suffix
    /// (`errors:code.<CODE>`).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::SystemFont(_) => "SYSTEM_FONT_FAILED",
            AppError::Youtube(_) => "YOUTUBE_ERROR",
            AppError::YtDlp(_) => "YTDLP_ERROR",
            AppError::Updater(_) => "UPDATER_ERROR",
            AppError::Download(_) => "DOWNLOAD_ERROR",
            AppError::Model(_) => "MODEL_ERROR",
            AppError::Llm(_) => "LLM_ERROR",
            AppError::Subtitle(_) => "SUBTITLE_ERROR",
            AppError::Demucs(_) => "DEMUCS_ERROR",
            AppError::File(_) => "FILE_ERROR",
            AppError::Other(_) => "APP_ERROR",
        }
    }

    /// Serialize to the same `{ code, message }` JSON shape as
    /// [`super::error::WorkspaceError::to_command_error`].
    pub fn to_command_error(&self) -> String {
        serde_json::to_string(&CommandErrorPayload {
            code: self.code(),
            message: self.to_string(),
        })
        .unwrap_or_else(|_| self.to_string())
    }
}

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        err.to_command_error()
    }
}

/// Convenience constructor: wrap any `Display` value as [`AppError::Other`].
impl From<String> for AppError {
    fn from(message: String) -> Self {
        AppError::Other(message)
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn app_error_serializes_stable_code_and_message() {
        let encoded = AppError::Youtube("link is empty".to_string()).to_command_error();
        let value: serde_json::Value = serde_json::from_str(&encoded).expect("json error");
        assert_eq!(value["code"], "YOUTUBE_ERROR");
        assert_eq!(value["message"], "YouTube error: link is empty");
    }

    #[test]
    fn app_error_converts_to_command_error_string() {
        let s: String = AppError::Updater("network".to_string()).into();
        let value: serde_json::Value = serde_json::from_str(&s).expect("json error");
        assert_eq!(value["code"], "UPDATER_ERROR");
    }
}
