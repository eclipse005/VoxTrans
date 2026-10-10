import { invoke } from "@tauri-apps/api/core";
import type { AlignModel, AsrModel, DemucsModel, ModelTarget } from "../../features/media/types";

type OpenTaskOutputDirRequest = {
  taskId: string;
  mediaPath: string;
};

type OpenTaskLogDirRequest = {
  taskId: string;
  mediaPath?: string;
};

export async function openTaskOutputDir(request: OpenTaskOutputDirRequest): Promise<void> {
  await invoke("open_task_output_dir", { request });
}

export async function openTaskLogDir(request: OpenTaskLogDirRequest): Promise<void> {
  await invoke("open_task_log_dir", { request });
}

/**
 * Open the app-wide error log directory (`<output root>/logs`). Not scoped to a
 * task, so it works even before anything has run — this is the folder to ask a
 * user for when they report a problem.
 */
export async function openAppLogDir(): Promise<void> {
  await invoke("open_app_log_dir");
}

/**
 * Health of the app-wide error log writer (`<output root>/logs/app.log`).
 *
 * The logger is deliberately silent and disables itself when the file cannot
 * be written (unwritable dir, locked file, full disk, failed rotation) — so an
 * empty log can mean either "no errors" or "logging is broken". This snapshot
 * is what lets the settings UI tell those two apart.
 */
export type AppLogHealth = {
  /** Absolute path of `app.log`. */
  path: string;
  /** Absolute path of the log directory. */
  directory: string;
  /** false = writes are being dropped for this process. */
  enabled: boolean;
  /** Why logging was disabled (present when `enabled` is false). */
  disabledReason: string | null;
  /** Whether the file handle is currently open. */
  fileOpen: boolean;
};

/**
 * Read-only diagnostics for the app-wide error log — cheap and side-effect
 * free, so the settings dialog can probe it on every open. Callers treat a
 * rejection as "unknown" (e.g. command not available yet) and stay quiet:
 * this must never surface as a user-facing error.
 */
export async function getAppLogHealth(): Promise<AppLogHealth> {
  return invoke<AppLogHealth>("get_app_log_health");
}

export async function openModelDir(target: ModelTarget, model: AsrModel | AlignModel | DemucsModel): Promise<void> {
  await invoke("open_model_dir", {
    request: { target, model },
  });
}

export async function listSystemFonts(): Promise<string[]> {
  return invoke<string[]>("list_system_fonts");
}

export async function openExternalUrl(url: string): Promise<void> {
  try {
    await invoke("open_external_url", { url });
  } catch (err) {
    console.error("failed to open external url:", url, err);
  }
}
