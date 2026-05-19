import { invoke } from "@tauri-apps/api/core";
import type { SaveAppSettingsRequest } from "../../generated/bindings/SaveAppSettingsRequest";
import type { SavedSettings } from "../../generated/bindings/SavedSettings";

export async function saveAppSettings(settings: SavedSettings): Promise<void> {
  const request: SaveAppSettingsRequest = { settings };
  await invoke("save_app_settings", {
    request,
  });
}

type TestTranslateLlmRequest = {
  apiKey: string;
  baseUrl: string;
  model: string;
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
