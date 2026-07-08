import { useEffect } from "react";
import { loadUserPreferences } from "../api/preferences";
import { getDefaultSettings } from "../api/settings";
import { normalizeSettings } from "../utils/normalizeSettings";
import { changeAppLanguage } from "../../i18n";
import type { SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";

type DispatchState = (action: AppAction) => void;

/**
 * Hardcoded last-resort defaults, mirroring the backend's `default_settings()`.
 *
 * Used only if the `get_default_settings` IPC itself rejects (e.g. command not
 * registered in a partial build, or the app not ready during cold start).
 * Without it, `state.settings` would stay `null` forever and the app would sit
 * on an infinite loading screen. This guarantees the UI always reaches a usable
 * state even when the backend IPC layer is unavailable.
 */
const LAST_RESORT_DEFAULTS: SavedSettings = {
  provider: "cpu",
  chunkTargetSeconds: 30,
  subtitleLengthPreset: "standard",
  asrModel: "Qwen3-ASR-0.6B",
  alignModel: "Qwen3-ForcedAligner-0.6B",
  demucsModel: "htdemucs_ft",
  enableVocalSeparation: false,
  translateApiKey: "",
  translateBaseUrl: "https://api.deepseek.com/v1",
  translateModel: "deepseek-chat",
  llmConcurrency: 4,
  terminologyGroups: [{ id: "group-default", name: "Default", terms: [] }],
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
    layout: { marginV: 40, alignment: 2, bilingualLineGap: 10 },
  },
  flatSrtOutput: false,
  flatSrtItems: ["source", "target"],
  enableVisionAssist: false,
  locale: "zh-CN",
  modelsDir: null,
};

export function useAppPersistence(dispatch: DispatchState) {
  useEffect(() => {
    let cancelled = false;
    (async () => {
      // Defaults are static and do not touch the DB; they must succeed for the
      // app to have a usable settings snapshot. If the IPC itself is
      // unavailable, fall back to the hardcoded snapshot so the UI still loads.
      let defaults: SavedSettings;
      try {
        defaults = await getDefaultSettings();
      } catch {
        defaults = LAST_RESORT_DEFAULTS;
      }

      try {
        const prefs = await loadUserPreferences();
        if (cancelled) return;
        const settings = normalizeSettings(prefs.settings, defaults);
        dispatch({ type: "set_settings", settings });
        void changeAppLanguage(settings.locale);
      } catch {
        if (cancelled) return;
        // DB read failed: fall back to authoritative defaults so the UI does
        // not stay on the loading screen forever.
        dispatch({ type: "set_settings", settings: defaults });
        void changeAppLanguage(defaults.locale);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [dispatch]);
}
