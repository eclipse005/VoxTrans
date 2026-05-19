import { useEffect } from "react";
import { loadUserPreferences } from "../api/preferences";
import { normalizeProvider } from "../../features/media/provider";
import type {
  AlignModel,
  AsrModel,
  DemucsModel,
  SubtitleLengthPreset,
  SubtitleBurnMode,
  SubtitleLineStyle,
  SubtitleRenderStyle,
  UserPreferencesResponse,
} from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import { normalizeTerminologyGroups } from "../utils/terminology";

type DispatchState = (action: AppAction) => void;

export function useAppPersistence(dispatch: DispatchState) {
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res: UserPreferencesResponse = await loadUserPreferences();
        if (cancelled) return;
        const provider = normalizeProvider(res.settings.provider);
        const chunkTargetSeconds = Number.isFinite(res.settings.chunkTargetSeconds)
          ? Math.max(30, Math.min(60, Math.round(res.settings.chunkTargetSeconds)))
          : 45;
        const subtitleLengthPreset = normalizeSubtitleLengthPreset(res.settings.subtitleLengthPreset);
        const asrModel = normalizeAsrModel(res.settings.asrModel);
        const alignModel = res.settings.alignModel === "Qwen3-ForcedAligner-0.6B"
          ? res.settings.alignModel as AlignModel
          : "Qwen3-ForcedAligner-0.6B";
        const demucsModel = res.settings.demucsModel === "htdemucs_ft"
          ? res.settings.demucsModel as DemucsModel
          : "htdemucs_ft";
        const enableVocalSeparation = Boolean(res.settings.enableVocalSeparation);
        const translateApiKey = String(res.settings.translateApiKey ?? "");
        const translateBaseUrl = String(res.settings.translateBaseUrl ?? "").trim()
          || "https://api.openai.com/v1";
        const translateModel = String(res.settings.translateModel ?? "").trim()
          || "gpt-4.1-mini";
        const llmConcurrencyRaw = Number.parseInt(String(res.settings.llmConcurrency ?? "4"), 10);
        const llmConcurrency = Number.isFinite(llmConcurrencyRaw)
          ? Math.max(1, Math.min(16, llmConcurrencyRaw))
          : 4;
        const terminologyGroupsRaw = Array.isArray(res.settings.terminologyGroups)
          ? res.settings.terminologyGroups
          : [];
        const terminologyGroups = normalizeTerminologyGroups(terminologyGroupsRaw);
        const enableTerminology = Boolean(res.settings.enableTerminology ?? true);
        const enableSubtitleBeautify = Boolean(res.settings.enableSubtitleBeautify ?? true);
        const enableClickSound = Boolean(res.settings.enableClickSound ?? true);
        const autoBurnHardSubtitle = Boolean(res.settings.autoBurnHardSubtitle ?? false);
        const subtitleBurnModeRaw = String(res.settings.subtitleBurnMode ?? "bilingualSourceFirst");
        const subtitleBurnMode: SubtitleBurnMode = subtitleBurnModeRaw === "source"
          || subtitleBurnModeRaw === "target"
          || subtitleBurnModeRaw === "bilingualSourceFirst"
          || subtitleBurnModeRaw === "bilingualTargetFirst"
          ? subtitleBurnModeRaw
          : "bilingualSourceFirst";
        const subtitleRenderStyleRaw = (res.settings.subtitleRenderStyle ?? {}) as Record<string, unknown>;
        const sourceRaw = subtitleRenderStyleRaw.source as Record<string, unknown> | undefined;
        const targetRaw = subtitleRenderStyleRaw.target as Record<string, unknown> | undefined;
        const layoutRaw = subtitleRenderStyleRaw.layout as Record<string, unknown> | undefined;
        const subtitleRenderStyle: SubtitleRenderStyle = {
          source: normalizeSubtitleLineStyle(sourceRaw, {
            fontFamily: "Arial",
            fontSize: 44,
            primaryColor: "#FFFFFF",
            outlineColor: "#101010",
            backColor: "#000000",
            outline: 2.5,
            shadow: 1,
            borderStyle: "outline",
            borderOpacity: 88,
          }),
          target: normalizeSubtitleLineStyle(targetRaw, {
            fontFamily: "Microsoft YaHei",
            fontSize: 40,
            primaryColor: "#EAF6FF",
            outlineColor: "#101010",
            backColor: "#000000",
            outline: 2.5,
            shadow: 1,
            borderStyle: "outline",
            borderOpacity: 88,
          }),
          layout: {
            marginV: Number.isFinite(layoutRaw?.marginV)
              ? Math.max(0, Math.min(200, Math.round(Number(layoutRaw?.marginV))))
              : 40,
            alignment: layoutRaw?.alignment === 1
              || layoutRaw?.alignment === 2
              || layoutRaw?.alignment === 3
              ? layoutRaw?.alignment
              : 2,
            bilingualLineGap: Number.isFinite(layoutRaw?.bilingualLineGap)
              ? Math.max(0, Math.min(140, Math.round(Number(layoutRaw?.bilingualLineGap))))
              : 10,
          },
        };

        dispatch({
          type: "set_settings",
          settings: {
            provider,
            chunkTargetSeconds,
            subtitleLengthPreset,
            asrModel,
            alignModel,
            demucsModel,
            enableVocalSeparation,
            translateApiKey,
            translateBaseUrl,
            translateModel,
            llmConcurrency,
            terminologyGroups,
            enableTerminology,
            enableSubtitleBeautify,
            enableClickSound,
            autoBurnHardSubtitle,
            subtitleBurnMode,
            subtitleRenderStyle,
          },
        });
      } catch {
        // Use default settings when DB read fails.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [dispatch]);
}

function normalizeSubtitleLengthPreset(value: unknown): SubtitleLengthPreset {
  return value === "short" || value === "standard" || value === "loose" ? value : "standard";
}

function normalizeAsrModel(value: unknown): AsrModel {
  if (value === "Qwen3-ASR-0.6B" || value === "Qwen3-ASR-1.7B") {
    return value;
  }
  return "Qwen3-ASR-0.6B";
}

function normalizeHexColor(raw: unknown, fallback: string): string {
  const value = String(raw ?? "").trim();
  if (/^#[0-9a-fA-F]{6}$/.test(value)) {
    return value.toUpperCase();
  }
  return fallback;
}

function normalizeSubtitleLineStyle(
  raw: Record<string, unknown> | undefined,
  fallback: SubtitleLineStyle,
): SubtitleLineStyle {
  const value = raw ?? {};
  return {
    fontFamily: String(value.fontFamily ?? fallback.fontFamily).trim() || fallback.fontFamily,
    fontSize: Number.isFinite(value.fontSize)
      ? Math.max(16, Math.min(96, Math.round(Number(value.fontSize))))
      : fallback.fontSize,
    primaryColor: normalizeHexColor(value.primaryColor, fallback.primaryColor),
    outlineColor: normalizeHexColor(value.outlineColor, fallback.outlineColor),
    backColor: normalizeHexColor(value.backColor, fallback.backColor),
    outline: Number.isFinite(value.outline)
      ? Math.max(0, Math.min(8, Number(value.outline)))
      : fallback.outline,
    shadow: Number.isFinite(value.shadow)
      ? Math.max(0, Math.min(8, Number(value.shadow)))
      : fallback.shadow,
    borderStyle: value.borderStyle === "box" ? "box" : "outline",
    borderOpacity: Number.isFinite(value.borderOpacity)
      ? Math.max(0, Math.min(100, Math.round(Number(value.borderOpacity))))
      : fallback.borderOpacity,
  };
}
