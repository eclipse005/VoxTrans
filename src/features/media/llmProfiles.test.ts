import { describe, expect, it } from "vitest";
import {
  createDefaultProfiles,
  ensureProfiles,
  flattenActiveToTranslateFields,
  getActiveProfile,
  isProfileAtPresetDefaults,
  isProfileConfigured,
  resetProfileToPreset,
  selectProvider,
  updateActiveProfile,
} from "./llmProfiles";
import { getProviderById } from "./llmProviders";

describe("llmProfiles", () => {
  it("createDefaultProfiles covers every provider slot", () => {
    const profiles = createDefaultProfiles();
    expect(profiles.length).toBeGreaterThanOrEqual(8);
    expect(profiles.map((p) => p.id)).toContain("deepseek");
    expect(profiles.map((p) => p.id)).toContain("custom");
    expect(profiles.map((p) => p.id)).not.toContain("agnes");
    expect(profiles.map((p) => p.id)).not.toContain("zhipu");
    expect(profiles.every((p) => p.id === p.presetId)).toBe(true);
  });

  it("selectProvider switches active without wiping other keys", () => {
    let profiles = createDefaultProfiles();
    profiles = updateActiveProfile(profiles, "deepseek", { apiKey: "ds-key" });
    const switched = selectProvider(profiles, "qwen");
    profiles = updateActiveProfile(switched.profiles, switched.activeLlmProfileId, {
      apiKey: "qwen-key",
      baseUrl: "https://proxy.example/v1",
    });

    expect(switched.activeLlmProfileId).toBe("qwen");
    expect(getActiveProfile(profiles, "qwen").apiKey).toBe("qwen-key");

    const back = selectProvider(profiles, "deepseek");
    expect(getActiveProfile(back.profiles, back.activeLlmProfileId).apiKey).toBe("ds-key");
  });

  it("ensureProfiles fills missing slots and repairs active id", () => {
    const fixed = ensureProfiles(
      [
        {
          id: "custom",
          name: "自定义",
          baseUrl: "",
          apiKey: "k",
          model: "m",
          presetId: "custom",
          requiresKey: true,
        },
      ],
      "missing",
    );
    expect(fixed.profiles.length).toBeGreaterThanOrEqual(8);
    expect(fixed.profiles.some((p) => p.id === "deepseek")).toBe(true);
    expect(fixed.profiles.find((p) => p.id === "custom")?.apiKey).toBe("k");
    expect(fixed.activeLlmProfileId).toBe("deepseek");
  });

  it("ollama is configured without API key", () => {
    const profiles = createDefaultProfiles();
    const ollama = profiles.find((p) => p.id === "ollama")!;
    expect(isProfileConfigured(ollama)).toBe(true);
    expect(isProfileConfigured(profiles.find((p) => p.id === "deepseek")!)).toBe(false);
  });

  it("seeds legacy triple into matching profile and sets active", () => {
    const fixed = ensureProfiles([], "", {
      apiKey: "legacy-key",
      baseUrl: "https://api.deepseek.com/v1",
      model: "deepseek-v4-flash",
    });
    const ds = fixed.profiles.find((p) => p.id === "deepseek")!;
    expect(ds.apiKey).toBe("legacy-key");
    expect(ds.model).toBe("deepseek-v4-flash");
    expect(fixed.activeLlmProfileId).toBe("deepseek");
  });

  it("legacy non-deepseek URL seeds custom and activates custom", () => {
    const fixed = ensureProfiles([], "deepseek", {
      apiKey: "openai-key",
      baseUrl: "https://api.openai.com/v1",
      model: "gpt-5-mini",
    });
    // Exact match lands on chatgpt slot (catalog baseURL equals OpenAI).
    expect(fixed.activeLlmProfileId).toBe("chatgpt");
    const slot = fixed.profiles.find((p) => p.id === "chatgpt")!;
    expect(slot.apiKey).toBe("openai-key");
    const flat = flattenActiveToTranslateFields(fixed.profiles, fixed.activeLlmProfileId);
    expect(flat.translateApiKey).toBe("openai-key");
    expect(flat.translateBaseUrl).toBe("https://api.openai.com/v1");
  });

  it("legacy unknown URL seeds custom and activates it", () => {
    const fixed = ensureProfiles([], "deepseek", {
      apiKey: "proxy-key",
      baseUrl: "https://my-proxy.example/v1",
      model: "my-model",
    });
    expect(fixed.activeLlmProfileId).toBe("custom");
    const custom = fixed.profiles.find((p) => p.id === "custom")!;
    expect(custom.apiKey).toBe("proxy-key");
    expect(custom.baseUrl).toBe("https://my-proxy.example/v1");
    expect(custom.model).toBe("my-model");
  });

  it("does not overwrite free-form model on ensure", () => {
    let profiles = createDefaultProfiles();
    profiles = updateActiveProfile(profiles, "deepseek", {
      model: "deepseek-chat",
      baseUrl: "https://api.deepseek.com/v1",
    });
    const fixed = ensureProfiles(profiles, "deepseek");
    expect(fixed.profiles.find((p) => p.id === "deepseek")?.model).toBe("deepseek-chat");
  });

  it("legacy match ignores trailing slash on base URL", () => {
    const fixed = ensureProfiles([], "deepseek", {
      apiKey: "k",
      baseUrl: "https://api.openai.com/v1/",
      model: "gpt-5-mini",
    });
    expect(fixed.activeLlmProfileId).toBe("chatgpt");
    expect(fixed.profiles.find((p) => p.id === "chatgpt")?.apiKey).toBe("k");
  });

  it("resetProfileToPreset restores URL/model and keeps key", () => {
    let profiles = createDefaultProfiles();
    profiles = updateActiveProfile(profiles, "deepseek", {
      apiKey: "keep-me",
      baseUrl: "https://proxy.example/v1",
      model: "custom-model",
    });
    expect(isProfileAtPresetDefaults(getActiveProfile(profiles, "deepseek"))).toBe(false);

    profiles = resetProfileToPreset(profiles, "deepseek");
    const ds = getActiveProfile(profiles, "deepseek");
    const preset = getProviderById("deepseek");
    expect(ds.apiKey).toBe("keep-me");
    expect(ds.baseUrl).toBe(preset.baseURL);
    expect(ds.model).toBe(preset.model);
    expect(isProfileAtPresetDefaults(ds)).toBe(true);
  });
});
