import { useCallback } from "react";
import {
  saveAppSettings as saveAppSettingsApi,
  testTranslateLlmConnection,
} from "../api/settings";
import type { SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";
import { normalizeTerminologyGroups } from "../utils/terminology";
import { normalizeHotwordGroups } from "../utils/hotwords";

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
  draftSubtitleLengthReferenceInput: string;
  draftAsrModel: SavedSettings["asrModel"];
  draftDemucsModel: SavedSettings["demucsModel"];
  draftEnableVocalSeparation: boolean;
  draftTranslateApiKey: string;
  draftTranslateBaseUrl: string;
  draftTranslateModel: string;
  draftLlmConcurrencyInput: string;
  draftTerminologyGroups: SavedSettings["terminologyGroups"];
  draftEnableTerminology: boolean;
  draftHotwordGroups: SavedSettings["hotwordGroups"];
  draftEnableHotwords: boolean;
  draftEnableSubtitleBeautify: boolean;
  draftAutoBurnHardSubtitle: boolean;
  draftSubtitleBurnMode: SavedSettings["subtitleBurnMode"];
  draftSubtitleRenderStyle: SavedSettings["subtitleRenderStyle"];
  dispatch: DispatchState;
  pushToast: PushToast;
  refreshModelStatus: () => Promise<void>;
};

export function useSettingsController({
  settings,
  draftProvider,
  draftChunkInput,
  draftSubtitleMaxWordsInput,
  draftSubtitleLengthReferenceInput,
  draftAsrModel,
  draftDemucsModel,
  draftEnableVocalSeparation,
  draftTranslateApiKey,
  draftTranslateBaseUrl,
  draftTranslateModel,
  draftLlmConcurrencyInput,
  draftTerminologyGroups,
  draftEnableTerminology,
  draftHotwordGroups,
  draftEnableHotwords,
  draftEnableSubtitleBeautify,
  draftAutoBurnHardSubtitle,
  draftSubtitleBurnMode,
  draftSubtitleRenderStyle,
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
        draftSubtitleLengthReferenceInput: String(settings.subtitleLengthReference),
        draftAsrModel: settings.asrModel,
        draftDemucsModel: settings.demucsModel,
        draftEnableVocalSeparation: settings.enableVocalSeparation,
        draftTranslateApiKey: settings.translateApiKey,
        draftTranslateBaseUrl: settings.translateBaseUrl,
        draftTranslateModel: settings.translateModel,
        draftLlmConcurrencyInput: String(settings.llmConcurrency),
        draftTerminologyGroups: settings.terminologyGroups,
        draftEnableTerminology: settings.enableTerminology,
        draftHotwordGroups: settings.hotwordGroups,
        draftEnableHotwords: settings.enableHotwords,
        draftEnableSubtitleBeautify: settings.enableSubtitleBeautify,
        draftAutoBurnHardSubtitle: settings.autoBurnHardSubtitle,
        draftSubtitleBurnMode: settings.subtitleBurnMode,
        draftSubtitleRenderStyle: settings.subtitleRenderStyle,
      },
    });
    dispatch({ type: "set_ui", payload: { showSettings: true } });
  }, [
    dispatch,
    refreshModelStatus,
    settings.chunkTargetSeconds,
    settings.demucsModel,
    settings.enableVocalSeparation,
    settings.provider,
    settings.asrModel,
    settings.subtitleMaxWordsPerSegment,
    settings.subtitleLengthReference,
    settings.translateApiKey,
    settings.translateBaseUrl,
    settings.translateModel,
    settings.llmConcurrency,
    settings.terminologyGroups,
    settings.enableTerminology,
    settings.hotwordGroups,
    settings.enableHotwords,
    settings.enableSubtitleBeautify,
    settings.autoBurnHardSubtitle,
    settings.subtitleBurnMode,
    settings.subtitleRenderStyle,
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
      pushToast("原文长度必须是数字", "error");
      return;
    }
    const clampedSubtitleWords = Math.max(8, Math.min(40, parsedSubtitleWords));
    const parsedSubtitleLengthReference = Number.parseInt(draftSubtitleLengthReferenceInput.trim(), 10);
    if (!Number.isFinite(parsedSubtitleLengthReference)) {
      pushToast("译文长度必须是数字", "error");
      return;
    }
    const clampedSubtitleLengthReference = Math.max(8, Math.min(80, parsedSubtitleLengthReference));
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
      subtitleLengthReference: clampedSubtitleLengthReference,
      asrModel: draftAsrModel,
      demucsModel: draftDemucsModel,
      enableVocalSeparation: draftEnableVocalSeparation,
      translateApiKey: draftTranslateApiKey.trim(),
      translateBaseUrl: draftTranslateBaseUrl.trim() || "https://api.openai.com/v1",
      translateModel: draftTranslateModel.trim() || "gpt-4.1-mini",
      llmConcurrency: clampedConcurrency,
      terminologyGroups: normalizeTerminologyGroups(draftTerminologyGroups),
      enableTerminology: draftEnableTerminology,
      hotwordGroups: normalizeHotwordGroups(draftHotwordGroups),
      enableHotwords: draftEnableHotwords,
      enableSubtitleBeautify: draftEnableSubtitleBeautify,
      autoBurnHardSubtitle: draftAutoBurnHardSubtitle,
      subtitleBurnMode: draftSubtitleBurnMode,
      subtitleRenderStyle: {
        source: normalizeSubtitleLineStyle(draftSubtitleRenderStyle.source, {
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
        target: normalizeSubtitleLineStyle(draftSubtitleRenderStyle.target, {
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
          marginV: Math.max(0, Math.min(200, Math.round(draftSubtitleRenderStyle.layout.marginV))),
          alignment: draftSubtitleRenderStyle.layout.alignment,
          bilingualLineGap: Math.max(0, Math.min(140, Math.round(draftSubtitleRenderStyle.layout.bilingualLineGap))),
        },
      },
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
        draftSubtitleLengthReferenceInput: String(clampedSubtitleLengthReference),
        draftAsrModel,
        draftDemucsModel,
        draftEnableVocalSeparation,
        draftTranslateApiKey: nextSettings.translateApiKey,
        draftTranslateBaseUrl: nextSettings.translateBaseUrl,
        draftTranslateModel: nextSettings.translateModel,
        draftLlmConcurrencyInput: String(nextSettings.llmConcurrency),
        draftTerminologyGroups: nextSettings.terminologyGroups,
        draftEnableTerminology: nextSettings.enableTerminology,
        draftHotwordGroups: nextSettings.hotwordGroups,
        draftEnableHotwords: nextSettings.enableHotwords,
        draftEnableSubtitleBeautify,
        draftAutoBurnHardSubtitle: nextSettings.autoBurnHardSubtitle,
        draftSubtitleBurnMode: nextSettings.subtitleBurnMode,
        draftSubtitleRenderStyle: nextSettings.subtitleRenderStyle,
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
    draftProvider,
    draftAsrModel,
    draftSubtitleMaxWordsInput,
    draftSubtitleLengthReferenceInput,
    draftTranslateApiKey,
    draftTranslateBaseUrl,
    draftTranslateModel,
    draftLlmConcurrencyInput,
    draftTerminologyGroups,
    draftEnableTerminology,
    draftHotwordGroups,
    draftEnableHotwords,
    draftAutoBurnHardSubtitle,
    draftSubtitleBurnMode,
    draftSubtitleRenderStyle,
    pushToast,
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
    saveHotwordGroups: async (groups: SavedSettings["hotwordGroups"]) => {
      const normalizedGroups = normalizeHotwordGroups(groups);
      const nextSettings: SavedSettings = {
        ...settings,
        hotwordGroups: normalizedGroups,
      };
      dispatch({ type: "set_settings", settings: nextSettings });
      dispatch({ type: "set_draft", payload: { draftHotwordGroups: normalizedGroups } });
      try {
        await saveAppSettingsApi(nextSettings);
        pushToast("热词已保存", "success");
      } catch (error) {
        const message = error instanceof Error ? error.message : "热词保存失败";
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
