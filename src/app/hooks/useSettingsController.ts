import { useCallback, useEffect, useState } from "react";
import {
  saveAppSettings as saveAppSettingsApi,
  testTranslateLlmConnection,
} from "../api/settings";
import type {
  AlignModel,
  AsrModel,
  DemucsModel,
  Provider,
  SavedSettings,
  SubtitleBurnMode,
  SubtitleLengthPreset,
  SubtitleRenderStyle,
} from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";
import { normalizeTerminologyGroups } from "../utils/terminology";

type DispatchState = (action: AppAction) => void;
type PushToast = (
  message: string,
  tone?: ToastTone,
  options?: { id?: number; sticky?: boolean; durationMs?: number },
) => number;

export type SettingsForm = {
  provider: Provider;
  chunkInput: string;
  subtitleLengthPreset: SubtitleLengthPreset;
  asrModel: AsrModel;
  alignModel: AlignModel;
  demucsModel: DemucsModel;
  enableVocalSeparation: boolean;
  translateApiKey: string;
  translateBaseUrl: string;
  translateModel: string;
  llmConcurrencyInput: string;
  terminologyGroups: SavedSettings["terminologyGroups"];
  enableTerminology: boolean;
  enableSubtitleBeautify: boolean;
  enableClickSound: boolean;
  autoBurnHardSubtitle: boolean;
  subtitleBurnMode: SubtitleBurnMode;
  subtitleRenderStyle: SubtitleRenderStyle;
  flatSrtOutput: boolean;
  flatSrtItems: SubtitleBurnMode[];
};

function settingsToForm(settings: SavedSettings): SettingsForm {
  return {
    provider: settings.provider,
    chunkInput: String(settings.chunkTargetSeconds),
    subtitleLengthPreset: settings.subtitleLengthPreset,
    asrModel: settings.asrModel,
    alignModel: settings.alignModel,
    demucsModel: settings.demucsModel,
    enableVocalSeparation: settings.enableVocalSeparation,
    translateApiKey: settings.translateApiKey,
    translateBaseUrl: settings.translateBaseUrl,
    translateModel: settings.translateModel,
    llmConcurrencyInput: String(settings.llmConcurrency),
    terminologyGroups: settings.terminologyGroups,
    enableTerminology: settings.enableTerminology,
    enableSubtitleBeautify: settings.enableSubtitleBeautify,
    enableClickSound: settings.enableClickSound,
    autoBurnHardSubtitle: settings.autoBurnHardSubtitle,
    subtitleBurnMode: settings.subtitleBurnMode,
    subtitleRenderStyle: settings.subtitleRenderStyle,
    flatSrtOutput: settings.flatSrtOutput ?? false,
    flatSrtItems: (settings.flatSrtItems ?? ["source", "target"]).filter(
      (v): v is SubtitleBurnMode =>
        v === "source" || v === "target" || v === "bilingualSourceFirst" || v === "bilingualTargetFirst"
    ),
  };
}

type UseSettingsControllerArgs = {
  settings: SavedSettings;
  dispatch: DispatchState;
  pushToast: PushToast;
  refreshModelStatus: () => Promise<void>;
};

export function useSettingsController({
  settings,
  dispatch,
  pushToast,
  refreshModelStatus,
}: UseSettingsControllerArgs) {
  const [form, setForm] = useState<SettingsForm>(() => settingsToForm(settings));

  // The terminology modal is opened from a separate entry point and reads
  // `form.terminologyGroups` directly, not via openSettings(). If the user
  // opens it before useAppPersistence's async load has populated `settings`,
  // the form still holds the initial empty default group and any saved
  // terms are invisible. Sync terminologyGroups whenever the upstream
  // settings change so the modal always reflects the latest DB state.
  useEffect(() => {
    setForm((prev) => ({ ...prev, terminologyGroups: settings.terminologyGroups }));
  }, [settings.terminologyGroups]);

  const openSettings = useCallback(() => {
    void refreshModelStatus();
    setForm(settingsToForm(settings));
    dispatch({ type: "set_ui", payload: { showSettings: true } });
  }, [dispatch, refreshModelStatus, settings]);

  const saveSettings = useCallback(async () => {
    const parsed = Number.parseInt(form.chunkInput.trim(), 10);
    if (!Number.isFinite(parsed)) {
      pushToast("分段时长必须是数字", "error");
      return;
    }
    const clamped = Math.max(30, Math.min(60, parsed));

    const parsedConcurrency = Number.parseInt(form.llmConcurrencyInput.trim(), 10);
    if (!Number.isFinite(parsedConcurrency)) {
      pushToast("并发数必须是数字", "error");
      return;
    }
    const clampedConcurrency = Math.max(1, Math.min(16, parsedConcurrency));

    const nextSettings: SavedSettings = {
      provider: form.provider,
      chunkTargetSeconds: clamped,
      subtitleLengthPreset: form.subtitleLengthPreset,
      asrModel: form.asrModel,
      alignModel: form.alignModel,
      demucsModel: form.demucsModel,
      enableVocalSeparation: form.enableVocalSeparation,
      translateApiKey: form.translateApiKey.trim(),
      translateBaseUrl: form.translateBaseUrl.trim() || "https://api.openai.com/v1",
      translateModel: form.translateModel.trim() || "gpt-4.1-mini",
      llmConcurrency: clampedConcurrency,
      terminologyGroups: normalizeTerminologyGroups(form.terminologyGroups),
      enableTerminology: form.enableTerminology,
      enableSubtitleBeautify: form.enableSubtitleBeautify,
      enableClickSound: form.enableClickSound,
      autoBurnHardSubtitle: form.autoBurnHardSubtitle,
      subtitleBurnMode: form.subtitleBurnMode,
      subtitleRenderStyle: {
        source: normalizeSubtitleLineStyle(form.subtitleRenderStyle.source, {
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
        target: normalizeSubtitleLineStyle(form.subtitleRenderStyle.target, {
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
          marginV: Math.max(0, Math.min(200, Math.round(form.subtitleRenderStyle.layout.marginV))),
          alignment: form.subtitleRenderStyle.layout.alignment,
          bilingualLineGap: Math.max(0, Math.min(140, Math.round(form.subtitleRenderStyle.layout.bilingualLineGap))),
        },
      },
      flatSrtOutput: form.flatSrtOutput,
      flatSrtItems: form.flatSrtItems,
    };

    dispatch({
      type: "set_settings",
      settings: nextSettings,
    });
    setForm(settingsToForm(nextSettings));

    try {
      await saveAppSettingsApi(nextSettings);
      pushToast("设置已保存（后续任务生效）", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "设置保存失败";
      pushToast(message, "error");
    }
  }, [form, dispatch, pushToast]);

  const saveTerminologyGroups = useCallback(async (groups: SavedSettings["terminologyGroups"]) => {
    const normalizedGroups = normalizeTerminologyGroups(groups);
    const nextSettings: SavedSettings = {
      ...settings,
      terminologyGroups: normalizedGroups,
    };
    dispatch({ type: "set_settings", settings: nextSettings });
    setForm((prev) => ({ ...prev, terminologyGroups: normalizedGroups }));
    try {
      await saveAppSettingsApi(nextSettings);
      pushToast("术语已保存", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "术语保存失败";
      pushToast(message, "error");
    }
  }, [settings, dispatch, pushToast]);

  const testTranslateConnection = useCallback(async () => {
    const apiKey = form.translateApiKey.trim();
    const baseUrl = form.translateBaseUrl.trim() || "https://api.openai.com/v1";
    const configuredModel = form.translateModel.trim() || "gpt-4.1-mini";
    if (!apiKey) {
      pushToast("请先填写接口密钥", "error");
      return;
    }
    const toastId = pushToast("正在测试 LLM 连通性...", "info", { sticky: true });
    try {
      const response = await testTranslateLlmConnection({
        apiKey,
        baseUrl,
        model: configuredModel,
      });
      if (response.ok) {
        const modelName = response.model?.trim() || configuredModel;
        pushToast(`测试成功：模型 ${modelName} 可用`, "success", { id: toastId, durationMs: 2600 });
        return;
      }
      pushToast(`测试失败：${response.message || "未知错误"}`, "error", { id: toastId, durationMs: 3000 });
    } catch (error) {
      const message = error instanceof Error ? error.message : "连通性测试失败";
      pushToast(message, "error", { id: toastId, durationMs: 3000 });
    }
  }, [form.translateApiKey, form.translateBaseUrl, form.translateModel, pushToast]);

  return {
    openSettings,
    saveSettings,
    saveTerminologyGroups,
    testTranslateConnection,
    form,
    setForm,
  };
}

function normalizeHexColor(raw: string, fallback: string): string {
  const value = String(raw ?? "").trim();
  if (/^#[0-9a-fA-F]{6}$/.test(value)) {
    return value.toUpperCase();
  }
  return fallback;
}

function normalizeSubtitleLineStyle(
  style: SavedSettings["subtitleRenderStyle"]["source"],
  fallback: SavedSettings["subtitleRenderStyle"]["source"],
): SavedSettings["subtitleRenderStyle"]["source"] {
  return {
    fontFamily: style.fontFamily.trim() || fallback.fontFamily,
    fontSize: Math.max(16, Math.min(96, Math.round(style.fontSize))),
    primaryColor: normalizeHexColor(style.primaryColor, fallback.primaryColor),
    outlineColor: normalizeHexColor(style.outlineColor, fallback.outlineColor),
    backColor: normalizeHexColor(style.backColor, fallback.backColor),
    outline: Math.max(0, Math.min(8, style.outline)),
    shadow: Math.max(0, Math.min(8, style.shadow)),
    borderStyle: style.borderStyle === "box" ? "box" : "outline",
    borderOpacity: Math.max(0, Math.min(100, Math.round(style.borderOpacity))),
  };
}
