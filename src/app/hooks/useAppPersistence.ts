import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Provider, UserPreferencesResponse } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { HotwordCorrection, TermEntry } from "../types";

type DispatchState = (action: AppAction) => void;

export function useAppPersistence(terms: TermEntry[], hotwordCorrection: HotwordCorrection, dispatch: DispatchState) {
  const hydratedRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await invoke<UserPreferencesResponse>("load_user_preferences");
        if (cancelled) return;
        const provider = (res.settings.provider === "cpu" || res.settings.provider === "cuda")
          ? res.settings.provider as Provider
          : "cuda";
        dispatch({ type: "set_terms", terms: res.terms });
        dispatch({
          type: "set_settings",
          settings: {
            provider,
            chunkTargetSeconds: res.settings.chunkTargetSeconds,
            autoPunc: res.settings.autoPunc ?? true,
          },
        });
        dispatch({
          type: "set_draft",
          payload: {
            draftProvider: provider,
            draftChunkInput: String(res.settings.chunkTargetSeconds),
            draftAutoPunc: res.settings.autoPunc ?? true,
            draftApiKey: res.llm.apiKey ?? "",
            draftApiBase: res.llm.apiBase ?? "",
            draftApiModel: res.llm.apiModel ?? "",
            hotwordCorrection: res.hotwordCorrection,
          },
        });
        hydratedRef.current = true;
      } catch {
        hydratedRef.current = true;
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [dispatch]);

  useEffect(() => {
    if (!hydratedRef.current) return;
    void invoke("save_terms", { request: { terms } });
  }, [terms]);

  useEffect(() => {
    if (!hydratedRef.current) return;
    void invoke("save_hotword_correction", {
      request: {
        hotwordCorrection,
      },
    });
  }, [hotwordCorrection]);
}
