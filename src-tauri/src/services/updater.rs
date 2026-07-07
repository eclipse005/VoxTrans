/// 更新服务：检测更新、下载并安装。
const BUILD_VARIANT: &str = if cfg!(feature = "cuda") { "cuda" } else { "cpu" };

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use dashmap::DashMap;
use tauri::Emitter;
use tauri::Manager;

use super::file_download::{DownloadCallback, DownloadOptions, DownloadProgress};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubReleaseInfo {
    #[serde(rename = "tag_name")]
    pub tag_name: String,
    pub name: String,
    #[serde(rename = "published_at")]
    pub published_at: String,
    pub body: String,
    #[serde(rename = "html_url")]
    pub html_url: String,
    pub assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubReleaseAsset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_name: String,
    pub published_at: String,
    pub notes: String,
    pub html_url: String,
    pub download_url: String,
    pub download_size: u64,
    pub has_update: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percent: f64,
    pub speed: f64,
}

static UPDATE_PROGRESS_SNAPSHOTS: OnceLock<DashMap<String, UpdateDownloadProgress>> =
    OnceLock::new();

static UPDATE_CANCEL_FLAGS: OnceLock<DashMap<String, Arc<AtomicBool>>> = OnceLock::new();

fn progress_snapshots() -> &'static DashMap<String, UpdateDownloadProgress> {
    UPDATE_PROGRESS_SNAPSHOTS.get_or_init(DashMap::new)
}

fn cancel_flags() -> &'static DashMap<String, Arc<AtomicBool>> {
    UPDATE_CANCEL_FLAGS.get_or_init(DashMap::new)
}

fn installer_filename_from_url(download_url: &str) -> &str {
    let tail = download_url
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .unwrap_or("");
    if tail.ends_with(".exe") && !tail.trim().is_empty() {
        tail
    } else {
        "VoxTrans_update.exe"
    }
}

pub fn request_cancel(task_id: &str) -> bool {
    if let Some(flags) = cancel_flags().get(task_id) {
        flags.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// 检查更新，带重试（0s, 2s, 4s）
pub async fn check_update(current_version: &str) -> Result<UpdateInfo, String> {
    let delays_ms = [0, 2000, 4000];
    let mut last_err: Option<String> = None;

    for delay in delays_ms {
        if delay > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
        }
        match try_check(current_version).await {
            Ok(result) => return Ok(result),
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| "Unknown error".to_string()))
}

async fn try_check(current_version: &str) -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get("https://api.github.com/repos/eclipse005/VoxTrans/releases/latest")
        .header("User-Agent", "VoxTrans-Updater")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to request GitHub API: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned error: {}", response.status()));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read response body: {}", e))?;

    if body.trim().is_empty() {
        return Err("GitHub API returned empty response".to_string());
    }

    let release: GitHubReleaseInfo =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse: {}", e))?;

    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = latest_version != current_version;

    let installer = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(".exe") && a.name.contains(BUILD_VARIANT))
        .ok_or(format!("Installer for current variant not found ({BUILD_VARIANT})"))?;

    Ok(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version,
        release_name: release.name,
        published_at: release.published_at,
        notes: release.body,
        html_url: release.html_url,
        download_url: installer.browser_download_url.clone(),
        download_size: installer.size,
        has_update,
    })
}

/// 下载并安装更新（阻塞调用，需 spawn_blocking）
pub fn download_and_install(
    download_url: String,
    task_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    cancel_flags().insert(task_id.clone(), cancel.clone());

    let temp_dir = std::env::temp_dir().join("voxtrans_update");
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp directory: {}", e))?;

    let installer_name = installer_filename_from_url(&download_url);
    let installer_path = temp_dir.join(Path::new(installer_name));

    struct Cb {
        app: tauri::AppHandle,
        task_id: String,
    }

    impl DownloadCallback for Cb {
        fn on_progress(&mut self, p: &DownloadProgress) {
            let pct = if p.total_bytes > 0 {
                (p.downloaded_bytes as f64 / p.total_bytes as f64) * 100.0
            } else {
                0.0
            };
            let prog = UpdateDownloadProgress {
                downloaded: p.downloaded_bytes,
                total: p.total_bytes,
                percent: pct,
                speed: p.speed_bytes_per_sec as f64,
            };
            progress_snapshots().insert(self.task_id.clone(), prog.clone());
            let _ = self
                .app
                .emit("update-download-progress", &(self.task_id.clone(), prog));
        }

        fn on_message(&mut self, _: &str) {}
    }

    let mut cb = Cb {
        app: app.clone(),
        task_id: task_id.clone(),
    };

    let opts = DownloadOptions {
        url: download_url,
        target: installer_path,
        timeout_secs: 600,
        user_agent: "VoxTrans-Updater".to_string(),
        referer: None,
    };

    let result = super::file_download::download_file(&opts, &cancel, &mut cb)?;
    if cancel.load(Ordering::SeqCst) {
        return Err("Update download cancelled".to_string());
    }

    std::process::Command::new(&result.path)
        .arg("/UPDATE")
        .arg("/P")
        .spawn()
        .map_err(|e| format!("Failed to launch installer: {}", e))?;

    app.exit(0);
    Ok(())
}

const SKIPPED_VERSION_FILE: &str = "skipped_version.txt";

pub fn save_skipped_version(app: &tauri::AppHandle, version: &str) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(SKIPPED_VERSION_FILE);
    std::fs::write(&path, version.trim()).map_err(|e| e.to_string())
}

pub fn load_skipped_version(app: &tauri::AppHandle) -> Option<String> {
    let dir = app.path().app_data_dir().ok()?;
    let path = dir.join(SKIPPED_VERSION_FILE);
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
