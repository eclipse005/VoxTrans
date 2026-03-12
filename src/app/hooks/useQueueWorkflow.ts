import { useCallback, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import { cuesToSrt } from "../../features/media/srt";
import type {
  BuildSegmentsResponse,
  QueueItem,
  SavedSettings,
  SubtitleSegment,
  TranscribeResponse,
} from "../../features/media/types";
import { detectMediaKind, fileName } from "../../features/media/utils";
import type { AppAction } from "../state/appReducer";
import type { HotwordCorrection } from "../types";
import {
  correctSegmentsWithHotwords,
  shouldRunHotwordCorrection,
  type TimedHotwordSegment,
} from "../utils/hotwordCorrection";
import { countSuspiciousPunctuationSentences, restorePunctuationOnWords } from "../utils/punctuationCorrection";
import { reportError, toUserErrorMessage } from "../utils/errors";

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
  llmSettings: {
    apiKey: string;
    apiBase: string;
    apiModel: string;
  };
  hotwordCorrection: HotwordCorrection;
  dispatch: DispatchState;
  pushToast: PushToast;
};

export function useQueueWorkflow({
  queue,
  settings,
  llmSettings,
  hotwordCorrection,
  dispatch,
  pushToast,
}: UseQueueWorkflowArgs) {
  const queueCount = queue.length;
  const hasProcessingTask = useMemo(() => queue.some((item) => item.transcribeStatus === "processing"), [queue]);
  const hasQueuedTask = useMemo(() => queue.some((item) => item.transcribeStatus === "queued"), [queue]);
  const queueBusy = hasProcessingTask || hasQueuedTask;

  const appendTaskLog = useCallback(async (
    channel: "main" | "llm",
    item: Pick<QueueItem, "id" | "path">,
    eventType: string,
    payload?: Record<string, unknown>,
  ) => {
    try {
      await invoke("append_task_log", {
        request: {
          taskId: item.id,
          mediaPath: item.path,
          channel,
          message: formatTaskLogLine(eventType, payload),
        },
      });
    } catch {
      // Log write failures must not affect core workflow.
    }
  }, []);

  const recordLlmUsage = useCallback(async (
    item: Pick<QueueItem, "id">,
    stage: "punctuation" | "hotword",
    usage: {
      promptTokens?: number;
      completionTokens?: number;
      totalTokens?: number;
    },
  ) => {
    const promptTokens = Math.max(0, Math.round(usage.promptTokens ?? 0));
    const completionTokens = Math.max(0, Math.round(usage.completionTokens ?? 0));
    const totalTokens = Math.max(0, Math.round(usage.totalTokens ?? (promptTokens + completionTokens)));
    if (promptTokens <= 0 && completionTokens <= 0 && totalTokens <= 0) return;
    try {
      await invoke("record_task_llm_usage", {
        request: {
          taskId: item.id,
          stage,
          promptTokens,
          completionTokens,
          totalTokens,
        },
      });
    } catch {
      // Usage stats failures must not affect workflow.
    }
  }, []);

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
          translateStatus: "idle",
          translateProgress: 0,
          translateError: "",
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

      // Fallback for environments that already report logical coordinates.
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
      .catch(() => {
        // Drag-drop listener is optional, click-upload always works.
      });

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
      .catch(() => {
        // Progress events are optional.
      });

    return () => {
      disposed = true;
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

      const response = await invoke<TranscribeResponse>("transcribe", {
        request: {
          taskId: item.id,
          audioPath: item.path,
          provider: settings.provider,
          chunkTargetSeconds: settings.chunkTargetSeconds,
        },
      });
      void appendTaskLog("main", item, "transcribe.asr.completed", {
        segmentTotal: response.segmentTotal,
        audioDurationSec: round2(response.audioDurationSec),
        transcribeElapsedSec: round2(response.transcribeElapsedSec),
        executionProvider: response.executionProvider,
        segmentDurationsSec: Array.isArray(response.segmentDurationsSec)
          ? response.segmentDurationsSec.map(round2)
          : [],
      });
      let wordsForBuild = response.words;
      if (settings.autoPunc) {
        const hasLlmConfig = Boolean(llmSettings.apiKey.trim() && llmSettings.apiModel.trim());
        if (!hasLlmConfig) {
          void appendTaskLog("main", item, "punc.skipped", {
            reason: "自动标点增强已启用，但未配置 LLM Key/Model，已跳过",
          });
        } else {
          const scan = countSuspiciousPunctuationSentences(response.words);
          if (scan.suspiciousCount > 0) {
            dispatch({
              type: "patch_queue_item",
              id: item.id,
              updater: (old) => ({
                ...old,
                transcribePhase: "punctuation",
              }),
            });
            void appendTaskLog("main", item, "punc.started", {
              sentenceTotal: scan.sentenceTotal,
              suspiciousCount: scan.suspiciousCount,
            });
          } else {
            void appendTaskLog("main", item, "punc.skipped", {
              reason: "未检测到可疑句，跳过标点恢复",
              sentenceTotal: scan.sentenceTotal,
              suspiciousCount: 0,
            });
          }
          if (scan.suspiciousCount > 0) {
            try {
            let puncRound = 0;
            const restored = await restorePunctuationOnWords({
              words: response.words,
              llm: llmSettings,
              invokeLlm: async (request) => {
                puncRound += 1;
                const safeRequest = { ...request, apiKey: "[redacted]" };
                await appendTaskLog("llm", item, "punc.llm.request", {
                  round: puncRound,
                  request: safeRequest,
                });
                const llmResponse = await invoke<{
                  status: "completed" | "requires_tool";
                  message?: string;
                  promptTokens?: number;
                  completionTokens?: number;
                  totalTokens?: number;
                  toolCalls: Array<{
                    id: string;
                    type: string;
                    function: {
                      name: string;
                      arguments: string;
                    };
                  }>;
                }>("llm_interact", { request });
                await appendTaskLog("llm", item, "punc.llm.response", {
                  round: puncRound,
                  response: llmResponse,
                });
                await recordLlmUsage(item, "punctuation", {
                  promptTokens: llmResponse.promptTokens,
                  completionTokens: llmResponse.completionTokens,
                  totalTokens: llmResponse.totalTokens,
                });
                return llmResponse;
              },
            });
            wordsForBuild = restored.words;
            void appendTaskLog("main", item, "punc.completed", {
              sentenceTotal: restored.sentenceTotal,
              suspiciousCount: restored.suspiciousCount,
              restoredCount: restored.restoredCount,
              acceptedCount: restored.acceptedCount,
              rejectedCount: restored.rejectedCount,
            });
            } catch (puncErr) {
              reportError(puncErr, "runPunctuationCorrection");
              void appendTaskLog("main", item, "punc.failed", {
                error: toUserErrorMessage(puncErr, "自动标点增强失败，已保留原始转录"),
              });
            }
          }
        }
      }
      const built = await invoke<BuildSegmentsResponse>("build_segments_from_words", {
        request: {
          taskId: item.id,
          audioPath: item.path,
          words: wordsForBuild,
        },
      });
      const baseSegments = toTimedHotwordSegments(built.segments);
      let finalSegments = baseSegments;
      let finalText = built.text;
      let finalSrt = built.srt;
      let hotwordError = "";

      if (shouldRunHotwordCorrection(hotwordCorrection)) {
        const hasLlmConfig = Boolean(llmSettings.apiKey.trim() && llmSettings.apiModel.trim());
        if (hasLlmConfig) {
          void appendTaskLog("main", item, "hotword.started", {
            groupId: hotwordCorrection.activeGroupId,
          });
          dispatch({
            type: "patch_queue_item",
            id: item.id,
            updater: (old) => ({
              ...old,
              transcribePhase: "hotword",
            }),
          });
          try {
            let llmRound = 0;
            const corrected = await correctSegmentsWithHotwords({
              segments: baseSegments,
              config: hotwordCorrection,
              llm: llmSettings,
              invokeLlm: async (request) => {
                llmRound += 1;
                const safeRequest = { ...request, apiKey: "[redacted]" };
                await appendTaskLog("llm", item, "llm.request", {
                  round: llmRound,
                  request: safeRequest,
                });
                const response = await invoke<{
                  status: "completed" | "requires_tool";
                  message?: string;
                  promptTokens?: number;
                  completionTokens?: number;
                  totalTokens?: number;
                  toolCalls: Array<{
                    id: string;
                    type: string;
                    function: {
                      name: string;
                      arguments: string;
                    };
                  }>;
                }>("llm_interact", { request });
                await appendTaskLog("llm", item, "llm.response", {
                  round: llmRound,
                  response,
                });
                await recordLlmUsage(item, "hotword", {
                  promptTokens: response.promptTokens,
                  completionTokens: response.completionTokens,
                  totalTokens: response.totalTokens,
                });
                return response;
              },
            });
            finalSegments = corrected.segments;
            finalText = toPlainText(toSubtitleSegments(finalSegments));
            finalSrt = toSrtFromSegments(toSubtitleSegments(finalSegments));
            const hotwordReport = buildHotwordReport(corrected.changedCount, corrected.replacementStats);
            void appendTaskLog("main", item, "hotword.completed", {
              changedCount: corrected.changedCount,
              replacements: corrected.replacementStats,
              report: hotwordReport,
            });
          } catch (hotwordErr) {
            reportError(hotwordErr, "runHotwordCorrection");
            hotwordError = toUserErrorMessage(hotwordErr, "热词矫正失败，已保留原始转录");
            void appendTaskLog("main", item, "hotword.failed", { error: hotwordError });
          }
        } else {
          hotwordError = "热词矫正已启用，但未配置 LLM Key/Model，已跳过";
          void appendTaskLog("main", item, "hotword.skipped", { reason: hotwordError });
        }
      }

      await invoke("save_srt", {
        request: {
          outputPath: built.srtOutputPath,
          content: finalSrt,
        },
      });

      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          subtitleSegmentsJson: JSON.stringify(toSubtitleSegments(finalSegments)),
          transcribeStatus: "done",
          transcribeProgress: 100,
          transcribeSegmentCurrent: response.segmentTotal > 0 ? response.segmentTotal : old.transcribeSegmentCurrent,
          transcribeSegmentTotal: response.segmentTotal > 0 ? response.segmentTotal : old.transcribeSegmentTotal,
          transcribePhase: "",
          resultText: finalText,
          resultSrt: finalSrt,
          transcribeError: "",
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
    hotwordCorrection,
    llmSettings,
    pushToast,
    appendTaskLog,
    recordLlmUsage,
    settings.autoPunc,
    settings.chunkTargetSeconds,
    settings.provider,
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
      // Queue state updates are UI-level events; avoid writing them into main task flow log.
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
      await invoke("delete_task_summaries", {
        request: { taskId: null, mediaPath: null },
      });
    } catch {
      // Queue is already cleared in UI; ignore history cleanup failure.
    }
    pushToast("队列已清空", "info");
  }, [dispatch, pushToast, queueBusy]);

  const translateSingle = useCallback((item: QueueItem) => {
    dispatch({ type: "set_ui", payload: { activeId: item.id } });
    const needsTranscribeFirst = item.transcribeStatus !== "done";
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({
        ...old,
        translateStatus: "queued",
        translateProgress: 0,
        translateError: "",
        transcribeStatus:
          needsTranscribeFirst && (old.transcribeStatus === "pending" || old.transcribeStatus === "error")
            ? "queued"
            : old.transcribeStatus,
        transcribePhase:
          needsTranscribeFirst && (old.transcribeStatus === "pending" || old.transcribeStatus === "error")
            ? ""
            : old.transcribePhase,
        transcribeError:
          needsTranscribeFirst && (old.transcribeStatus === "pending" || old.transcribeStatus === "error")
            ? ""
            : old.transcribeError,
      }),
    });
    if (needsTranscribeFirst) {
      pushToast(`已加入转译队列：先转录再翻译（${item.name}）`, "info");
    } else {
      pushToast(`转译排期中：${item.name}（功能即将接入）`, "info");
    }
  }, [dispatch, pushToast]);

  const removeItem = useCallback((id: string) => {
    const item = queue.find((q) => q.id === id);
    if (item) {
      void invoke("delete_task_summaries", {
        request: { taskId: item.id, mediaPath: item.path },
      });
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
    translateSingle,
    removeItem,
  };
}

function toTimedHotwordSegments(segments: BuildSegmentsResponse["segments"]): TimedHotwordSegment[] {
  return segments.map((segment) => ({
    startMs: Math.max(0, Math.round(segment.start * 1000)),
    endMs: Math.max(0, Math.round(segment.end * 1000)),
    sourceText: segment.text ?? "",
    translatedText: "",
    words: (segment.words ?? []).map((word) => ({
      start: word.start,
      end: word.end,
      word: word.word ?? "",
    })),
  }));
}

function toSubtitleSegments(segments: TimedHotwordSegment[]): SubtitleSegment[] {
  return segments.map((segment) => ({
    startMs: segment.startMs,
    endMs: segment.endMs,
    sourceText: segment.sourceText,
    translatedText: segment.translatedText,
  }));
}

function toPlainText(segments: SubtitleSegment[]): string {
  return segments.map((segment) => segment.sourceText.trim()).filter(Boolean).join(" ");
}

function toSrtFromSegments(segments: SubtitleSegment[]): string {
  return cuesToSrt(
    segments.map((segment, index) => ({
      id: `seg-${index}-${segment.startMs}`,
      startMs: Math.max(0, Math.round(segment.startMs)),
      endMs: Math.max(Math.round(segment.startMs), Math.round(segment.endMs)),
      text: segment.sourceText ?? "",
      translatedText: segment.translatedText ?? "",
    })),
  );
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

function buildHotwordReport(
  changedCount: number,
  replacementStats: Array<{ oldText: string; newText: string; count: number }>,
): string {
  if (changedCount <= 0) {
    return "矫正完成: 0 处修改";
  }

  const lines = [`矫正完成: ${changedCount} 处修改`, ""];
  const stats = replacementStats.length > 0
    ? replacementStats
    : [{ oldText: "（未知）", newText: "（未知）", count: changedCount }];
  for (const stat of stats) {
    lines.push(`  ${stat.oldText} -> ${stat.newText}: ${stat.count} 处`);
  }
  return lines.join("\n");
}





