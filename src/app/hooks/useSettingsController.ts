import { useCallback, useState } from "react";
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
import { normalizeSettings } from "../utils/normalizeSettings";

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
  activeTerminologyGroupId: string;
  enableSubtitleBeautify: boolean;
  enableClickSound: boolean;
  autoBurnHardSubtitle: boolean;
  subtitleBurnMode: SubtitleBurnMode;
  subtitleRenderStyle: SubtitleRenderStyle;
  flatSrtOutput: boolean;
  flatSrtItems: SubtitleBurnMode[];
  enableVisionAssist: boolean;
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
    activeTerminologyGroupId: settings.activeTerminologyGroupId,
    enableSubtitleBeautify: settings.enableSubtitleBeautify,
    enableClickSound: settings.enableClickSound,
    autoBurnHardSubtitle: settings.autoBurnHardSubtitle,
    subtitleBurnMode: settings.subtitleBurnMode,
    subtitleRenderStyle: settings.subtitleRenderStyle,
    flatSrtOutput: settings.flatSrtOutput,
    flatSrtItems: settings.flatSrtItems,
    enableVisionAssist: settings.enableVisionAssist,
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

  // Keep terminology form fields in sync with the authoritative settings
  // snapshot. Call this before opening the terminology modal so it always
  // reflects the latest DB state, even when opened from outside the settings
  // dialog.
  const prepareTerminologyForm = useCallback(() => {
    setForm((prev) => ({
      ...prev,
      terminologyGroups: settings.terminologyGroups,
      activeTerminologyGroupId: settings.activeTerminologyGroupId,
    }));
  }, [settings.terminologyGroups, settings.activeTerminologyGroupId]);

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
    const parsedConcurrency = Number.parseInt(form.llmConcurrencyInput.trim(), 10);
    if (!Number.isFinite(parsedConcurrency)) {
      pushToast("并发数必须是数字", "error");
      return;
    }

    const draft: SavedSettings = {
      ...settings,
      provider: form.provider,
      chunkTargetSeconds: parsed,
      subtitleLengthPreset: form.subtitleLengthPreset,
      asrModel: form.asrModel,
      alignModel: form.alignModel,
      demucsModel: form.demucsModel,
      enableVocalSeparation: form.enableVocalSeparation,
      translateApiKey: form.translateApiKey,
      translateBaseUrl: form.translateBaseUrl,
      translateModel: form.translateModel,
      llmConcurrency: parsedConcurrency,
      terminologyGroups: normalizeTerminologyGroups(form.terminologyGroups),
      activeTerminologyGroupId: form.activeTerminologyGroupId,
      enableSubtitleBeautify: form.enableSubtitleBeautify,
      enableClickSound: form.enableClickSound,
      autoBurnHardSubtitle: form.autoBurnHardSubtitle,
      subtitleBurnMode: form.subtitleBurnMode,
      subtitleRenderStyle: form.subtitleRenderStyle,
      flatSrtOutput: form.flatSrtOutput,
      flatSrtItems: form.flatSrtItems,
      enableVisionAssist: form.enableVisionAssist,
    };

    const nextSettings = normalizeSettings(draft, settings);

    dispatch({ type: "set_settings", settings: nextSettings });
    setForm(settingsToForm(nextSettings));

    try {
      await saveAppSettingsApi(nextSettings);
      pushToast("设置已保存（后续任务生效）", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "设置保存失败";
      pushToast(message, "error");
    }
  }, [form, settings, dispatch, pushToast]);

  const saveTerminologyGroups = useCallback(async (groups: SavedSettings["terminologyGroups"]) => {
    const normalizedGroups = normalizeTerminologyGroups(groups);
    const nextSettings: SavedSettings = {
      ...settings,
      terminologyGroups: normalizedGroups,
      activeTerminologyGroupId: form.activeTerminologyGroupId,
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
  }, [settings, form.activeTerminologyGroupId, dispatch, pushToast]);

  const testTranslateConnection = useCallback(async () => {
    const apiKey = form.translateApiKey.trim();
    const baseUrl = form.translateBaseUrl.trim() || settings.translateBaseUrl;
    const configuredModel = form.translateModel.trim() || settings.translateModel;
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
        enableVisionAssist: form.enableVisionAssist,
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
  }, [form.translateApiKey, form.translateBaseUrl, form.translateModel, form.enableVisionAssist, settings.translateBaseUrl, settings.translateModel, pushToast]);

  return {
    openSettings,
    saveSettings,
    saveTerminologyGroups,
    testTranslateConnection,
    prepareTerminologyForm,
    form,
    setForm,
  };
}
