import { useEffect } from "react";
import { loadUserPreferences } from "../api/preferences";
import { normalizeProvider } from "../../features/media/provider";
import type {
  DemucsModel,
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
          ? Math.max(30, Math.min(300, Math.round(res.settings.chunkTargetSeconds)))
          : 300;
        const subtitleMaxWordsPerSegment = Number.isFinite(res.settings.subtitleMaxWordsPerSegment)
          ? Math.max(8, Math.min(40, Math.round(res.settings.subtitleMaxWordsPerSegment)))
          : 20;
        const asrModel = res.settings.asrModel === "parakeet-tdt-0.6b-v2"
          ? res.settings.asrModel
          : "parakeet-tdt-0.6b-v2";
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
        const enablePunctuationOptimization = Boolean(res.settings.enablePunctuationOptimization);

        dispatch({
          type: "set_settings",
          settings: {
            provider,
            chunkTargetSeconds,
            subtitleMaxWordsPerSegment,
            asrModel,
            demucsModel,
            enableVocalSeparation,
            translateApiKey,
            translateBaseUrl,
            translateModel,
            llmConcurrency,
            terminologyGroups,
            enableTerminology,
            enablePunctuationOptimization,
          },
        });
        dispatch({
          type: "set_draft",
          payload: {
            draftProvider: provider,
            draftChunkInput: String(chunkTargetSeconds),
            draftSubtitleMaxWordsInput: String(subtitleMaxWordsPerSegment),
            draftAsrModel: asrModel,
            draftDemucsModel: demucsModel,
            draftEnableVocalSeparation: enableVocalSeparation,
            draftTranslateApiKey: translateApiKey,
            draftTranslateBaseUrl: translateBaseUrl,
            draftTranslateModel: translateModel,
            draftLlmConcurrencyInput: String(llmConcurrency),
            draftTerminologyGroups: terminologyGroups,
            draftEnableTerminology: enableTerminology,
            draftEnablePunctuationOptimization: enablePunctuationOptimization,
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
