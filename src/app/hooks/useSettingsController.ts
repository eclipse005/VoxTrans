import { useCallback } from "react";
import {
  saveAppSettings as saveAppSettingsApi,
  testTranslateLlmConnection,
} from "../api/settings";
import type { SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";
import { normalizeTerminologyGroups } from "../utils/terminology";

type DispatchState = (action: AppAction) => void;
type PushToast = (
  message: string,
  tone?: ToastTone,
  options?: { id?: number; sticky?: boolean; durationMs?: number },
) => number;

type UseSettingsControllerArgs = {
  settings: SavedSettings;
  draftProvider: SavedSettings["provider"];
  draftChunkInput: string;
  draftSubtitleMaxWordsInput: string;
  draftAsrModel: SavedSettings["asrModel"];
  draftDemucsModel: SavedSettings["demucsModel"];
  draftEnableVocalSeparation: boolean;
  draftTranslateApiKey: string;
  draftTranslateBaseUrl: string;
  draftTranslateModel: string;
  draftLlmConcurrencyInput: string;
  draftTerminologyGroups: SavedSettings["terminologyGroups"];
  draftEnableTerminology: boolean;
  draftEnablePunctuationOptimization: boolean;
  draftEnableAsrCorrection: boolean;
  draftEnableSubtitleBeautify: boolean;
  dispatch: DispatchState;
  pushToast: PushToast;
  refreshModelStatus: () => Promise<void>;
};

export function useSettingsController({
  settings,
  draftProvider,
  draftChunkInput,
  draftSubtitleMaxWordsInput,
  draftAsrModel,
  draftDemucsModel,
  draftEnableVocalSeparation,
  draftTranslateApiKey,
  draftTranslateBaseUrl,
  draftTranslateModel,
  draftLlmConcurrencyInput,
  draftTerminologyGroups,
  draftEnableTerminology,
  draftEnablePunctuationOptimization,
  draftEnableAsrCorrection,
  draftEnableSubtitleBeautify,
  dispatch,
  pushToast,
  refreshModelStatus,
}: UseSettingsControllerArgs) {
  const openSettings = useCallback(() => {
    void refreshModelStatus();
    dispatch({
      type: "set_draft",
      payload: {
        draftProvider: settings.provider,
        draftChunkInput: String(settings.chunkTargetSeconds),
        draftSubtitleMaxWordsInput: String(settings.subtitleMaxWordsPerSegment),
        draftAsrModel: settings.asrModel,
        draftDemucsModel: settings.demucsModel,
        draftEnableVocalSeparation: settings.enableVocalSeparation,
        draftTranslateApiKey: settings.translateApiKey,
        draftTranslateBaseUrl: settings.translateBaseUrl,
        draftTranslateModel: settings.translateModel,
        draftLlmConcurrencyInput: String(settings.llmConcurrency),
        draftTerminologyGroups: settings.terminologyGroups,
        draftEnableTerminology: settings.enableTerminology,
        draftEnablePunctuationOptimization: settings.enablePunctuationOptimization,
        draftEnableAsrCorrection: settings.enableAsrCorrection,
        draftEnableSubtitleBeautify: settings.enableSubtitleBeautify,
      },
    });
    dispatch({ type: "set_ui", payload: { showSettings: true } });
  }, [
    dispatch,
    refreshModelStatus,
    settings.chunkTargetSeconds,
    settings.demucsModel,
    settings.enableVocalSeparation,
    settings.enablePunctuationOptimization,
    settings.provider,
    settings.asrModel,
    settings.subtitleMaxWordsPerSegment,
    settings.translateApiKey,
    settings.translateBaseUrl,
    settings.translateModel,
    settings.llmConcurrency,
    settings.terminologyGroups,
    settings.enableTerminology,
    settings.enableAsrCorrection,
    settings.enableSubtitleBeautify,
  ]);

  const saveSettings = useCallback(async () => {
    const parsed = Number.parseInt(draftChunkInput.trim(), 10);
    if (!Number.isFinite(parsed)) {
      pushToast("分段时长必须是数字", "error");
      return;
    }
    const clamped = Math.max(30, Math.min(300, parsed));

    const parsedSubtitleWords = Number.parseInt(draftSubtitleMaxWordsInput.trim(), 10);
    if (!Number.isFinite(parsedSubtitleWords)) {
      pushToast("字幕长度必须是数字", "error");
      return;
    }
    const clampedSubtitleWords = Math.max(8, Math.min(40, parsedSubtitleWords));
    const parsedConcurrency = Number.parseInt(draftLlmConcurrencyInput.trim(), 10);
    if (!Number.isFinite(parsedConcurrency)) {
      pushToast("并发数必须是数字", "error");
      return;
    }
    const clampedConcurrency = Math.max(1, Math.min(16, parsedConcurrency));

    const nextSettings = {
      provider: draftProvider,
      chunkTargetSeconds: clamped,
      subtitleMaxWordsPerSegment: clampedSubtitleWords,
      asrModel: draftAsrModel,
      demucsModel: draftDemucsModel,
      enableVocalSeparation: draftEnableVocalSeparation,
      translateApiKey: draftTranslateApiKey.trim(),
      translateBaseUrl: draftTranslateBaseUrl.trim() || "https://api.openai.com/v1",
      translateModel: draftTranslateModel.trim() || "gpt-4.1-mini",
      llmConcurrency: clampedConcurrency,
      terminologyGroups: normalizeTerminologyGroups(draftTerminologyGroups),
      enableTerminology: draftEnableTerminology,
      enablePunctuationOptimization: draftEnablePunctuationOptimization,
      enableAsrCorrection: draftEnableAsrCorrection,
      enableSubtitleBeautify: draftEnableSubtitleBeautify,
    } satisfies SavedSettings;

    dispatch({
      type: "set_settings",
      settings: nextSettings,
    });
    dispatch({
      type: "set_draft",
      payload: {
        draftChunkInput: String(clamped),
        draftSubtitleMaxWordsInput: String(clampedSubtitleWords),
        draftAsrModel,
        draftDemucsModel,
        draftEnableVocalSeparation,
        draftTranslateApiKey: nextSettings.translateApiKey,
        draftTranslateBaseUrl: nextSettings.translateBaseUrl,
        draftTranslateModel: nextSettings.translateModel,
        draftLlmConcurrencyInput: String(nextSettings.llmConcurrency),
        draftTerminologyGroups: nextSettings.terminologyGroups,
        draftEnableTerminology: nextSettings.enableTerminology,
        draftEnablePunctuationOptimization,
        draftEnableAsrCorrection,
        draftEnableSubtitleBeautify,
      },
    });

    try {
      await saveAppSettingsApi(nextSettings);
      pushToast("设置已保存（后续任务生效）", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "设置保存失败";
      pushToast(message, "error");
    }
  }, [
    dispatch,
    draftChunkInput,
    draftDemucsModel,
    draftEnableVocalSeparation,
    draftEnablePunctuationOptimization,
    draftProvider,
    draftAsrModel,
    draftSubtitleMaxWordsInput,
    draftTranslateApiKey,
    draftTranslateBaseUrl,
    draftTranslateModel,
    draftLlmConcurrencyInput,
    draftTerminologyGroups,
    draftEnableTerminology,
    pushToast,
    draftEnableAsrCorrection,
    draftEnableSubtitleBeautify,
  ]);

  return {
    openSettings,
    saveSettings,
    saveTerminologyGroups: async (groups: SavedSettings["terminologyGroups"]) => {
      const normalizedGroups = normalizeTerminologyGroups(groups);
      const nextSettings: SavedSettings = {
        ...settings,
        terminologyGroups: normalizedGroups,
      };
      dispatch({ type: "set_settings", settings: nextSettings });
      dispatch({ type: "set_draft", payload: { draftTerminologyGroups: normalizedGroups } });
      try {
        await saveAppSettingsApi(nextSettings);
        pushToast("术语已保存", "success");
      } catch (error) {
        const message = error instanceof Error ? error.message : "术语保存失败";
        pushToast(message, "error");
      }
    },
    testTranslateConnection: async () => {
      const apiKey = draftTranslateApiKey.trim();
      const baseUrl = draftTranslateBaseUrl.trim() || "https://api.openai.com/v1";
      const configuredModel = draftTranslateModel.trim() || "gpt-4.1-mini";
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
    },
  };
}
