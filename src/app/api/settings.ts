import { invoke } from "@tauri-apps/api/core";
import type { SavedSettings } from "../../features/media/types";

export async function saveAppSettings(settings: SavedSettings): Promise<void> {
  await invoke("save_app_settings", {
    request: { settings },
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
