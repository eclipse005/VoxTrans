import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type UpdateCheckResult = {
  currentVersion: string;
  latestVersion: string;
  releaseName: string;
  publishedAt: string;
  notes: string;
  htmlUrl: string;
  downloadUrl: string;
  downloadSize: number;
  hasUpdate: boolean;
};

export type UpdateDownloadProgress = {
  downloaded: number;
  total: number;
  percent: number;
  speed: number;
};

export async function checkForUpdate(): Promise<UpdateCheckResult> {
  return await invoke<UpdateCheckResult>("check_update");
}

export async function downloadUpdate(
  downloadUrl: string,
  taskId: string,
): Promise<void> {
  return await invoke<void>("download_update", {
    request: {
      downloadUrl,
      taskId,
    },
  });
}

export async function cancelUpdate(taskId: string): Promise<boolean> {
  return await invoke<boolean>("cancel_update", { taskId });
}

export async function skipUpdateVersion(version: string): Promise<void> {
  return await invoke<void>("skip_update_version", { version });
}

export async function getSkippedVersion(): Promise<string | null> {
  return await invoke<string | null>("get_skipped_version");
}

export type UpdateProgressEvent = [string, UpdateDownloadProgress];

export async function onUpdateProgress(
  callback: (event: UpdateProgressEvent) => void,
): Promise<() => void> {
  const unlisten = await listen<UpdateProgressEvent>(
    "update-download-progress",
    (event) => callback(event.payload),
  );
  return unlisten;
}
