import type {
  SavedSettings,
  SubtitleBorderStyle,
  SubtitleLineStyle,
  SubtitleRenderStyle,
} from "../../features/media/types";
import { ASR_MODELS, DEFAULT_ALIGN_MODEL } from "../../features/media/modelCatalog";
import { ensureProfiles, flattenActiveToTranslateFields } from "../../features/media/llmProfiles";
import { normalizeTerminologyGroups } from "./terminology";

const PROVIDERS: readonly SavedSettings["provider"][] = ["cpu", "cuda"];
const ALIGN_MODELS: readonly SavedSettings["alignModel"][] = [DEFAULT_ALIGN_MODEL];
const DEMUCUS_MODELS: readonly SavedSettings["demucsModel"][] = ["htdemucs_ft"];
const SUBTITLE_LENGTH_PRESETS: readonly SavedSettings["subtitleLengthPreset"][] = [
  "short",
  "standard",
  "loose",
];
const SUBTITLE_BURN_MODES: readonly SavedSettings["subtitleBurnMode"][] = [
  "source",
  "target",
  "bilingualSourceFirst",
  "bilingualTargetFirst",
];
const BORDER_STYLES: readonly SubtitleBorderStyle[] = ["outline", "box"];
const VALID_ALIGNMENTS = [1, 2, 3];
const LOCALES: readonly SavedSettings["locale"][] = ["zh-CN", "en"];

function pickEnum<T>(value: unknown, allowed: readonly T[], fallback: T): T {
  if (allowed.includes(value as T)) return value as T;
  return fallback;
}

/**
 * Normalize raw SavedSettings (from DB or form input) against authoritative
 * defaults pulled from the backend. Clamps numeric ranges, trims strings,
 * validates enum/union fields, and fills missing/invalid fields with `defaults`.
 */
export function normalizeSettings(raw: SavedSettings, defaults: SavedSettings): SavedSettings {
  const ensured = ensureProfiles(
    raw.llmProfiles ?? defaults.llmProfiles,
    raw.activeLlmProfileId ?? defaults.activeLlmProfileId,
    {
      apiKey: raw.translateApiKey,
      baseUrl: raw.translateBaseUrl,
      model: raw.translateModel,
    },
  );
  const flat = flattenActiveToTranslateFields(ensured.profiles, ensured.activeLlmProfileId);

  return {
    provider: pickEnum(raw.provider, PROVIDERS, defaults.provider),
    chunkTargetSeconds: clampInt(raw.chunkTargetSeconds, 30, 60, defaults.chunkTargetSeconds),
    subtitleLengthPreset: pickEnum(
      raw.subtitleLengthPreset,
      SUBTITLE_LENGTH_PRESETS,
      defaults.subtitleLengthPreset,
    ),
    asrModel: pickEnum(raw.asrModel, ASR_MODELS, defaults.asrModel),
    alignModel: pickEnum(raw.alignModel, ALIGN_MODELS, defaults.alignModel),
    demucsModel: pickEnum(raw.demucsModel, DEMUCUS_MODELS, defaults.demucsModel),
    enableVocalSeparation: Boolean(raw.enableVocalSeparation),
    llmProfiles: ensured.profiles,
    activeLlmProfileId: ensured.activeLlmProfileId,
    // Strictly mirror the active profile — never splice key/url/model from
    // stale denormalized fields of a previous vendor (cross-profile mix).
    translateApiKey: flat.translateApiKey,
    translateBaseUrl: flat.translateBaseUrl,
    translateModel: flat.translateModel,
    llmConcurrency: clampInt(raw.llmConcurrency, 1, 16, defaults.llmConcurrency),
    terminologyGroups: normalizeTerminologyGroups(raw.terminologyGroups ?? []),
    activeTerminologyGroupId: String(raw.activeTerminologyGroupId ?? ""),
    enableSubtitleBeautify: raw.enableSubtitleBeautify ?? true,
    enableClickSound: raw.enableClickSound ?? true,
    autoBurnHardSubtitle: Boolean(raw.autoBurnHardSubtitle),
    subtitleBurnMode: pickEnum(raw.subtitleBurnMode, SUBTITLE_BURN_MODES, defaults.subtitleBurnMode),
    subtitleRenderStyle: normalizeSubtitleRenderStyle(raw.subtitleRenderStyle, defaults.subtitleRenderStyle),
    flatSrtOutput: Boolean(raw.flatSrtOutput),
    flatSrtItems: dedupeFlatSrtItems(raw.flatSrtItems, defaults.flatSrtItems),
    enableVisionAssist: Boolean(raw.enableVisionAssist),
    locale: pickEnum(raw.locale, LOCALES, defaults.locale),
    modelsDir: raw.modelsDir?.trim() || null,
  };
}

function clampInt(value: unknown, min: number, max: number, fallback: number): number {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.min(max, Math.round(n)));
}

function dedupeFlatSrtItems(
  items: SavedSettings["flatSrtItems"],
  fallback: SavedSettings["flatSrtItems"],
): SavedSettings["flatSrtItems"] {
  if (!Array.isArray(items)) return fallback;
  // Validate against the known enum: a DB row could hold a stale/invalid
  // string that the generated binding types as a valid union member at
  // compile time but isn't actually legal at runtime. Drop anything unknown.
  const validItems = items.filter(
    (item): item is SavedSettings["flatSrtItems"][number] =>
      (SUBTITLE_BURN_MODES as readonly string[]).includes(item as string),
  );
  if (validItems.length === 0) return fallback;
  const seen = new Set<SavedSettings["flatSrtItems"][number]>();
  const result: SavedSettings["flatSrtItems"] = [];
  for (const item of validItems) {
    if (seen.has(item)) continue;
    seen.add(item);
    result.push(item);
  }
  return result;
}

function normalizeSubtitleRenderStyle(
  raw: SubtitleRenderStyle | undefined,
  fallback: SubtitleRenderStyle,
): SubtitleRenderStyle {
  if (!raw || typeof raw !== "object") {
    return fallback;
  }
  return {
    source: normalizeSubtitleLineStyle(raw.source, fallback.source),
    target: normalizeSubtitleLineStyle(raw.target, fallback.target),
    layout: {
      marginV: clampInt(raw.layout?.marginV, 0, 200, fallback.layout.marginV),
      alignment: VALID_ALIGNMENTS.includes(raw.layout?.alignment) ? raw.layout.alignment : fallback.layout.alignment,
      bilingualLineGap: clampInt(raw.layout?.bilingualLineGap, 0, 140, fallback.layout.bilingualLineGap),
    },
  };
}

function normalizeSubtitleLineStyle(
  raw: SubtitleLineStyle | undefined,
  fallback: SubtitleLineStyle,
): SubtitleLineStyle {
  if (!raw || typeof raw !== "object") {
    return fallback;
  }
  return {
    fontFamily: String(raw.fontFamily ?? "").trim() || fallback.fontFamily,
    fontSize: clampInt(raw.fontSize, 16, 96, fallback.fontSize),
    primaryColor: normalizeHexColor(raw.primaryColor, fallback.primaryColor),
    outlineColor: normalizeHexColor(raw.outlineColor, fallback.outlineColor),
    backColor: normalizeHexColor(raw.backColor, fallback.backColor),
    // libass renders nothing at outline=0.0, so clamp to 0.1 minimum.
    outline: clampNumber(raw.outline, 0.1, 8, fallback.outline),
    shadow: clampNumber(raw.shadow, 0, 8, fallback.shadow),
    borderStyle: pickEnum(raw.borderStyle, BORDER_STYLES, fallback.borderStyle),
    borderOpacity: clampInt(raw.borderOpacity, 0, 100, fallback.borderOpacity),
  };
}

function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.min(max, n));
}

function normalizeHexColor(value: unknown, fallback: string): string {
  const s = String(value ?? "").trim();
  if (/^#[0-9a-fA-F]{6}$/.test(s)) {
    return s.toUpperCase();
  }
  return fallback;
}
