import { useCallback, useEffect, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import type { QueueItem, SubtitleCue } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import { reportError, toUserErrorMessage } from "../utils/errors";
import { buildCueWarningsById } from "../utils/subtitleWarnings";
import {
  addCueAfterSelection,
  mergeSelectedCueList,
  removeCueFromList,
  replaceTextInCueList,
  splitSelectedCueList,
  updateCueList,
} from "./subtitleWorkflow/cueOperations";
import { exportSubtitleToDirectory, loadSubtitleEditorData, saveSubtitleEditor } from "./subtitleWorkflow/io";
import { buildSubtitleVersion } from "./subtitleWorkflow/versionSync";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseSubtitleWorkflowArgs = {
  queue: QueueItem[];
  activeId: string;
  subtitleTaskId: string;
  subtitleTaskName: string;
  subtitleMediaPath: string;
  subtitleSrtPath: string;
  subtitleCues: SubtitleCue[];
  subtitleDirty: boolean;
  dispatch: DispatchState;
  pushToast: PushToast;
};

export function useSubtitleWorkflow({
  queue,
  activeId,
  subtitleTaskId,
  subtitleTaskName,
  subtitleMediaPath,
  subtitleSrtPath,
  subtitleCues,
  subtitleDirty,
  dispatch,
  pushToast,
}: UseSubtitleWorkflowArgs) {
  const subtitleSaveTimerRef = useRef<number | null>(null);
  const subtitleSavedIndicatorTimerRef = useRef<number | null>(null);
  const loadedSubtitleVersionRef = useRef<string>("");

  const clearSubtitleSavedIndicatorTimer = useCallback(() => {
    if (subtitleSavedIndicatorTimerRef.current != null) {
      window.clearTimeout(subtitleSavedIndicatorTimerRef.current);
      subtitleSavedIndicatorTimerRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => {
      if (subtitleSaveTimerRef.current != null) {
        window.clearTimeout(subtitleSaveTimerRef.current);
      }
      if (subtitleSavedIndicatorTimerRef.current != null) {
        window.clearTimeout(subtitleSavedIndicatorTimerRef.current);
      }
    };
  }, []);

  const saveSubtitle = useCallback(
    async (finalSave: boolean) => {
      if (!subtitleMediaPath || !subtitleTaskId) return;

      try {
        clearSubtitleSavedIndicatorTimer();
        dispatch({ type: "set_subtitle", payload: { subtitleSaveState: "saving" } });
        const response = await saveSubtitleEditor(subtitleTaskId, subtitleMediaPath, subtitleCues, finalSave);

        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleSaveState: "saved",
            subtitleDirty: false,
            subtitleSrtPath: response.srtPath,
            subtitleCueWarnings: buildCueWarningsById(subtitleCues, response.warnings),
          },
        });

        subtitleSavedIndicatorTimerRef.current = window.setTimeout(() => {
          dispatch({ type: "set_subtitle", payload: { subtitleSaveState: "idle" } });
          subtitleSavedIndicatorTimerRef.current = null;
        }, 1200);

        if (finalSave) {
          pushToast("字幕已保存", "success");
        }
      } catch (err) {
        reportError(err, "saveSubtitle");
        dispatch({ type: "set_subtitle", payload: { subtitleSaveState: "error" } });
        if (finalSave) {
          pushToast(toUserErrorMessage(err, "字幕保存失败"), "error");
        }
      }
    },
    [clearSubtitleSavedIndicatorTimer, dispatch, pushToast, subtitleCues, subtitleMediaPath, subtitleTaskId],
  );

  const loadSubtitleEditor = useCallback(
    async (item: QueueItem) => {
      try {
        clearSubtitleSavedIndicatorTimer();
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: item.id,
            subtitleTaskName: item.name,
            subtitleMediaPath: item.path,
            subtitleSaveState: "idle",
          },
        });

        const { response, hydratedCues } = await loadSubtitleEditorData(item);

        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleSrtPath: response.srtPath,
            subtitleDraftPath: response.draftPath,
            subtitleCues: hydratedCues,
            subtitleCueWarnings: buildCueWarningsById(hydratedCues, response.warnings),
            subtitleDirty: false,
            subtitleSaveState: "idle",
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(item);

        if (response.usingDraft) {
          pushToast("已恢复自动保存草稿", "info");
        }
      } catch (error) {
        reportError(error, "loadSubtitleEditor");
        pushToast("字幕格式有误，无法加载编辑器", "error");
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: item.id,
            subtitleTaskName: item.name,
            subtitleMediaPath: item.path,
            subtitleDraftPath: "",
            subtitleSrtPath: "",
            subtitleCues: [],
            subtitleCueWarnings: {},
            subtitleDirty: false,
            subtitleSaveState: "error",
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(item);
      }
    },
    [clearSubtitleSavedIndicatorTimer, dispatch, pushToast],
  );

  const markSubtitleEdited = useCallback(
    (nextCues: SubtitleCue[]) => {
      clearSubtitleSavedIndicatorTimer();
      dispatch({
        type: "set_subtitle",
        payload: {
          subtitleCues: nextCues,
          subtitleCueWarnings: {},
          subtitleDirty: true,
          subtitleSaveState: "idle",
        },
      });
    },
    [clearSubtitleSavedIndicatorTimer, dispatch],
  );

  useEffect(() => {
    if (!subtitleMediaPath || !subtitleDirty) {
      return;
    }

    if (subtitleSaveTimerRef.current) {
      window.clearTimeout(subtitleSaveTimerRef.current);
    }

    subtitleSaveTimerRef.current = window.setTimeout(() => {
      void saveSubtitle(false);
    }, 800);

    return () => {
      if (subtitleSaveTimerRef.current) {
        window.clearTimeout(subtitleSaveTimerRef.current);
      }
    };
  }, [saveSubtitle, subtitleMediaPath, subtitleCues, subtitleDirty]);

  const updateCue = useCallback(
    (cueId: string, patchCue: Partial<SubtitleCue>) => {
      markSubtitleEdited(updateCueList(subtitleCues, cueId, patchCue));
    },
    [markSubtitleEdited, subtitleCues],
  );

  const addCueAfter = useCallback(
    (selectedCueId: string | null) => {
      markSubtitleEdited(addCueAfterSelection(subtitleCues, selectedCueId));
    },
    [markSubtitleEdited, subtitleCues],
  );

  const mergeSelectedCues = useCallback(
    (selectedCueIds: string[]) => {
      const next = mergeSelectedCueList(subtitleCues, selectedCueIds);
      if (next !== subtitleCues) {
        markSubtitleEdited(next);
      }
    },
    [markSubtitleEdited, subtitleCues],
  );

  const splitSelectedCues = useCallback(
    (selectedCueIds: string[]): Array<{ sourceCueId: string; bornCueId: string }> => {
      const { nextCues, bornCueIds } = splitSelectedCueList(subtitleCues, selectedCueIds);
      if (bornCueIds.length > 0) {
        markSubtitleEdited(nextCues);
      }
      return bornCueIds;
    },
    [markSubtitleEdited, subtitleCues],
  );

  const replaceTextInCues = useCallback(
    (findText: string, replaceText: string, scopeCueIds: string[] | null, maxReplacements?: number): number => {
      const { nextCues, replacedCount } = replaceTextInCueList(
        subtitleCues,
        findText,
        replaceText,
        scopeCueIds,
        maxReplacements,
      );
      if (replacedCount > 0) {
        markSubtitleEdited(nextCues);
      }
      return replacedCount;
    },
    [markSubtitleEdited, subtitleCues],
  );

  const removeCue = useCallback(
    (cueId: string) => {
      markSubtitleEdited(removeCueFromList(subtitleCues, cueId));
    },
    [markSubtitleEdited, subtitleCues],
  );

  const exportSubtitleSrt = useCallback(async () => {
    if (!subtitleTaskId) {
      pushToast("当前没有可导出的任务", "error");
      return;
    }

    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择导出目录",
      });
      if (!picked || Array.isArray(picked)) return;

      const exportPath = await exportSubtitleToDirectory(subtitleTaskId, picked, subtitleTaskName, subtitleCues);
      pushToast(`已导出：${exportPath}`, "success");
    } catch (error) {
      reportError(error, "exportSubtitleSrt");
      pushToast(toUserErrorMessage(error, "导出字幕失败"), "error");
    }
  }, [pushToast, subtitleCues, subtitleTaskId, subtitleTaskName]);

  useEffect(() => {
    const activeItem = queue.find((item) => item.id === activeId);
    if (!activeItem) {
      if (subtitleSaveTimerRef.current != null) {
        window.clearTimeout(subtitleSaveTimerRef.current);
        subtitleSaveTimerRef.current = null;
      }
      clearSubtitleSavedIndicatorTimer();
      dispatch({
        type: "set_subtitle",
        payload: {
          subtitleTaskId: "",
          subtitleTaskName: "",
          subtitleMediaPath: "",
          subtitleDraftPath: "",
          subtitleSrtPath: "",
          subtitleCues: [],
          subtitleCueWarnings: {},
          subtitleSaveState: "idle",
          subtitleDirty: false,
        },
      });
      loadedSubtitleVersionRef.current = "";
      return;
    }

    if (subtitleTaskId !== activeItem.id) {
      void loadSubtitleEditor(activeItem);
      return;
    }

    if (subtitleDirty) return;
    if (activeItem.transcribeStatus !== "done") return;
    const currentVersion = buildSubtitleVersion(activeItem);
    if (loadedSubtitleVersionRef.current === currentVersion) return;
    void loadSubtitleEditor(activeItem);
  }, [activeId, clearSubtitleSavedIndicatorTimer, dispatch, loadSubtitleEditor, queue, subtitleDirty, subtitleTaskId]);

  const activeItem = queue.find((item) => item.id === activeId) ?? null;

  return {
    activeItem,
    subtitleTaskName,
    subtitleSrtPath,
    subtitleCues,
    saveSubtitle,
    updateCue,
    addCueAfter,
    mergeSelectedCues,
    splitSelectedCues,
    replaceTextInCues,
    removeCue,
    exportSubtitleSrt,
  };
}
