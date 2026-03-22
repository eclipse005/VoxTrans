import { useEffect, useState } from "react";
import { loadWorkspaceState } from "../api/workspace";
import { normalizeTranscribeStatus } from "../../features/media/stateMachine";
import type { QueueItem, SubtitleSegment } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";

type DispatchState = (action: AppAction) => void;

type UseWorkspacePersistenceArgs = {
  dispatch: DispatchState;
};

export function useWorkspacePersistence({
  dispatch,
}: UseWorkspacePersistenceArgs) {
  const [workspaceHydrated, setWorkspaceHydrated] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await loadWorkspaceState();
        if (cancelled) return;

        const queueItems = Array.isArray(res.queue) ? res.queue.map(normalizeQueueItem) : [];
        const deduped = dedupeById(queueItems);
        if (deduped.length > 0) {
          dispatch({ type: "add_queue_items", items: deduped });
        }
      } finally {
        if (!cancelled) {
          setWorkspaceHydrated(true);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [dispatch]);

  return {
    workspaceHydrated,
  };
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
    transcribePhaseDetail: typeof item.transcribePhaseDetail === "string" ? item.transcribePhaseDetail : "",
    transcribeError: item.transcribeError || "",
    subtitleSegmentsJson: normalizeSubtitleSegmentsJson(item.subtitleSegmentsJson),
  });
}

function normalizeTranscribePhase(value: unknown): QueueItem["transcribePhase"] {
  switch (value) {
    case "downloading":
    case "initializing":
    case "separating":
    case "recognizing":
    case "punctuate":
    case "correct":
    case "segment":
    case "summarize":
    case "translate":
    case "segment_optimize":
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
  const isYoutubePlaceholder = item.path.startsWith("youtube://pending/");
  if (isYoutubePlaceholder) {
    return {
      ...item,
      transcribeStatus: "error",
      transcribeProgress: 0,
      transcribeSegmentCurrent: 0,
      transcribeSegmentTotal: 0,
      transcribePhase: "",
      transcribePhaseDetail: "",
      transcribeError: "下载任务中断，请点击转录或转译继续下载",
    };
  }

  if (item.transcribeStatus === "queued") {
    return {
      ...item,
      transcribeStatus: "pending",
      transcribeProgress: 0,
      transcribeSegmentCurrent: 0,
      transcribeSegmentTotal: 0,
      transcribePhase: "",
      transcribePhaseDetail: "",
      transcribeError: "",
    };
  }

  if (item.transcribeStatus === "processing") {
    return {
      ...item,
      transcribeStatus: "error",
      transcribePhase: "",
      transcribePhaseDetail: "",
      transcribeError: item.transcribeError || "任务在运行中被中断，请重新开始",
    };
  }

  return item;
}
