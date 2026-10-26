/// 更新服务：经 Cloudflare 更新网关检测、下载并安装。
///
/// 默认走 Workers 网关（国内加速）。检测失败时可回退 GitHub API 拿版本元数据，
/// 但下载 URL 仍优先改写为网关地址，避免直连 github.com releases。
use crate::services::BUILD_VARIANT;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use tauri::Manager;

use super::file_download::{DownloadCallback, DownloadOptions, DownloadProgress};

/// Production Cloudflare Workers update gateway (no trailing slash).
/// Compile-time override: `VOXTRANS_UPDATE_BASE` (CI/dev mirrors).
const DEFAULT_UPDATE_BASE: &str = "https://voxtrans-updates.277141767.workers.dev";

/// GitHub releases/latest — metadata fallback only when the gateway check fails.
const GITHUB_RELEASES_LATEST: &str =
    "https://api.github.com/repos/eclipse005/VoxTrans/releases/latest";

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
struct UpdateDownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percent: f64,
    pub speed: f64,
}

static UPDATE_CANCEL_FLAGS: OnceLock<DashMap<String, Arc<AtomicBool>>> = OnceLock::new();

fn cancel_flags() -> &'static DashMap<String, Arc<AtomicBool>> {
    UPDATE_CANCEL_FLAGS.get_or_init(DashMap::new)
}

/// Resolved update base URL (no trailing slash).
fn update_base_url() -> &'static str {
    static BASE: OnceLock<String> = OnceLock::new();
    BASE.get_or_init(|| {
        let raw = option_env!("VOXTRANS_UPDATE_BASE").unwrap_or(DEFAULT_UPDATE_BASE);
        raw.trim().trim_end_matches('/').to_string()
    })
    .as_str()
}

fn update_gateway_host() -> Result<&'static str, String> {
    static HOST: OnceLock<Result<String, String>> = OnceLock::new();
    match HOST.get_or_init(|| {
        let base = update_base_url();
        reqwest::Url::parse(base)
            .map_err(|e| format!("invalid update base URL `{base}`: {e}"))
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.to_string())
                    .ok_or_else(|| format!("update base URL has no host: `{base}`"))
            })
    }) {
        Ok(h) => Ok(h.as_str()),
        Err(e) => Err(e.clone()),
    }
}

fn latest_check_url() -> String {
    format!("{}/releases/latest", update_base_url())
}

fn gateway_download_url(tag_name: &str, asset_name: &str) -> String {
    // tag_name is kept as published (usually `v1.6.2`); asset_name is the exe filename.
    format!(
        "{}/download/{}/{}",
        update_base_url(),
        tag_name.trim_matches('/'),
        asset_name.trim_matches('/')
    )
}

fn url_host(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

fn is_github_download_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "github.com"
        || h == "objects.githubusercontent.com"
        || h.ends_with(".githubusercontent.com")
}

/// Ensure the download URL targets the configured update gateway.
///
/// Prefer gateway downloads for CN acceleration. If release JSON still points at
/// GitHub, rewrite to `{base}/download/{tag}/{asset}` using tag + asset name.
/// Reject any other unexpected download host.
fn normalize_download_url(
    raw_url: &str,
    tag_name: &str,
    asset_name: &str,
) -> Result<String, String> {
    let gateway_host = update_gateway_host()?;
    let preferred = gateway_download_url(tag_name, asset_name);

    let Some(host) = url_host(raw_url) else {
        // Unparseable — still prefer constructed gateway URL when tag/asset known.
        return Ok(preferred);
    };

    if host.eq_ignore_ascii_case(gateway_host) {
        // Already on gateway; prefer the canonical path form for consistency.
        return Ok(preferred);
    }

    if is_github_download_host(&host) {
        // Fallback check / stale worker rewrite: keep version metadata from GitHub,
        // but download via gateway so CN users still get accelerated assets.
        return Ok(preferred);
    }

    Err(format!(
        "refusing unexpected update download host `{host}` (expected `{gateway_host}` or GitHub); asset={asset_name}"
    ))
}

/// Defense-in-depth: downloads must hit the configured gateway host only.
/// If a GitHub release URL slipped through, rewrite from path segments.
pub fn ensure_gateway_download_url(download_url: &str) -> Result<String, String> {
    let gateway_host = update_gateway_host()?;
    let parsed =
        reqwest::Url::parse(download_url).map_err(|e| format!("invalid download URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "download URL has no host".to_string())?;

    if host.eq_ignore_ascii_case(gateway_host) {
        return Ok(download_url.to_string());
    }

    if is_github_download_host(host) {
        // Expect .../releases/download/{tag}/{name} or rewrite via last two segments.
        let segs: Vec<&str> = parsed
            .path_segments()
            .map(|s| s.filter(|p| !p.is_empty()).collect())
            .unwrap_or_default();
        if let Some(rel_idx) = segs.iter().position(|s| *s == "download") {
            // github: owner/repo/releases/download/tag/name
            if segs.len() >= rel_idx + 3 {
                let tag = segs[rel_idx + 1];
                let name = segs[rel_idx + 2];
                return Ok(gateway_download_url(tag, name));
            }
        }
        if segs.len() >= 2 {
            let tag = segs[segs.len() - 2];
            let name = segs[segs.len() - 1];
            if name.ends_with(".exe") {
                return Ok(gateway_download_url(tag, name));
            }
        }
        return Err(format!(
            "cannot rewrite GitHub download URL to gateway: {download_url}"
        ));
    }

    Err(format!(
        "refusing download from unexpected host `{host}` (allowed: `{gateway_host}`)"
    ))
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

/// 检查更新，带重试（0s, 2s, 4s）。默认请求网关；全部失败后可选一次 GitHub 元数据回退。
pub async fn check_update(current_version: &str) -> Result<UpdateInfo, String> {
    let delays_ms = [0, 2000, 4000];
    let mut last_err: Option<String> = None;
    let gateway_url = latest_check_url();

    for delay in delays_ms {
        if delay > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
        }
        match try_check(current_version, &gateway_url).await {
            Ok(result) => return Ok(result),
            Err(e) => last_err = Some(e),
        }
    }

    // Gateway exhausted. One GitHub API fallback for version/notes only;
    // download URLs are still rewritten to the gateway (see normalize_download_url).
    match try_check(current_version, GITHUB_RELEASES_LATEST).await {
        Ok(result) => Ok(result),
        Err(fallback_err) => Err(format!(
            "update gateway failed after retries ({}); GitHub fallback also failed: {}",
            last_err.unwrap_or_else(|| "unknown".to_string()),
            fallback_err
        )),
    }
}

async fn try_check(current_version: &str, check_url: &str) -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(check_url)
        .header("User-Agent", "VoxTrans-Updater")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to request update check ({check_url}): {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Update check returned error {}: {}",
            response.status(),
            check_url
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read response body: {}", e))?;

    if body.trim().is_empty() {
        return Err("Update check returned empty response".to_string());
    }

    let release: GitHubReleaseInfo =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse: {}", e))?;

    release_to_update_info(current_version, release)
}

fn release_to_update_info(
    current_version: &str,
    release: GitHubReleaseInfo,
) -> Result<UpdateInfo, String> {
    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = latest_version != current_version;

    let installer = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(".exe") && a.name.contains(BUILD_VARIANT))
        .ok_or_else(|| format!("Installer for current variant not found ({BUILD_VARIANT})"))?;

    let download_url = normalize_download_url(
        &installer.browser_download_url,
        &release.tag_name,
        &installer.name,
    )?;

    Ok(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version,
        release_name: release.name,
        published_at: release.published_at,
        notes: release.body,
        html_url: release.html_url,
        download_url,
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
    let download_url = ensure_gateway_download_url(&download_url)?;

    let cancel = Arc::new(AtomicBool::new(false));
    cancel_flags().insert(task_id.clone(), cancel.clone());

    let temp_dir = std::env::temp_dir().join("voxtrans_update");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_is_workers_gateway() {
        assert_eq!(update_base_url(), DEFAULT_UPDATE_BASE);
        assert_eq!(
            update_gateway_host().unwrap(),
            "voxtrans-updates.277141767.workers.dev"
        );
        assert_eq!(
            latest_check_url(),
            "https://voxtrans-updates.277141767.workers.dev/releases/latest"
        );
    }

    #[test]
    fn rewrite_github_asset_url_to_gateway() {
        let raw = "https://github.com/eclipse005/VoxTrans/releases/download/v1.6.2/VoxTrans_1.6.2_cpu.exe";
        let got = normalize_download_url(raw, "v1.6.2", "VoxTrans_1.6.2_cpu.exe").unwrap();
        assert_eq!(
            got,
            "https://voxtrans-updates.277141767.workers.dev/download/v1.6.2/VoxTrans_1.6.2_cpu.exe"
        );
    }

    #[test]
    fn accept_already_gateway_url() {
        let raw = "https://voxtrans-updates.277141767.workers.dev/download/v1.6.2/VoxTrans_1.6.2_cuda.exe";
        let got = normalize_download_url(raw, "v1.6.2", "VoxTrans_1.6.2_cuda.exe").unwrap();
        assert_eq!(got, raw);
    }

    #[test]
    fn reject_unexpected_download_host() {
        let err = normalize_download_url(
            "https://evil.example/malware.exe",
            "v1.6.2",
            "VoxTrans_1.6.2_cpu.exe",
        )
        .unwrap_err();
        assert!(err.contains("unexpected update download host"), "{err}");
    }

    #[test]
    fn ensure_gateway_rewrites_github_release_path() {
        let raw = "https://github.com/eclipse005/VoxTrans/releases/download/v1.6.3/VoxTrans_1.6.3_cpu.exe";
        let got = ensure_gateway_download_url(raw).unwrap();
        assert_eq!(
            got,
            "https://voxtrans-updates.277141767.workers.dev/download/v1.6.3/VoxTrans_1.6.3_cpu.exe"
        );
    }

    #[test]
    fn ensure_gateway_allows_gateway_host() {
        let raw =
            "https://voxtrans-updates.277141767.workers.dev/download/v1.6.2/VoxTrans_1.6.2_cpu.exe";
        assert_eq!(ensure_gateway_download_url(raw).unwrap(), raw);
    }

    #[test]
    fn ensure_gateway_rejects_random_host() {
        let err =
            ensure_gateway_download_url("https://cdn.example/VoxTrans_1.6.2_cpu.exe").unwrap_err();
        assert!(err.contains("unexpected host"), "{err}");
    }

    #[test]
    fn release_to_update_info_keeps_build_variant_and_gateway_url() {
        let release = GitHubReleaseInfo {
            tag_name: "v1.6.3".into(),
            name: "v1.6.3".into(),
            published_at: "2026-10-26T00:00:00Z".into(),
            body: "notes".into(),
            html_url: "https://github.com/eclipse005/VoxTrans/releases/tag/v1.6.3".into(),
            assets: vec![
                GitHubReleaseAsset {
                    name: "VoxTrans_1.6.3_cpu.exe".into(),
                    browser_download_url:
                        "https://github.com/eclipse005/VoxTrans/releases/download/v1.6.3/VoxTrans_1.6.3_cpu.exe"
                            .into(),
                    size: 10,
                },
                GitHubReleaseAsset {
                    name: "VoxTrans_1.6.3_cuda.exe".into(),
                    browser_download_url:
                        "https://github.com/eclipse005/VoxTrans/releases/download/v1.6.3/VoxTrans_1.6.3_cuda.exe"
                            .into(),
                    size: 20,
                },
            ],
        };
        let info = release_to_update_info("1.6.2", release).unwrap();
        assert!(info.has_update);
        assert_eq!(info.latest_version, "1.6.3");
        assert!(
            info.download_url.contains(BUILD_VARIANT),
            "variant {}: {}",
            BUILD_VARIANT,
            info.download_url
        );
        assert!(
            info.download_url
                .starts_with("https://voxtrans-updates.277141767.workers.dev/download/v1.6.3/"),
            "{}",
            info.download_url
        );
    }
}
