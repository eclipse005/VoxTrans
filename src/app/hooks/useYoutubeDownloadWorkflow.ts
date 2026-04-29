import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  createEmptyTaskProgress,
  createTaskProgress,
  type QueueItem,
} from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { QueueRunMode } from "./queue/useQueueRunner";
import type { YoutubeDownloadProgressResponse } from "../api/youtube";
import { cancelYoutubeDownload, downloadYoutubeTask } from "../api/youtube";
import { addQueueItems, patchQueueItem, removeQueueItem } from "../state/queueDomainActions";
import { deleteTasks, registerTaskUpload } from "../api/workspace";
import { toUserErrorMessage } from "../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type YoutubeQueuedDownload = {
  taskId: string;
  url: string;
  createdAt: number;
};

type UseYoutubeDownloadWorkflowArgs = {
  queue: QueueItem[];
  dispatch: DispatchState;
  pushToast: PushToast;
  isTaskPresent: (taskId: string) => boolean;
  processSingleFromScheduler: (item: QueueItem) => Promise<void>;
  processSingleTranscribeTranslateFromScheduler: (item: QueueItem) => Promise<void>;
};

const YOUTUBE_PLACEHOLDER_PREFIX = "youtube://pending/";

function encodeYoutubePlaceholderPath(taskId: string, url: string): string {
  return `${YOUTUBE_PLACEHOLDER_PREFIX}${taskId}?url=${encodeURIComponent(url)}`;
}

function decodeYoutubeUrlFromPath(path: string): string {
  if (!path.startsWith(YOUTUBE_PLACEHOLDER_PREFIX)) return "";
  const queryIndex = path.indexOf("?");
  if (queryIndex < 0) return "";
  const query = path.slice(queryIndex + 1);
  const params = new URLSearchParams(query);
  return (params.get("url") || "").trim();
}

function isYoutubePlaceholderPath(path: string): boolean {
  return path.startsWith(YOUTUBE_PLACEHOLDER_PREFIX);
}

function normalizeTitle(raw: string): string {
  const text = (raw || "").trim();
  if (!text) return "";
  const slashNormalized = text.replace(/\\/g, "/");
  const base = slashNormalized.split("/").pop() || slashNormalized;
  const withoutExt = base.replace(/\.[a-zA-Z0-9]{2,5}$/u, "");
  return withoutExt.replace(/\.f\d+$/u, "").trim();
}

function createYoutubePlaceholderTask(
  taskId: string,
  path: string,
  name: string,
  sizeBytes: number,
  progress: number,
): QueueItem {
  return {
    id: taskId,
    path,
    name,
    mediaKind: "video",
    sizeBytes,
    transcribeStatus: "processing",
    taskProgress: createTaskProgress({
      code: "downloading",
      label: "下载中",
      detail: `${progress}%`,
      current: progress,
      total: 100,
    }),
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
  };
}

function isCancelledMessage(message: string): boolean {
  const value = message.toLowerCase();
  return value.includes("取消") || value.includes("cancel");
}

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value)));
}

function parseSizeToBytes(raw: string): number {
  const text = (raw || "").trim();
  if (!text) return 0;
  const matched = text.match(/^(\d+(?:\.\d+)?)\s*([a-zA-Z]+)$/);
  if (!matched) return 0;
  const value = Number(matched[1]);
  if (!Number.isFinite(value) || value <= 0) return 0;
  const unit = matched[2].toLowerCase();
  const factorMap: Record<string, number> = {
    b: 1,
    kb: 1000,
    mb: 1000 ** 2,
    gb: 1000 ** 3,
    tb: 1000 ** 4,
    kib: 1024,
    mib: 1024 ** 2,
    gib: 1024 ** 3,
    tib: 1024 ** 4,
  };
  const factor = factorMap[unit];
  if (!factor) return 0;
  return Math.round(value * factor);
}

export function useYoutubeDownloadWorkflow({
  queue,
  dispatch,
  pushToast,
  isTaskPresent,
  processSingleFromScheduler,
  processSingleTranscribeTranslateFromScheduler,
}: UseYoutubeDownloadWorkflowArgs) {
  const youtubeTrackedTaskIdsRef = useRef<Set<string>>(new Set());
  const youtubeRemovedTaskIdsRef = useRef<Set<string>>(new Set());
  const youtubeTaskUrlRef = useRef<Map<string, string>>(new Map());
  const youtubePostDownloadModeRef = useRef<Map<string, QueueRunMode>>(new Map());
  const runningYoutubeTaskIdRef = useRef("");
  const [youtubeDownloadQueue, setYoutubeDownloadQueue] = useState<YoutubeQueuedDownload[]>([]);

  useEffect(() => {
    for (const item of queue) {
      if (!isYoutubePlaceholderPath(item.path)) continue;
      youtubeTrackedTaskIdsRef.current.add(item.id);
      if (!youtubeTaskUrlRef.current.has(item.id)) {
        const restored = decodeYoutubeUrlFromPath(item.path);
        if (restored) youtubeTaskUrlRef.current.set(item.id, restored);
      }
    }
  }, [queue]);

  const applyYoutubeProgress = useCallback((payload: YoutubeDownloadProgressResponse) => {
    if (!payload.taskId) return;
    if (!youtubeTrackedTaskIdsRef.current.has(payload.taskId)) return;
    if (youtubeRemovedTaskIdsRef.current.has(payload.taskId)) return;
    const progress = clampPercent(payload.progressPercent || 0);
    const phase = (payload.phase || "").toLowerCase();
    const downloadingLike = phase === "starting" || phase === "downloading" || phase === "merging";
    const normalizedTitle = normalizeTitle(payload.title || "");
    const totalBytes = parseSizeToBytes(payload.totalSize || "");
    const hasMetadata = normalizedTitle.length > 0 && totalBytes > 0;

    if (!isTaskPresent(payload.taskId)) {
      if (!downloadingLike || !hasMetadata) return;
      const sourceUrl = youtubeTaskUrlRef.current.get(payload.taskId) || "";
      const placeholderPath = encodeYoutubePlaceholderPath(payload.taskId, sourceUrl);
      addQueueItems(dispatch, [
        createYoutubePlaceholderTask(
          payload.taskId,
          placeholderPath,
          normalizedTitle,
          totalBytes,
          progress,
        ),
      ]);
      return;
    }

    patchQueueItem(dispatch, payload.taskId, (item) => {
      const nextName = normalizedTitle || item.name;
      if (!downloadingLike) {
        return {
          ...item,
          name: nextName,
          sizeBytes: item.sizeBytes > 0 ? item.sizeBytes : totalBytes,
        };
      }
      return {
        ...item,
        name: nextName,
        sizeBytes: totalBytes > 0 ? totalBytes : item.sizeBytes,
        transcribeStatus: "processing",
        taskProgress: createTaskProgress({
          code: "downloading",
          label: "下载中",
          detail: `${progress}%`,
          current: progress,
          total: 100,
        }),
      };
    });
  }, [dispatch, isTaskPresent]);

  useEffect(() => {
    let disposed = false;
    let unlisten: undefined | (() => void);

    void listen<YoutubeDownloadProgressResponse>("youtube-download-progress", (event) => {
      if (disposed || !event.payload) return;
      applyYoutubeProgress(event.payload);
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    });

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [applyYoutubeProgress]);

  const runQueuedYoutubeDownload = useCallback(async (taskId: string, url: string) => {
    runningYoutubeTaskIdRef.current = taskId;

    if (isTaskPresent(taskId)) {
      patchQueueItem(dispatch, taskId, (item) => ({
        ...item,
        transcribeStatus: "processing",
        taskProgress: createTaskProgress({
          code: "downloading",
          label: "下载中",
          detail: "0%",
          current: 0,
          total: 100,
        }),
      }));
    }

    try {
      const response = await downloadYoutubeTask({ url, taskId });
      if (youtubeRemovedTaskIdsRef.current.has(taskId)) {
        return;
      }

      await registerTaskUpload({
        id: response.task.id,
        mediaPath: response.task.mediaPath,
        name: response.task.name,
        mediaKind: response.task.mediaKind,
        sizeBytes: response.task.sizeBytes,
      });

      youtubeTrackedTaskIdsRef.current.delete(taskId);
      youtubeTaskUrlRef.current.delete(taskId);
      if (isTaskPresent(taskId)) {
        patchQueueItem(dispatch, taskId, (item) => ({
          ...item,
          id: response.task.id,
          path: response.task.mediaPath,
          name: response.task.name,
          mediaKind: response.task.mediaKind,
          sizeBytes: response.task.sizeBytes,
          transcribeStatus: "pending",
          taskProgress: createEmptyTaskProgress(),
          transcribeError: "",
        }));
      } else {
        addQueueItems(dispatch, [{
          id: response.task.id,
          path: response.task.mediaPath,
          name: response.task.name,
          mediaKind: response.task.mediaKind,
          sizeBytes: response.task.sizeBytes,
          transcribeStatus: "pending",
          taskProgress: createEmptyTaskProgress(),
          transcribeError: "",
          resultText: "",
          resultSrt: "",
          subtitleSegmentsJson: "[]",
        }]);
      }
      pushToast("YouTube 下载完成，已加入任务列表", "success");

      const pendingMode = youtubePostDownloadModeRef.current.get(taskId);
      youtubePostDownloadModeRef.current.delete(taskId);
      if (pendingMode) {
        const downloadedItem: QueueItem = {
          id: response.task.id,
          path: response.task.mediaPath,
          name: response.task.name,
          mediaKind: response.task.mediaKind,
          sizeBytes: response.task.sizeBytes,
          transcribeStatus: "pending",
          taskProgress: createEmptyTaskProgress(),
          transcribeError: "",
          resultText: "",
          resultSrt: "",
          subtitleSegmentsJson: "[]",
        };
        if (pendingMode === "transcribe") {
          void processSingleFromScheduler(downloadedItem);
        } else {
          void processSingleTranscribeTranslateFromScheduler(downloadedItem);
        }
      }
    } catch (error) {
      const message = toUserErrorMessage(error, "YouTube 下载失败");
      const cancelled = isCancelledMessage(message) || youtubeRemovedTaskIdsRef.current.has(taskId);
      if (!cancelled && isTaskPresent(taskId)) {
        patchQueueItem(dispatch, taskId, (item) => ({
          ...item,
          transcribeStatus: "error",
          taskProgress: createEmptyTaskProgress(),
          transcribeError: message,
        }));
        pushToast(message, "error");
      } else if (!cancelled) {
        pushToast(message, "error");
      }
    } finally {
      if (runningYoutubeTaskIdRef.current === taskId) {
        runningYoutubeTaskIdRef.current = "";
      }
      setYoutubeDownloadQueue((prev) => prev.filter((item) => item.taskId !== taskId));
    }
  }, [dispatch, isTaskPresent, processSingleFromScheduler, processSingleTranscribeTranslateFromScheduler, pushToast]);

  useEffect(() => {
    if (runningYoutubeTaskIdRef.current) return;
    if (youtubeDownloadQueue.length === 0) return;
    const next = [...youtubeDownloadQueue].sort((a, b) => a.createdAt - b.createdAt)[0];
    if (!next) return;
    void runQueuedYoutubeDownload(next.taskId, next.url);
  }, [runQueuedYoutubeDownload, youtubeDownloadQueue]);

  const downloadYoutube = useCallback(async (url: string) => {
    const trimmed = url.trim();
    if (!trimmed) {
      pushToast("请先输入 YouTube 链接", "info");
      return;
    }

    const taskId = `yt-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    youtubeRemovedTaskIdsRef.current.delete(taskId);
    youtubeTrackedTaskIdsRef.current.add(taskId);
    youtubeTaskUrlRef.current.set(taskId, trimmed);

    setYoutubeDownloadQueue((prev) => ([
      ...prev,
      {
        taskId,
        url: trimmed,
        createdAt: Date.now(),
      },
    ]));

    pushToast("已加入 YouTube 下载队列", "info");
    return Promise.resolve();
  }, [pushToast]);

  const enqueueYoutubeRetry = useCallback((taskId: string, mode: QueueRunMode) => {
    const url = youtubeTaskUrlRef.current.get(taskId);
    if (!url) {
      pushToast("缺少下载链接，无法重试。请删除后重新添加。", "error");
      return;
    }
    if (runningYoutubeTaskIdRef.current === taskId) {
      pushToast("该任务正在下载中", "info");
      return;
    }
    if (youtubeDownloadQueue.some((item) => item.taskId === taskId)) {
      pushToast("该任务已在下载队列中", "info");
      return;
    }

    youtubeTrackedTaskIdsRef.current.add(taskId);
    youtubePostDownloadModeRef.current.set(taskId, mode);
    patchQueueItem(dispatch, taskId, (item) => ({
      ...item,
      transcribeStatus: "queued",
      taskProgress: createTaskProgress({
        code: "downloading",
        label: "下载中",
        detail: "排队中",
        current: 0,
        total: 100,
      }),
      transcribeError: "",
    }));
    setYoutubeDownloadQueue((prev) => ([
      ...prev,
      {
        taskId,
        url,
        createdAt: Date.now(),
      },
    ]));
    pushToast("已加入下载重试队列", "info");
  }, [dispatch, pushToast, youtubeDownloadQueue]);

  const processSingle = useCallback(async (item: QueueItem): Promise<boolean> => {
    const isYoutubeTask = youtubeTrackedTaskIdsRef.current.has(item.id) || isYoutubePlaceholderPath(item.path);
    if (!isYoutubeTask) {
      return false;
    }
    if (!youtubeTaskUrlRef.current.has(item.id)) {
      const restored = decodeYoutubeUrlFromPath(item.path);
      if (restored) youtubeTaskUrlRef.current.set(item.id, restored);
    }
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") {
      pushToast("该任务正在下载中", "info");
      return true;
    }
    enqueueYoutubeRetry(item.id, "transcribe");
    return true;
  }, [enqueueYoutubeRetry, pushToast]);

  const processSingleTranscribeTranslate = useCallback(async (item: QueueItem): Promise<boolean> => {
    const isYoutubeTask = youtubeTrackedTaskIdsRef.current.has(item.id) || isYoutubePlaceholderPath(item.path);
    if (!isYoutubeTask) {
      return false;
    }
    if (!youtubeTaskUrlRef.current.has(item.id)) {
      const restored = decodeYoutubeUrlFromPath(item.path);
      if (restored) youtubeTaskUrlRef.current.set(item.id, restored);
    }
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") {
      pushToast("该任务正在下载中", "info");
      return true;
    }
    enqueueYoutubeRetry(item.id, "transcribe_translate");
    return true;
  }, [enqueueYoutubeRetry, pushToast]);

  const removeItem = useCallback(async (id: string): Promise<boolean> => {
    const target = queue.find((item) => item.id === id) || null;
    const isYoutubeTask = youtubeTrackedTaskIdsRef.current.has(id)
      || (target ? isYoutubePlaceholderPath(target.path) : false);
    if (!isYoutubeTask) {
      return false;
    }

    youtubeRemovedTaskIdsRef.current.add(id);
    setYoutubeDownloadQueue((prev) => prev.filter((item) => item.taskId !== id));

    if (runningYoutubeTaskIdRef.current === id) {
      try {
        await cancelYoutubeDownload(id);
      } catch {
        // Ignore cancellation failure if process already exited.
      }
    }

    try {
      await deleteTasks({
        taskId: id,
        mediaPath: target?.path || null,
      });
    } catch {
      // Keep local removal responsive even if DB cleanup fails.
    }
    removeQueueItem(dispatch, id);
    youtubeTrackedTaskIdsRef.current.delete(id);
    youtubeTaskUrlRef.current.delete(id);
    youtubePostDownloadModeRef.current.delete(id);
    return true;
  }, [dispatch, queue]);

  const clearYoutubeQueue = useCallback(async () => {
    const runningTaskId = runningYoutubeTaskIdRef.current;
    const trackedIds = [...youtubeTrackedTaskIdsRef.current];
    for (const taskId of trackedIds) {
      youtubeRemovedTaskIdsRef.current.add(taskId);
    }
    setYoutubeDownloadQueue([]);
    youtubeTrackedTaskIdsRef.current.clear();
    youtubeTaskUrlRef.current.clear();
    youtubePostDownloadModeRef.current.clear();

    if (runningTaskId) {
      try {
        await cancelYoutubeDownload(runningTaskId);
      } catch {
        // Ignore cancellation failure if process already exited.
      }
    }
  }, []);

  return {
    downloadYoutube,
    processSingle,
    processSingleTranscribeTranslate,
    removeItem,
    clearYoutubeQueue,
  };
}
