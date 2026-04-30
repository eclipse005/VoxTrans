use std::path::Path;

pub(super) struct DownloadStart {
    pub(super) response: reqwest::blocking::Response,
    pub(super) restarted: bool,
}

pub(super) fn build_download_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) VoxTrans/0.1")
        .build()
        .map_err(|err| err.to_string())
}

pub(super) fn start_modelscope_download(
    client: &reqwest::blocking::Client,
    url: &str,
    part_path: &Path,
    part_bytes: u64,
) -> Result<DownloadStart, String> {
    let mut response = modelscope_request(client, url, part_bytes)
        .send()
        .map_err(|err| err.to_string())?;
    let mut restarted = false;
    if response.status() == reqwest::StatusCode::OK && part_bytes > 0 {
        let _ = std::fs::remove_file(part_path);
        restarted = true;
        response = modelscope_request(client, url, 0)
            .send()
            .map_err(|err| err.to_string())?;
    }
    Ok(DownloadStart {
        response,
        restarted,
    })
}

pub(super) fn is_download_success_status(status: reqwest::StatusCode) -> bool {
    status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT
}

fn modelscope_request(
    client: &reqwest::blocking::Client,
    url: &str,
    part_bytes: u64,
) -> reqwest::blocking::RequestBuilder {
    let request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "*/*")
        .header(reqwest::header::REFERER, "https://modelscope.cn/");
    if part_bytes > 0 {
        request.header(reqwest::header::RANGE, format!("bytes={}-", part_bytes))
    } else {
        request
    }
}
