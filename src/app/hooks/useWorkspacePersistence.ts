import { useEffect, useState } from "react";
import { loadWorkspaceState } from "../api/workspace";
import { normalizeTranscribeStatus } from "../../features/media/stateMachine";
import { normalizeSubtitleSegmentsJson } from "../../features/media/subtitleSegments";
import {
  createEmptyTaskProgress,
  normalizeTaskProgress,
  type QueueItem,
} from "../../features/media/types";
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
    taskProgress: normalizeTaskProgress(item.taskProgress),
    transcribeError: item.transcribeError || "",
    subtitleSegmentsJson: normalizeSubtitleSegmentsJson(item.subtitleSegmentsJson),
  });
}

function recoverTransientStates(item: QueueItem): QueueItem {
  const isYoutubePlaceholder = item.path.startsWith("youtube://pending/");
  if (isYoutubePlaceholder) {
    return {
      ...item,
      transcribeStatus: "error",
      taskProgress: createEmptyTaskProgress(),
      transcribeError: "下载任务中断，请点击转录或转译继续下载",
    };
  }

  if (item.transcribeStatus === "queued") {
    return {
      ...item,
      transcribeStatus: "pending",
      taskProgress: createEmptyTaskProgress(),
      transcribeError: "",
    };
  }

  if (item.transcribeStatus === "processing") {
    return {
      ...item,
      transcribeStatus: "error",
      taskProgress: createEmptyTaskProgress(),
      transcribeError: item.transcribeError || "任务在运行中被中断，请重新开始",
    };
  }

  return item;
}
