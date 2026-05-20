import { useCallback, useEffect, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import type { QueueItem, SubtitleCue } from "../../features/media/types";
import { buildSubtitleSegmentsFromCues, subtitleSegmentsToSrt } from "../../features/media/subtitleSegments";
import type { ExportSrtItem } from "../api/transcribe";
import { getTaskRunQueueItem } from "../api/workspace";
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
import { exportSubtitleVariantsToDirectory, loadSubtitleEditorData, saveSubtitleEditor } from "./subtitleWorkflow/io";
import { buildSubtitleVersion } from "./subtitleWorkflow/versionSync";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseSubtitleWorkflowArgs = {
  queue: QueueItem[];
  activeId: string;
  subtitleTaskId: string;
  subtitleTaskName: string;
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
  subtitleSrtPath,
  subtitleCues,
  subtitleDirty,
  dispatch,
  pushToast,
}: UseSubtitleWorkflowArgs) {
  const loadedSubtitleVersionRef = useRef<string>("");
  const persistSeqRef = useRef(0);
  const loadSeqRef = useRef(0);
  const currentSubtitleTaskIdRef = useRef<string>(subtitleTaskId);
  const activeTaskIdRef = useRef<string>(activeId);
  const existingTaskIdsRef = useRef<Set<string>>(new Set(queue.map((item) => item.id)));

  useEffect(() => {
    currentSubtitleTaskIdRef.current = subtitleTaskId;
  }, [subtitleTaskId]);

  useEffect(() => {
    activeTaskIdRef.current = activeId;
  }, [activeId]);

  useEffect(() => {
    existingTaskIdsRef.current = new Set(queue.map((item) => item.id));
  }, [queue]);

  const canEditTask = useCallback((taskId: string) => {
    return queue.some((item) => item.id === taskId && item.transcribeStatus === "done");
  }, [queue]);

  const persistSubtitleToTask = useCallback(async (
    taskId: string,
    cues: SubtitleCue[],
  ) => {
    const seq = ++persistSeqRef.current;
    try {
      await saveSubtitleEditor(taskId, cues);
      if (seq !== persistSeqRef.current) return;
      if (currentSubtitleTaskIdRef.current !== taskId) return;
      if (!existingTaskIdsRef.current.has(taskId)) return;
    } catch (err) {
      reportError(err, "saveSubtitleEditor");
      const message = toUserErrorMessage(err, "字幕保存失败");
      if (currentSubtitleTaskIdRef.current !== taskId) {
        return;
      }
      if (!existingTaskIdsRef.current.has(taskId)) {
        return;
      }
      if (/task not found|taskId is required/i.test(message)) {
        return;
      }
      pushToast(message, "error");
    }
  }, [pushToast]);

  const loadSubtitleEditor = useCallback(
    async (item: QueueItem) => {
      const seq = ++loadSeqRef.current;
      const taskId = item.id;
      const isStaleRequest = () => (
        seq !== loadSeqRef.current
        || activeTaskIdRef.current !== taskId
        || !existingTaskIdsRef.current.has(taskId)
      );

      if (item.transcribeStatus !== "done") {
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: item.id,
            subtitleTaskName: item.name,
            subtitleMediaPath: item.path,
            subtitleSrtPath: "",
            subtitleCues: [],
            subtitleCueWarnings: {},
            subtitleDirty: false,
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(item);
        return;
      }

      try {
        const taskDetail = await getTaskRunQueueItem(taskId);
        if (isStaleRequest()) return;

        const enrichedItem: QueueItem = {
          ...item,
          ...taskDetail,
        };
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: enrichedItem.id,
            subtitleTaskName: enrichedItem.name,
            subtitleMediaPath: enrichedItem.path,
          },
        });

        const { response, hydratedCues } = await loadSubtitleEditorData(enrichedItem);
        if (isStaleRequest()) return;

        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleSrtPath: response.srtPath,
            subtitleCues: hydratedCues,
            subtitleCueWarnings: buildCueWarningsById(hydratedCues, response.warnings),
            subtitleDirty: false,
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(enrichedItem);
      } catch (error) {
        if (isStaleRequest()) return;
        reportError(error, "loadSubtitleEditor");
        pushToast("字幕格式有误，无法加载编辑器", "error");
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: taskId,
            subtitleTaskName: item.name,
            subtitleMediaPath: item.path,
            subtitleSrtPath: "",
            subtitleCues: [],
            subtitleCueWarnings: {},
            subtitleDirty: false,
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(item);
      }
    },
    [dispatch, pushToast],
  );

  const markSubtitleEdited = useCallback(
    (nextCues: SubtitleCue[]) => {
      dispatch({
        type: "set_subtitle",
        payload: {
          subtitleCues: nextCues,
          subtitleCueWarnings: {},
          subtitleDirty: true,
        },
      });
      if (subtitleTaskId) {
        if (!canEditTask(subtitleTaskId)) {
          return;
        }
        const taskStillExists = queue.some((item) => item.id === subtitleTaskId);
        if (!taskStillExists) {
          return;
        }
        const segments = buildSubtitleSegmentsFromCues(nextCues);
        const resultSrt = subtitleSegmentsToSrt(segments);
        const subtitleSegmentsJson = JSON.stringify(segments);
        dispatch({
          type: "patch_queue_item",
          id: subtitleTaskId,
          updater: (item) => ({
            ...item,
            resultSrt,
            subtitleSegmentsJson,
          }),
        });
        void persistSubtitleToTask(subtitleTaskId, nextCues);
      }
    },
    [canEditTask, dispatch, persistSubtitleToTask, queue, subtitleTaskId],
  );

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

  const exportSubtitleSrt = useCallback(async (items: ExportSrtItem[]) => {
    if (!subtitleTaskId) {
      pushToast("当前没有可导出的任务", "error");
      return;
    }
    if (items.length === 0) {
      pushToast("请至少选择一项导出内容", "error");
      return;
    }

    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择导出目录",
      });
      if (!picked || Array.isArray(picked)) return;

      const paths = await exportSubtitleVariantsToDirectory(
        subtitleTaskId,
        picked,
        subtitleTaskName,
        items,
      );
      if (paths.length === 1) {
        pushToast(`已导出：${paths[0]}`, "success");
      } else {
        pushToast(`已导出 ${paths.length} 个文件`, "success");
      }
    } catch (error) {
      reportError(error, "exportSubtitleSrt");
      pushToast(toUserErrorMessage(error, "导出字幕失败"), "error");
    }
  }, [pushToast, subtitleTaskId, subtitleTaskName]);

  useEffect(() => {
    const activeItem = queue.find((item) => item.id === activeId);
    if (!activeItem) {
      loadSeqRef.current += 1;
      dispatch({
        type: "set_subtitle",
        payload: {
          subtitleTaskId: "",
          subtitleTaskName: "",
          subtitleMediaPath: "",
          subtitleSrtPath: "",
          subtitleCues: [],
          subtitleCueWarnings: {},
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
    const currentVersion = buildSubtitleVersion(activeItem);
    if (loadedSubtitleVersionRef.current === currentVersion) return;
    void loadSubtitleEditor(activeItem);
  }, [activeId, dispatch, loadSubtitleEditor, queue, subtitleDirty, subtitleTaskId]);

  const activeItem = queue.find((item) => item.id === activeId) ?? null;
  const canEditSubtitle = activeItem?.transcribeStatus === "done";

  return {
    activeItem,
    canEditSubtitle,
    subtitleTaskName,
    subtitleSrtPath,
    subtitleCues,
    updateCue,
    addCueAfter,
    mergeSelectedCues,
    splitSelectedCues,
    replaceTextInCues,
    removeCue,
    exportSubtitleSrt,
  };
}
