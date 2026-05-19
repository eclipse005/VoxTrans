import { invoke } from "@tauri-apps/api/core";
import type { UserPreferencesResponse } from "../../generated/bindings/UserPreferencesResponse";

export async function loadUserPreferences(): Promise<UserPreferencesResponse> {
  return invoke<UserPreferencesResponse>("load_user_preferences");
}
