import { invoke } from "@tauri-apps/api/core";

type DownloadYoutubeTaskRequest = {
  url: string;
  taskId?: string;
};

export type DownloadYoutubeTaskResponse = {
  task: {
    id: string;
    mediaPath: string;
    name: string;
    mediaKind: "audio" | "video";
    sizeBytes: number;
  };
  outputDir: string;
  downloadedPath: string;
};

export type YoutubeDownloadProgressResponse = {
  taskId: string;
  phase: string;
  progressPercent: number;
  title: string;
  speed: string;
  totalSize: string;
  downloadedSize: string;
  eta: string;
  message: string;
};

export type UpdateYtDlpResponse = {
  fromVersion: string;
  toVersion: string;
  updated: boolean;
  output: string;
};

export async function downloadYoutubeTask(
  request: DownloadYoutubeTaskRequest,
): Promise<DownloadYoutubeTaskResponse> {
  return invoke<DownloadYoutubeTaskResponse>("download_youtube_to_task_run", { request });
}

export async function getYoutubeDownloadProgress(taskId: string): Promise<YoutubeDownloadProgressResponse> {
  return invoke<YoutubeDownloadProgressResponse>("get_youtube_download_progress", {
    request: { taskId },
  });
}

export async function listYoutubeDownloadProgress(): Promise<YoutubeDownloadProgressResponse[]> {
  return invoke<YoutubeDownloadProgressResponse[]>("list_youtube_download_progress");
}

export async function cancelYoutubeDownload(taskId: string): Promise<void> {
  await invoke("cancel_youtube_download", {
    request: { taskId },
  });
}

export async function getYtDlpVersion(): Promise<string> {
  return invoke<string>("get_yt_dlp_version");
}

export async function updateYtDlp(): Promise<UpdateYtDlpResponse> {
  return invoke<UpdateYtDlpResponse>("update_yt_dlp");
}
