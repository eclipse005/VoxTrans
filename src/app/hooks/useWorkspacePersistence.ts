import { useEffect, useRef } from "react";
import { listTaskSummaries } from "../api/history";
import { loadWorkspaceState, saveQueueState } from "../api/workspace";
import type { QueueItem, SubtitleSegment, TaskSummary, TranscribeStatus } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";

type DispatchState = (action: AppAction) => void;

type UseWorkspacePersistenceArgs = {
  queue: QueueItem[];
  dispatch: DispatchState;
};

export function useWorkspacePersistence({
  queue,
  dispatch,
}: UseWorkspacePersistenceArgs) {
  const hydratedRef = useRef(false);
  const queueSaveTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await loadWorkspaceState();
        const history: TaskSummary[] = await listTaskSummaries({ limit: 500 });
        if (cancelled) return;

        const queueItems = Array.isArray(res.queue) ? res.queue.map(normalizeQueueItem) : [];
        const historyItems = Array.isArray(history)
          ? history.map(taskSummaryToQueueItem)
          : [];

        const merged = dedupeById([...queueItems, ...historyItems]);
        if (merged.length > 0) {
          dispatch({ type: "add_queue_items", items: merged });
          dispatch({ type: "set_ui", payload: { activeId: merged[0]?.id || "" } });
        }
      } finally {
        hydratedRef.current = true;
      }
    })();

    return () => {
      cancelled = true;
      if (queueSaveTimerRef.current != null) {
        window.clearTimeout(queueSaveTimerRef.current);
      }
    };
  }, [dispatch]);

  useEffect(() => {
    if (!hydratedRef.current) return;
    if (queueSaveTimerRef.current != null) {
      window.clearTimeout(queueSaveTimerRef.current);
    }

    queueSaveTimerRef.current = window.setTimeout(() => {
      void saveQueueState({ queue });
    }, 300);
  }, [queue]);
}

function taskSummaryToQueueItem(task: TaskSummary): QueueItem {
  const transcribeStatus = normalizeTranscribeStatus(task.transcribeStatus || task.lastStatus);
  const segments = parseSubtitleSegments(task.subtitleSegmentsJson);
  const resultText = segments.map((segment) => segment.sourceText.trim()).filter(Boolean).join("\n");
  return recoverTransientStates({
    id: task.id,
    path: task.mediaPath,
    name: task.name,
    mediaKind: task.mediaKind,
    sizeBytes: Math.max(0, task.sizeBytes || 0),
    transcribeStatus,
    transcribeProgress: transcribeStatus === "done" ? 100 : 0,
    transcribeSegmentCurrent: 0,
    transcribeSegmentTotal: 0,
    transcribePhase: "",
    transcribeError: task.transcribeError || task.lastError || "",
    resultText,
    resultSrt: task.transcriptSrt || "",
    subtitleSegmentsJson: JSON.stringify(segments),
  });
}

function normalizeTranscribeStatus(value: string): TranscribeStatus {
  switch (value) {
    case "queued":
    case "processing":
    case "done":
    case "error":
    case "pending":
      return value;
    default:
      return "pending";
  }
}

function dedupeById(items: QueueItem[]): QueueItem[] {
  const seen = new Set<string>();
  const deduped: QueueItem[] = [];
  for (const item of items) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    deduped.push(item);
  }
  return deduped;
}

function normalizeQueueItem(item: QueueItem): QueueItem {
  return recoverTransientStates({
    ...item,
    transcribeStatus: normalizeTranscribeStatus(item.transcribeStatus),
    transcribeProgress: clampProgress(item.transcribeProgress),
    transcribeSegmentCurrent: Math.max(0, item.transcribeSegmentCurrent || 0),
    transcribeSegmentTotal: Math.max(0, item.transcribeSegmentTotal || 0),
    transcribePhase: normalizeTranscribePhase(item.transcribePhase),
    transcribeError: item.transcribeError || "",
    subtitleSegmentsJson: normalizeSubtitleSegmentsJson(item.subtitleSegmentsJson),
  });
}

function normalizeTranscribePhase(value: unknown): QueueItem["transcribePhase"] {
  switch (value) {
    case "initializing":
    case "recognizing":
    case "segment":
      return value;
    default:
      return "";
  }
}

function clampProgress(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value)));
}

function normalizeSubtitleSegmentsJson(raw: string): string {
  return JSON.stringify(parseSubtitleSegments(raw));
}

function parseSubtitleSegments(raw: string): SubtitleSegment[] {
  if (!raw?.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((segment) => {
        const startMs = Number(segment?.startMs ?? 0);
        const endMs = Number(segment?.endMs ?? startMs);
        const sourceText = String(segment?.sourceText ?? "");
        const translatedText = String(segment?.translatedText ?? "");
        if (!Number.isFinite(startMs) || !Number.isFinite(endMs)) return null;
        return {
          startMs: Math.max(0, Math.round(startMs)),
          endMs: Math.max(0, Math.round(endMs)),
          sourceText,
          translatedText,
        };
      })
      .filter((segment): segment is SubtitleSegment => segment !== null);
  } catch {
    return [];
  }
}

function recoverTransientStates(item: QueueItem): QueueItem {
  if (item.transcribeStatus === "processing") {
    return {
      ...item,
      transcribeStatus: "error",
      transcribeProgress: 0,
      transcribeSegmentCurrent: 0,
      transcribeSegmentTotal: 0,
      transcribePhase: "",
      transcribeError: item.transcribeError || "任务在上次退出时中断，请重试",
    };
  }

  return item;
}
