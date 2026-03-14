import { invoke } from "@tauri-apps/api/core";
import type { SavedSettings } from "../../features/media/types";

export async function saveAppSettings(settings: SavedSettings): Promise<void> {
  await invoke("save_app_settings", {
    request: { settings },
  });
}
