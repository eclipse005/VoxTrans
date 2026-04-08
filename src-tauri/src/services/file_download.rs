/// 通用文件下载器，支持断点续传、进度回调、取消控制。
///
/// 下载过程中文件保存为 `<target>.part`，完成后自动重命名。
use reqwest::header;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub struct DownloadOptions {
    pub url: String,
    pub target: PathBuf,
    pub timeout_secs: u64,
    pub user_agent: String,
    pub referer: Option<String>,
}

pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
}

pub struct DownloadResult {
    pub path: PathBuf,
}

pub trait DownloadCallback: Send {
    fn on_progress(&mut self, progress: &DownloadProgress);
    fn on_message(&mut self, message: &str);
}

/// 使用阻塞 reqwest 客户端下载单个文件，支持断点续传。
pub fn download_file<F: DownloadCallback>(
    opts: &DownloadOptions,
    cancel_flag: &Arc<AtomicBool>,
    callback: &mut F,
) -> Result<DownloadResult, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(opts.timeout_secs))
        .user_agent(&opts.user_agent)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let part_path = opts.target.with_extension(format!(
        "{}.part",
        opts.target
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("part")
    ));

    let existing_len = if opts.target.exists() {
        std::fs::metadata(&opts.target)
            .map(|m| m.len())
            .unwrap_or(0)
    } else if part_path.exists() {
        std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    if opts.target.exists() {
        callback.on_message("文件已存在，跳过下载");
        return Ok(DownloadResult {
            path: opts.target.clone(),
        });
    }

    let mut request = client.get(&opts.url).header(header::ACCEPT, "*/*");
    if let Some(referer) = &opts.referer {
        request = request.header(header::REFERER, referer);
    }

    // 断点续传：已有部分文件
    let mut downloaded_bytes = 0;
    let mut append_mode = false;
    if existing_len > 0 {
        request = request.header(header::RANGE, format!("bytes={}-", existing_len));
        append_mode = true;
        downloaded_bytes = existing_len;
    }

    let response = request.send().map_err(|e| format!("请求失败: {}", e))?;

    // 如果服务器不支持 Range 请求，删除部分文件重新下载
    let (mut response, mut downloaded_bytes) =
        if response.status() == reqwest::StatusCode::OK && existing_len > 0 {
            let _ = std::fs::remove_file(&part_path);
            append_mode = false;
            let new_resp = client
                .get(&opts.url)
                .header(header::ACCEPT, "*/*")
                .send()
                .map_err(|e| format!("请求失败: {}", e))?;
            (new_resp, 0)
        } else {
            (response, downloaded_bytes)
        };

    if !(response.status().is_success()
        || response.status() == reqwest::StatusCode::PARTIAL_CONTENT)
    {
        return Err(format!("下载失败: HTTP {}", response.status()));
    }

    // 获取总大小
    let total_bytes = response
        .content_length()
        .map(|len| len + downloaded_bytes)
        .unwrap_or(0)
        .max(downloaded_bytes);

    // 打开文件
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(append_mode)
        .write(!append_mode)
        .truncate(!append_mode)
        .open(&part_path)
        .map_err(|e| format!("打开临时文件失败: {}", e))?;

    let mut buf = [0_u8; 64 * 1024];
    let mut last_speed_mark = Instant::now();
    let mut last_speed_bytes = downloaded_bytes;

    callback.on_message("下载中");

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            callback.on_message("下载已取消");
            return Ok(DownloadResult { path: part_path });
        }

        let read = response.read(&mut buf).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }

        file.write_all(&buf[..read])
            .map_err(|e| format!("写入文件失败: {}", e))?;
        downloaded_bytes += read as u64;

        let elapsed = last_speed_mark.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            let speed = if elapsed > 0.0 {
                ((downloaded_bytes.saturating_sub(last_speed_bytes)) as f64 / elapsed).round()
                    as u64
            } else {
                0
            };
            last_speed_bytes = downloaded_bytes;
            last_speed_mark = Instant::now();

            callback.on_progress(&DownloadProgress {
                downloaded_bytes,
                total_bytes,
                speed_bytes_per_sec: speed,
            });
        }
    }

    drop(file);

    if total_bytes > 0 && downloaded_bytes < total_bytes {
        let _ = std::fs::remove_file(&part_path);
        return Err(format!(
            "下载不完整: 预期 {} 字节，实际 {} 字节",
            total_bytes, downloaded_bytes
        ));
    }

    // 重命名完成
    std::fs::rename(&part_path, &opts.target).map_err(|e| format!("重命名文件失败: {}", e))?;

    callback.on_message("下载完成");

    Ok(DownloadResult {
        path: opts.target.clone(),
    })
}
