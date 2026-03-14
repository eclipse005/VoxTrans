import { useCallback, useEffect, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import {
  appendTaskLog as appendTaskLogApi,
  getFileSize,
  runPostAsrPipeline,
  saveSrt,
  transcribeMedia,
} from "../api/transcribe";
import { deleteTaskSummaries } from "../api/workspace";
import type {
  BuildSegmentsResponse,
  QueueItem,
  SavedSettings,
  SubtitleSegment,
} from "../../features/media/types";
import { detectMediaKind, fileName } from "../../features/media/utils";
import type { AppAction } from "../state/appReducer";
import { reportError, toUserErrorMessage } from "../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type TranscribeProgressEvent = {
  taskId: string;
  currentSegment: number;
  totalSegments: number;
};

type TranscribePhaseEvent = {
  taskId: string;
  phase: "segment";
};

type UseQueueWorkflowArgs = {
  queue: QueueItem[];
  settings: SavedSettings;
  dispatch: DispatchState;
  pushToast: PushToast;
};

export function useQueueWorkflow({
  queue,
  settings,
  dispatch,
  pushToast,
}: UseQueueWorkflowArgs) {
  const queueCount = queue.length;
  const hasProcessingTask = useMemo(() => queue.some((item) => item.transcribeStatus === "processing"), [queue]);
  const hasQueuedTask = useMemo(() => queue.some((item) => item.transcribeStatus === "queued"), [queue]);
  const queueBusy = hasProcessingTask || hasQueuedTask;

  const appendTaskLog = useCallback(async (
    channel: "main",
    item: Pick<QueueItem, "id" | "path">,
    eventType: string,
    payload?: Record<string, unknown>,
  ) => {
    try {
      await appendTaskLogApi({
        taskId: item.id,
        mediaPath: item.path,
        channel,
        message: formatTaskLogLine(eventType, payload),
      });
    } catch (error) {
      // Log write failures must not affect core workflow.
      reportError(error, "appendTaskLog");
    }
  }, []);

  const appendPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;

    const incoming = await Promise.all(
      paths.map(async (path) => {
        let sizeBytes = 0;
        try {
          sizeBytes = await getFileSize(path);
        } catch {
          sizeBytes = 0;
        }

        return {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
          path,
          name: fileName(path),
          mediaKind: detectMediaKind(path),
          sizeBytes,
          transcribeStatus: "pending",
          transcribeProgress: 0,
          transcribeSegmentCurrent: 0,
          transcribeSegmentTotal: 0,
          transcribePhase: "",
          transcribeError: "",
          resultText: "",
          resultSrt: "",
          subtitleSegmentsJson: "[]",
        } satisfies QueueItem;
      }),
    );

    dispatch({ type: "add_queue_items", items: incoming });
    pushToast(`已加入队列 ${paths.length} 个文件`, "success");
  }, [dispatch, pushToast]);

  useEffect(() => {
    let disposed = false;
    let unlisten: undefined | (() => void);
    let scaleFactor = 1;

    void getCurrentWindow()
      .scaleFactor()
      .then((value) => {
        if (!disposed && Number.isFinite(value) && value > 0) {
          scaleFactor = value;
        }
      })
      .catch(() => {});

    const isInsideUploadArea = (position: { x: number; y: number }) => {
      const zone = document.querySelector(".upload-panel-content.active .upload-area");
      if (!(zone instanceof HTMLElement)) return false;
      const rect = zone.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return false;

      const logicalX = position.x / scaleFactor;
      const logicalY = position.y / scaleFactor;
      const insideLogical = logicalX >= rect.left && logicalX <= rect.right && logicalY >= rect.top && logicalY <= rect.bottom;
      if (insideLogical) return true;

      return position.x >= rect.left && position.x <= rect.right && position.y >= rect.top && position.y <= rect.bottom;
    };

    getCurrentWindow()
      .onDragDropEvent((event: { payload: DragDropEvent }) => {
        const payload = event.payload;
        if (!payload) return;

        if (payload.type === "enter" || payload.type === "over") {
          const inside = isInsideUploadArea(payload.position);
          dispatch({ type: "set_ui", payload: { dragActive: inside } });
        } else if (payload.type === "leave") {
          dispatch({ type: "set_ui", payload: { dragActive: false } });
        } else if (payload.type === "drop") {
          dispatch({ type: "set_ui", payload: { dragActive: false } });
          if (!isInsideUploadArea(payload.position)) return;
          const paths = Array.isArray(payload.paths) ? payload.paths : [];
          void appendPaths(paths);
        }
      })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [appendPaths, dispatch]);

  useEffect(() => {
    let disposed = false;
    let unlistenProgress: undefined | (() => void);

    listen<TranscribeProgressEvent>("transcribe-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      dispatch({
        type: "patch_queue_item",
        id: payload.taskId,
        updater: (old) => ({
          ...old,
          transcribeSegmentCurrent: Math.max(0, payload.currentSegment || 0),
          transcribeSegmentTotal: Math.max(0, payload.totalSegments || 0),
          transcribePhase: "recognizing",
          transcribeProgress:
            payload.totalSegments > 0
              ? Math.min(99, Math.round((Math.max(0, payload.currentSegment || 0) / payload.totalSegments) * 100))
              : old.transcribeProgress,
        }),
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenProgress = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlistenProgress) unlistenProgress();
    };
  }, [dispatch]);

  useEffect(() => {
    let disposed = false;
    let unlistenPhase: undefined | (() => void);
    listen<TranscribePhaseEvent>("transcribe-phase", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      dispatch({
        type: "patch_queue_item",
        id: payload.taskId,
        updater: (old) => ({
          ...old,
          transcribePhase: payload.phase || old.transcribePhase,
        }),
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenPhase = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      if (unlistenPhase) unlistenPhase();
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

  const runTranscribe = useCallback(async (item: QueueItem) => {
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({
        ...old,
        transcribeStatus: "processing",
        transcribeProgress: 0,
        transcribeSegmentCurrent: 0,
        transcribeSegmentTotal: 0,
        transcribePhase: "initializing",
        transcribeError: "",
      }),
    });

    try {
      void appendTaskLog("main", item, "transcribe.started", {
        chunkTargetSeconds: settings.chunkTargetSeconds,
        provider: settings.provider,
        mediaPath: item.path,
      });

      const response = await transcribeMedia({
        taskId: item.id,
        audioPath: item.path,
        provider: settings.provider,
        chunkTargetSeconds: settings.chunkTargetSeconds,
      });
      void appendTaskLog("main", item, "transcribe.asr.completed", {
        segmentTotal: response.segmentTotal,
        audioDurationSec: round2(response.audioDurationSec),
        transcribeElapsedSec: round2(response.transcribeElapsedSec),
        executionProvider: response.executionProvider,
      });
      const processed = await runPostAsrPipeline({
        taskId: item.id,
        audioPath: item.path,
        words: response.words,
        subtitleMaxWordsPerSegment: settings.subtitleMaxWordsPerSegment,
      });

      await saveSrt({
        outputPath: processed.srtOutputPath,
        content: processed.srt,
      });

      const normalizedSegments = toSubtitleSegmentsFromBuilt(processed.segments);
      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          subtitleSegmentsJson: JSON.stringify(normalizedSegments),
          transcribeStatus: "done",
          transcribeProgress: 100,
          transcribeSegmentCurrent: response.segmentTotal > 0 ? response.segmentTotal : old.transcribeSegmentCurrent,
          transcribeSegmentTotal: response.segmentTotal > 0 ? response.segmentTotal : old.transcribeSegmentTotal,
          transcribePhase: "",
          resultText: processed.text,
          resultSrt: processed.srt,
          transcribeError: "",
        }),
      });
      pushToast(`已完成：${item.name}，SRT 已保存到 ${processed.srtOutputPath}`, "success");
    } catch (err) {
      reportError(err, "runTranscribe");
      const errorMessage = toUserErrorMessage(err, "转录失败，请检查模型和运行时配置");
      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          transcribeStatus: "error",
          transcribeProgress: 0,
          transcribeSegmentCurrent: 0,
          transcribeSegmentTotal: 0,
          transcribePhase: "",
          transcribeError: errorMessage,
        }),
      });
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
      void appendTaskLog("main", item, "transcribe.failed", { error: errorMessage });
    }
  }, [
    dispatch,
    pushToast,
    appendTaskLog,
    settings.chunkTargetSeconds,
    settings.provider,
    settings.subtitleMaxWordsPerSegment,
  ]);

  useEffect(() => {
    if (hasProcessingTask) return;
    const next = queue.find((item) => item.transcribeStatus === "queued");
    if (!next) return;
    void runTranscribe(next);
  }, [hasProcessingTask, queue, runTranscribe]);

  const processQueue = useCallback(async () => {
    const pendingCount = queue.filter((item) => item.transcribeStatus === "pending").length;
    if (!pendingCount) {
      pushToast("没有待处理文件", "error");
      return;
    }

    const queuedIds = queue
      .filter((q) => q.transcribeStatus === "pending")
      .map((q) => q.id);

    for (const id of queuedIds) {
      dispatch({
        type: "patch_queue_item",
        id,
        updater: (old) => ({
          ...old,
          transcribeStatus: "queued",
          transcribeProgress: 0,
          transcribeSegmentCurrent: 0,
          transcribeSegmentTotal: 0,
          transcribePhase: "",
          transcribeError: "",
        }),
      });
    }

    pushToast(`开始批量处理，共 ${pendingCount} 个文件`, "info");
  }, [dispatch, pushToast, queue]);

  const processSingle = useCallback(async (item: QueueItem) => {
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({
        ...old,
        transcribeStatus: "queued",
        transcribeProgress: 0,
        transcribeSegmentCurrent: 0,
        transcribeSegmentTotal: 0,
        transcribePhase: "",
        transcribeError: "",
      }),
    });
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [dispatch, pushToast, queueBusy]);

  const clearQueue = useCallback(async () => {
    if (queueBusy) {
      pushToast("正在处理时不能清空队列", "error");
      return;
    }
    dispatch({ type: "clear_queue" });
    try {
      await deleteTaskSummaries({ taskId: null, mediaPath: null });
    } catch {
      // Queue is already cleared in UI; ignore history cleanup failure.
    }
    pushToast("队列已清空", "info");
  }, [dispatch, pushToast, queueBusy]);

  const removeItem = useCallback((id: string) => {
    const item = queue.find((q) => q.id === id);
    if (item) {
      void deleteTaskSummaries({ taskId: item.id, mediaPath: item.path });
    }
    dispatch({ type: "remove_queue_item", id });
  }, [dispatch, queue]);

  return {
    queueCount,
    queueBusy,
    appendPaths,
    pickFiles,
    processQueue,
    processSingle,
    clearQueue,
    removeItem,
  };
}

function toSubtitleSegmentsFromBuilt(segments: BuildSegmentsResponse["segments"]): SubtitleSegment[] {
  return segments.map((segment) => ({
    startMs: Math.max(0, Math.round(segment.start * 1000)),
    endMs: Math.max(0, Math.round(segment.end * 1000)),
    sourceText: segment.text ?? "",
    translatedText: "",
  }));
}

function formatTaskLogLine(eventType: string, payload?: Record<string, unknown>): string {
  if (!payload || Object.keys(payload).length === 0) {
    return eventType;
  }
  return `${eventType}\n${JSON.stringify(payload, null, 2)}`;
}

function round2(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.round(value * 100) / 100;
}
