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
    expect(profiles.map((p) => p.id)).toContain("hunyuan");
    expect(profiles.map((p) => p.id)).toContain("openrouter");
    // Non-Messages vendors and the official Claude slot stay out of the
    // domestic-first catalog; qwen was dropped (workspace-scoped gateway).
    expect(profiles.map((p) => p.id)).not.toContain("qwen");
    expect(profiles.map((p) => p.id)).not.toContain("chatgpt");
    expect(profiles.map((p) => p.id)).not.toContain("gemini");
    expect(profiles.map((p) => p.id)).not.toContain("ollama");
    expect(profiles.map((p) => p.id)).not.toContain("anthropic");
    expect(profiles.every((p) => p.id === p.presetId)).toBe(true);
  });

  it("selectProvider switches active without wiping other keys", () => {
    let profiles = createDefaultProfiles();
    profiles = updateActiveProfile(profiles, "deepseek", { apiKey: "ds-key" });
    const switched = selectProvider(profiles, "moonshot");
    profiles = updateActiveProfile(switched.profiles, switched.activeLlmProfileId, {
      apiKey: "kimi-key",
      baseUrl: "https://proxy.example/v1",
    });

    expect(switched.activeLlmProfileId).toBe("moonshot");
    expect(getActiveProfile(profiles, "moonshot").apiKey).toBe("kimi-key");

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
    expect(fixed.profiles.length).toBeGreaterThanOrEqual(7);
    expect(fixed.profiles.some((p) => p.id === "deepseek")).toBe(true);
    expect(fixed.profiles.find((p) => p.id === "custom")?.apiKey).toBe("k");
    expect(fixed.activeLlmProfileId).toBe("deepseek");
  });

  it("vendor slots start unconfigured until a key is set", () => {
    const profiles = createDefaultProfiles();
    expect(isProfileConfigured(profiles.find((p) => p.id === "deepseek")!)).toBe(false);
    expect(isProfileConfigured(profiles.find((p) => p.id === "hunyuan")!)).toBe(false);
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
    // OpenAI endpoints have no Anthropic catalog slot — they land on custom.
    expect(fixed.activeLlmProfileId).toBe("custom");
    const slot = fixed.profiles.find((p) => p.id === "custom")!;
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
    expect(fixed.activeLlmProfileId).toBe("custom");
    expect(fixed.profiles.find((p) => p.id === "custom")?.apiKey).toBe("k");
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
