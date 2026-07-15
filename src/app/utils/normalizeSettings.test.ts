import { describe, expect, it } from "vitest";
import { normalizeSettings } from "./normalizeSettings";
import type { SavedSettings } from "../../features/media/types";
import { createDefaultProfiles } from "../../features/media/llmProfiles";

// A non-empty group so normalizeTerminologyGroups is a no-op for pass-through.
const DEFAULT_GROUP = { id: "group-test", name: "默认", terms: [] };

function baseDefaults(): SavedSettings {
  return {
    provider: "cpu",
    chunkTargetSeconds: 60,
    subtitleLengthPreset: "standard",
    asrModel: "Qwen3-ASR-0.6B",
    alignModel: "mms-300m-1130-forced-aligner",
    demucsModel: "htdemucs_ft",
    enableVocalSeparation: false,
    translateApiKey: "",
    translateBaseUrl: "https://api.deepseek.com/v1",
    translateModel: "deepseek-v4-flash",
    llmProfiles: createDefaultProfiles(),
    activeLlmProfileId: "deepseek",
    llmConcurrency: 4,
    terminologyGroups: [DEFAULT_GROUP],
    activeTerminologyGroupId: "",
    enableSubtitleBeautify: true,
    enableClickSound: true,
    autoBurnHardSubtitle: false,
    subtitleBurnMode: "bilingualSourceFirst",
    subtitleRenderStyle: {
      source: {
        fontFamily: "Arial",
        fontSize: 44,
        primaryColor: "#FFFFFF",
        outlineColor: "#101010",
        backColor: "#000000",
        outline: 2.5,
        shadow: 1,
        borderStyle: "outline",
        borderOpacity: 88,
      },
      target: {
        fontFamily: "Microsoft YaHei",
        fontSize: 40,
        primaryColor: "#EAF6FF",
        outlineColor: "#101010",
        backColor: "#000000",
        outline: 2.5,
        shadow: 1,
        borderStyle: "outline",
        borderOpacity: 88,
      },
      layout: {
        marginV: 40,
        alignment: 2,
        bilingualLineGap: 10,
      },
    },
    flatSrtOutput: false,
    flatSrtItems: ["source", "target"],
    enableVisionAssist: false,
    locale: "zh-CN",
    modelsDir: null,
  };
}

describe("normalizeSettings", () => {
  it("passes through valid settings unchanged", () => {
    const defaults = baseDefaults();
    const profiles = createDefaultProfiles().map((p) =>
      p.id === "deepseek" ? { ...p, apiKey: "sk-abc" } : p,
    );
    const input: SavedSettings = {
      ...defaults,
      llmProfiles: profiles,
      activeLlmProfileId: "deepseek",
      translateApiKey: "sk-abc",
    };
    const result = normalizeSettings(input, defaults);
    expect(result).toEqual(input);
  });

  it("clamps chunkTargetSeconds to [30, 180]", () => {
    const defaults = baseDefaults();
    const tooLow = normalizeSettings({ ...defaults, chunkTargetSeconds: 10 }, defaults);
    expect(tooLow.chunkTargetSeconds).toBe(30);
    const mid = normalizeSettings({ ...defaults, chunkTargetSeconds: 120 }, defaults);
    expect(mid.chunkTargetSeconds).toBe(120);
    const tooHigh = normalizeSettings({ ...defaults, chunkTargetSeconds: 200 }, defaults);
    expect(tooHigh.chunkTargetSeconds).toBe(180);
  });

  it("clamps llmConcurrency to [1, 16]", () => {
    const defaults = baseDefaults();
    const tooLow = normalizeSettings({ ...defaults, llmConcurrency: 0 }, defaults);
    expect(tooLow.llmConcurrency).toBe(1);
    const tooHigh = normalizeSettings({ ...defaults, llmConcurrency: 100 }, defaults);
    expect(tooHigh.llmConcurrency).toBe(16);
  });

  it("mirrors active profile baseUrl (no stale denormalized fallback)", () => {
    const defaults = baseDefaults();
    const profiles = createDefaultProfiles().map((p) =>
      p.id === "deepseek"
        ? { ...p, baseUrl: "https://api.deepseek.com/v1", model: "deepseek-v4-flash", apiKey: "k" }
        : p,
    );
    const result = normalizeSettings(
      {
        ...defaults,
        llmProfiles: profiles,
        activeLlmProfileId: "deepseek",
        // Stale denormalized fields from another vendor must not win.
        translateBaseUrl: "https://other.example/v1",
        translateModel: "other-model",
        translateApiKey: "stale",
      },
      defaults,
    );
    expect(result.translateBaseUrl).toBe("https://api.deepseek.com/v1");
    expect(result.translateModel).toBe("deepseek-v4-flash");
    expect(result.translateApiKey).toBe("k");
  });

  it("keeps empty custom baseUrl without inventing deepseek endpoint", () => {
    const defaults = baseDefaults();
    const profiles = createDefaultProfiles().map((p) =>
      p.id === "custom" ? { ...p, baseUrl: "", model: "m", apiKey: "k" } : p,
    );
    const result = normalizeSettings(
      {
        ...defaults,
        llmProfiles: profiles,
        activeLlmProfileId: "custom",
        translateBaseUrl: "https://api.deepseek.com/v1",
        translateModel: "deepseek-v4-flash",
        translateApiKey: "stale",
      },
      defaults,
    );
    expect(result.translateBaseUrl).toBe("");
    expect(result.translateModel).toBe("m");
    expect(result.translateApiKey).toBe("k");
  });

  it("trims translateApiKey via active profile flatten", () => {
    const defaults = baseDefaults();
    const profiles = createDefaultProfiles().map((p) =>
      p.id === "deepseek" ? { ...p, apiKey: "  sk-abc  " } : p,
    );
    const result = normalizeSettings(
      { ...defaults, llmProfiles: profiles, activeLlmProfileId: "deepseek" },
      defaults,
    );
    expect(result.translateApiKey).toBe("sk-abc");
  });

  it("fills missing llm profile slots from catalog", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        llmProfiles: [
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
        activeLlmProfileId: "custom",
      },
      defaults,
    );
    expect(result.llmProfiles.some((p) => p.id === "deepseek")).toBe(true);
    expect(result.llmProfiles.find((p) => p.id === "custom")?.apiKey).toBe("k");
    expect(result.activeLlmProfileId).toBe("custom");
  });

  it("clamps subtitle font size to [16, 96]", () => {
    const defaults = baseDefaults();
    const tooSmall = normalizeSettings(
      {
        ...defaults,
        subtitleRenderStyle: {
          ...defaults.subtitleRenderStyle,
          source: {
            ...defaults.subtitleRenderStyle.source,
            fontSize: 8,
          },
        },
      },
      defaults,
    );
    expect(tooSmall.subtitleRenderStyle.source.fontSize).toBe(16);
    const tooLarge = normalizeSettings(
      {
        ...defaults,
        subtitleRenderStyle: {
          ...defaults.subtitleRenderStyle,
          source: {
            ...defaults.subtitleRenderStyle.source,
            fontSize: 200,
          },
        },
      },
      defaults,
    );
    expect(tooLarge.subtitleRenderStyle.source.fontSize).toBe(96);
  });

  it("normalizes hex colors to uppercase", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        subtitleRenderStyle: {
          ...defaults.subtitleRenderStyle,
          source: {
            ...defaults.subtitleRenderStyle.source,
            primaryColor: "#abcdef",
          },
        },
      },
      defaults,
    );
    expect(result.subtitleRenderStyle.source.primaryColor).toBe("#ABCDEF");
  });

  it("falls back to default color when hex is invalid", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        subtitleRenderStyle: {
          ...defaults.subtitleRenderStyle,
          source: {
            ...defaults.subtitleRenderStyle.source,
            primaryColor: "not-a-color",
          },
        },
      },
      defaults,
    );
    expect(result.subtitleRenderStyle.source.primaryColor).toBe("#FFFFFF");
  });

  it("dedupes flatSrtItems", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        flatSrtItems: ["source", "source", "target", "target"],
      },
      defaults,
    );
    expect(result.flatSrtItems).toEqual(["source", "target"]);
  });

  it("falls back to defaults when flatSrtItems is empty", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      { ...defaults, flatSrtItems: [] },
      defaults,
    );
    expect(result.flatSrtItems).toEqual(["source", "target"]);
  });

  it("clamps subtitle layout marginV to [0, 200]", () => {
    const defaults = baseDefaults();
    const tooHigh = normalizeSettings(
      {
        ...defaults,
        subtitleRenderStyle: {
          ...defaults.subtitleRenderStyle,
          layout: {
            ...defaults.subtitleRenderStyle.layout,
            marginV: 500,
          },
        },
      },
      defaults,
    );
    expect(tooHigh.subtitleRenderStyle.layout.marginV).toBe(200);
  });

  it("passes through enum fields without modification", () => {
    const defaults = baseDefaults();
    const input: SavedSettings = {
      ...defaults,
      provider: "cuda",
      subtitleLengthPreset: "loose",
      subtitleBurnMode: "target",
      asrModel: "Qwen3-ASR-1.7B",
    };
    const result = normalizeSettings(input, defaults);
    expect(result.provider).toBe("cuda");
    expect(result.subtitleLengthPreset).toBe("loose");
    expect(result.subtitleBurnMode).toBe("target");
    expect(result.asrModel).toBe("Qwen3-ASR-1.7B");
  });

  it("falls back font family to default when empty", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        subtitleRenderStyle: {
          ...defaults.subtitleRenderStyle,
          source: {
            ...defaults.subtitleRenderStyle.source,
            fontFamily: "   ",
          },
        },
      },
      defaults,
    );
    expect(result.subtitleRenderStyle.source.fontFamily).toBe("Arial");
  });

  it("normalizes empty terminologyGroups into a default group", () => {
    // A DB row with no groups (or a malformed empty array) must not render
    // an empty terminology UI; normalize fills in the default group, same
    // as the save path already does.
    const defaults = baseDefaults();
    const result = normalizeSettings(
      { ...defaults, terminologyGroups: [] },
      defaults,
    );
    expect(result.terminologyGroups).toHaveLength(1);
    expect(result.terminologyGroups[0].name).toBe("Default");
  });

  // --- Edge cases / defensive normalization ---
  // The tests below mirror the real load path: an unvalidated `invoke()`
  // response (or a form draft) that may carry undefined / wrong-typed /
  // illegal values. They prove the normalizer actually defends against bad
  // input rather than only passing well-typed data through untouched.

  it("falls back unknown enum values to defaults", () => {
    const defaults = baseDefaults();
    // Cast to simulate a DB row that predates the enum validation (e.g. a
    // stale "tpu" provider or a bogus preset from an older build).
    const result = normalizeSettings(
      {
        ...defaults,
        provider: "tpu" as SavedSettings["provider"],
        subtitleLengthPreset: "extra" as SavedSettings["subtitleLengthPreset"],
        subtitleBurnMode: "nope" as SavedSettings["subtitleBurnMode"],
      },
      defaults,
    );
    expect(result.provider).toBe("cpu");
    expect(result.subtitleLengthPreset).toBe("standard");
    expect(result.subtitleBurnMode).toBe("bilingualSourceFirst");
  });

  it("drops invalid flatSrtItems entries and keeps valid ones", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        flatSrtItems: [
          "source",
          "bogus",
          "target",
          "also-bogus",
        ] as unknown as SavedSettings["flatSrtItems"],
      },
      defaults,
    );
    expect(result.flatSrtItems).toEqual(["source", "target"]);
  });

  it("falls back to defaults when every flatSrtItems entry is invalid", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        flatSrtItems: ["bogus", "more-bogus"] as unknown as SavedSettings["flatSrtItems"],
      },
      defaults,
    );
    expect(result.flatSrtItems).toEqual(["source", "target"]);
  });

  it("coerces string/non-finite chunkTargetSeconds via fallback", () => {
    const defaults = baseDefaults();
    const fromString = normalizeSettings(
      { ...defaults, chunkTargetSeconds: "forty" as unknown as number },
      defaults,
    );
    expect(fromString.chunkTargetSeconds).toBe(60);
    const fromUndefined = normalizeSettings(
      { ...defaults, chunkTargetSeconds: undefined as unknown as number },
      defaults,
    );
    expect(fromUndefined.chunkTargetSeconds).toBe(60);
  });

  it("defaults enableSubtitleBeautify/enableClickSound to true when missing", () => {
    const defaults = baseDefaults();
    // A row missing these fields entirely must resolve to ON, matching the
    // backend's `#[serde(default = "default_true")]`.
    const partial = {
      ...defaults,
      enableSubtitleBeautify: undefined,
      enableClickSound: undefined,
    } as unknown as SavedSettings;
    const result = normalizeSettings(partial, defaults);
    expect(result.enableSubtitleBeautify).toBe(true);
    expect(result.enableClickSound).toBe(true);
  });

  it("respects explicit false for enableSubtitleBeautify/enableClickSound", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      { ...defaults, enableSubtitleBeautify: false, enableClickSound: false },
      defaults,
    );
    expect(result.enableSubtitleBeautify).toBe(false);
    expect(result.enableClickSound).toBe(false);
  });

  it("falls back to default subtitleRenderStyle when missing or non-object", () => {
    const defaults = baseDefaults();
    const fromUndefined = normalizeSettings(
      { ...defaults, subtitleRenderStyle: undefined as unknown as SavedSettings["subtitleRenderStyle"] },
      defaults,
    );
    expect(fromUndefined.subtitleRenderStyle).toEqual(defaults.subtitleRenderStyle);
    const fromNull = normalizeSettings(
      { ...defaults, subtitleRenderStyle: null as unknown as SavedSettings["subtitleRenderStyle"] },
      defaults,
    );
    expect(fromNull.subtitleRenderStyle).toEqual(defaults.subtitleRenderStyle);
  });

  it("falls back layout fields when subtitleRenderStyle.layout is partial", () => {
    const defaults = baseDefaults();
    const result = normalizeSettings(
      {
        ...defaults,
        subtitleRenderStyle: {
          ...defaults.subtitleRenderStyle,
          layout: { marginV: 50 } as SavedSettings["subtitleRenderStyle"]["layout"],
        },
      },
      defaults,
    );
    // marginV preserved + clamped, alignment/gap fall back to defaults.
    expect(result.subtitleRenderStyle.layout.marginV).toBe(50);
    expect(result.subtitleRenderStyle.layout.alignment).toBe(2);
    expect(result.subtitleRenderStyle.layout.bilingualLineGap).toBe(10);
  });
});
