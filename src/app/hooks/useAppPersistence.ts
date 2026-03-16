import { useEffect } from "react";
import { loadUserPreferences } from "../api/preferences";
import type {
  DemucsModel,
  Provider,
  UserPreferencesResponse,
} from "../../features/media/types";
import type { AppAction } from "../state/appReducer";

type DispatchState = (action: AppAction) => void;

export function useAppPersistence(dispatch: DispatchState) {
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res: UserPreferencesResponse = await loadUserPreferences();
        if (cancelled) return;
        const provider = (res.settings.provider === "cpu"
          || res.settings.provider === "cuda")
          ? res.settings.provider as Provider
          : "cpu";
        const chunkTargetSeconds = Number.isFinite(res.settings.chunkTargetSeconds)
          ? Math.max(60, Math.min(300, Math.round(res.settings.chunkTargetSeconds)))
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

        dispatch({
          type: "set_settings",
          settings: {
            provider,
            chunkTargetSeconds,
            subtitleMaxWordsPerSegment,
            asrModel,
            demucsModel,
            enableVocalSeparation,
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
