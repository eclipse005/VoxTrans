import { invoke } from "@tauri-apps/api/core";
import type { SaveAppSettingsRequest } from "../../generated/bindings/SaveAppSettingsRequest";
import type { SavedSettings } from "../../generated/bindings/SavedSettings";
import type { DefaultSettingsResponse } from "../../generated/bindings/DefaultSettingsResponse";

export async function saveAppSettings(settings: SavedSettings): Promise<void> {
  const request: SaveAppSettingsRequest = { settings };
  await invoke("save_app_settings", {
    request,
  });
}

export async function getDefaultSettings(): Promise<SavedSettings> {
  const response = await invoke<DefaultSettingsResponse>("get_default_settings");
  return response.settings;
}

type TestTranslateLlmRequest = {
  apiKey: string;
  baseUrl: string;
  model: string;
  enableVisionAssist: boolean;
};

type TestTranslateLlmResponse = {
  ok: boolean;
  message: string;
  model: string;
};

export async function testTranslateLlmConnection(
  request: TestTranslateLlmRequest,
): Promise<TestTranslateLlmResponse> {
  return invoke<TestTranslateLlmResponse>("test_translate_llm", { request });
}
