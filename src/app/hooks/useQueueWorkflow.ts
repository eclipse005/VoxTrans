import { useCallback, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import type {
  BuildSegmentsResponse,
  QueueItem,
  QueueStatus,
  SavedSettings,
  TranscribeResponse,
} from "../../features/media/types";
import { detectMediaKind, fileName } from "../../features/media/utils";
import type { AppAction, AppState } from "../state/appReducer";
import { reportError, toUserErrorMessage } from "../utils/errors";

type PatchState = (payload: Partial<AppState>) => void;
type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type TranscribeProgressEvent = {
  taskId: string;
  currentSegment: number;
  totalSegments: number;
};

type UseQueueWorkflowArgs = {
  queue: QueueItem[];
  settings: SavedSettings;
  dispatch: DispatchState;
  patch: PatchState;
  pushToast: PushToast;
};

export function useQueueWorkflow({ queue, settings, dispatch, patch, pushToast }: UseQueueWorkflowArgs) {
  const queueCount = queue.length;
  const hasProcessingTask = useMemo(() => queue.some((item) => item.status === "processing"), [queue]);
  const hasQueuedTask = useMemo(() => queue.some((item) => item.status === "queued"), [queue]);
  const queueBusy = hasProcessingTask || hasQueuedTask;

  const appendPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;

    const incoming = await Promise.all(
      paths.map(async (path) => {
        let sizeBytes = 0;
        try {
          sizeBytes = await invoke<number>("get_file_size", { path });
        } catch {
          sizeBytes = 0;
        }

        return {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}-${path}`,
          path,
          name: fileName(path),
          mediaKind: detectMediaKind(path),
          sizeBytes,
          status: "pending" as QueueStatus,
          progress: 0,
          segmentCurrent: 0,
          segmentTotal: 0,
          resultText: "",
          resultSrt: "",
          rtfx: null,
          error: "",
        } satisfies QueueItem;
      }),
    );

    dispatch({ type: "add_queue_items", items: incoming });
    pushToast(`已加入队列 ${paths.length} 个文件`, "success");
  }, [dispatch, pushToast]);

  useEffect(() => {
    let unlisten: undefined | (() => void);

    getCurrentWindow()
      .onDragDropEvent((event: { payload: DragDropEvent }) => {
        const payload = event.payload;
        if (!payload) return;

        if (payload.type === "enter" || payload.type === "over") {
          patch({ dragActive: true });
        } else if (payload.type === "leave") {
          patch({ dragActive: false });
        } else if (payload.type === "drop") {
          patch({ dragActive: false });
          const paths = Array.isArray(payload.paths) ? payload.paths : [];
          void appendPaths(paths);
        }
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {
        // Drag-drop listener is optional, click-upload always works.
      });

    return () => {
      if (unlisten) unlisten();
    };
  }, [appendPaths, patch]);

  useEffect(() => {
    let unlistenProgress: undefined | (() => void);

    listen<TranscribeProgressEvent>("transcribe-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      dispatch({
        type: "patch_queue_item",
        id: payload.taskId,
        updater: (old) => ({
          ...old,
          segmentCurrent: Math.max(0, payload.currentSegment || 0),
          segmentTotal: Math.max(0, payload.totalSegments || 0),
          progress:
            payload.totalSegments > 0
              ? Math.min(99, Math.round((Math.max(0, payload.currentSegment || 0) / payload.totalSegments) * 100))
              : old.progress,
        }),
      });
    })
      .then((fn) => {
        unlistenProgress = fn;
      })
      .catch(() => {
        // Progress events are optional.
      });

    return () => {
      if (unlistenProgress) unlistenProgress();
    };
  }, [dispatch]);

  const pickFiles = useCallback(async () => {
    try {
      const picked = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "Media",
            extensions: ["mp3", "wav", "m4a", "mp4", "mkv", "flac", "aac", "mov", "webm", "avi"],
          },
        ],
      });

      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      await appendPaths(paths);
    } catch (error) {
      reportError(error, "pickFiles");
      pushToast(toUserErrorMessage(error, "打开文件选择器失败"), "error");
    }
  }, [appendPaths, pushToast]);

  const runTranscribe = useCallback(async (item: Pick<QueueItem, "id" | "path" | "name">) => {
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({
        ...old,
        status: "processing",
        progress: 15,
        segmentCurrent: 0,
        segmentTotal: 0,
        error: "",
      }),
    });
    patch({ activeId: item.id });

    try {
      const response = await invoke<TranscribeResponse>("transcribe", {
        request: {
          taskId: item.id,
          audioPath: item.path,
          provider: settings.provider,
          chunkTargetSeconds: settings.chunkTargetSeconds,
        },
      });
      const built = await invoke<BuildSegmentsResponse>("build_segments_from_words", {
        request: {
          audioPath: item.path,
          words: response.words,
        },
      });

      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          status: "done",
          progress: 100,
          segmentCurrent: response.segmentTotal > 0 ? response.segmentTotal : old.segmentCurrent,
          segmentTotal: response.segmentTotal > 0 ? response.segmentTotal : old.segmentTotal,
          resultText: built.text,
          resultSrt: built.srt,
          rtfx: response.rtfx,
          error: "",
        }),
      });
      pushToast(`已完成：${item.name}，SRT 已保存到 ${built.srtOutputPath}`, "success");
    } catch (err) {
      reportError(err, "runTranscribe");
      const errorMessage = toUserErrorMessage(err, "转录失败，请检查模型和运行时配置");
      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          status: "error",
          progress: 0,
          segmentCurrent: 0,
          segmentTotal: 0,
          error: errorMessage,
        }),
      });
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
    }
  }, [dispatch, patch, pushToast, settings.chunkTargetSeconds, settings.provider]);

  useEffect(() => {
    if (hasProcessingTask) return;
    const next = queue.find((item) => item.status === "queued");
    if (!next) return;
    void runTranscribe({ id: next.id, path: next.path, name: next.name });
  }, [hasProcessingTask, queue, runTranscribe]);

  const processQueue = useCallback(async () => {
    const pendingCount = queue.filter((item) => item.status === "pending").length;
    if (!pendingCount) {
      pushToast("没有待处理文件", "error");
      return;
    }

    const queuedIds = queue
      .filter((q) => q.status === "pending")
      .map((q) => q.id);

    for (const id of queuedIds) {
      dispatch({
        type: "patch_queue_item",
        id,
        updater: (old) => ({ ...old, status: "queued", progress: 0, segmentCurrent: 0, segmentTotal: 0, error: "" }),
      });
    }

    pushToast(`开始批量处理，共 ${pendingCount} 个文件`, "info");
  }, [dispatch, pushToast, queue]);

  const processSingle = useCallback(async (item: QueueItem) => {
    if (item.status === "processing" || item.status === "queued") return;
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({ ...old, status: "queued", progress: 0, segmentCurrent: 0, segmentTotal: 0, error: "" }),
    });

    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [dispatch, pushToast, queueBusy]);

  const clearQueue = useCallback(() => {
    if (queueBusy) {
      pushToast("正在处理时不能清空队列", "error");
      return;
    }
    dispatch({ type: "clear_queue" });
    pushToast("队列已清空", "info");
  }, [dispatch, pushToast, queueBusy]);

  const translateSingle = useCallback((item: QueueItem) => {
    patch({ activeId: item.id });
    pushToast(`转译排期中：${item.name}（功能即将接入）`, "info");
  }, [patch, pushToast]);

  const removeItem = useCallback((id: string) => {
    dispatch({ type: "remove_queue_item", id });
  }, [dispatch]);

  return {
    queueCount,
    queueBusy,
    appendPaths,
    pickFiles,
    processQueue,
    processSingle,
    clearQueue,
    translateSingle,
    removeItem,
  };
}
