import { useCallback } from "react";
import { saveAppSettings as saveAppSettingsApi } from "../api/settings";
import type { SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: ToastTone) => void;

type UseSettingsControllerArgs = {
  settings: SavedSettings;
  draftProvider: SavedSettings["provider"];
  draftChunkInput: string;
  draftSubtitleMaxWordsInput: string;
  draftAsrModel: SavedSettings["asrModel"];
  draftDemucsModel: SavedSettings["demucsModel"];
  draftEnableVocalSeparation: boolean;
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

    const nextSettings = {
      provider: draftProvider,
      chunkTargetSeconds: clamped,
      subtitleMaxWordsPerSegment: clampedSubtitleWords,
      asrModel: draftAsrModel,
      demucsModel: draftDemucsModel,
      enableVocalSeparation: draftEnableVocalSeparation,
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
    pushToast,
  ]);

  return {
    openSettings,
    saveSettings,
  };
}
