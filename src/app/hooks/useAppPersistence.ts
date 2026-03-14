import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Provider, UserPreferencesResponse } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";

type DispatchState = (action: AppAction) => void;

export function useAppPersistence(dispatch: DispatchState) {
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await invoke<UserPreferencesResponse>("load_user_preferences");
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

        dispatch({
          type: "set_settings",
          settings: {
            provider,
            chunkTargetSeconds,
            subtitleMaxWordsPerSegment,
          },
        });
        dispatch({
          type: "set_draft",
          payload: {
            draftProvider: provider,
            draftChunkInput: String(chunkTargetSeconds),
            draftSubtitleMaxWordsInput: String(subtitleMaxWordsPerSegment),
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
