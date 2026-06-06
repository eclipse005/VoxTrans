import type { QueueItem, TranscribeStatus } from "./types";

/**
 * Transcribe queue state machine:
 * pending -> queued -> processing -> (done | error)
 * done/error -> queued (manual re-run)
 */
const TRANSCRIBE_STATUS_TRANSITIONS = {
  pending: ["queued"],
  queued: ["processing"],
  processing: ["done", "error"],
  done: ["queued"],
  error: ["queued"],
} as const satisfies Record<TranscribeStatus, readonly TranscribeStatus[]>;

function canTransitionTranscribeStatus(
  from: TranscribeStatus,
  to: TranscribeStatus,
): boolean {
  const allowed = TRANSCRIBE_STATUS_TRANSITIONS[from] as readonly TranscribeStatus[];
  return allowed.includes(to);
}

export function normalizeTranscribeStatus(value: unknown): TranscribeStatus {
  if (typeof value !== "string") return "pending";
  if (value in TRANSCRIBE_STATUS_TRANSITIONS) {
    return value as TranscribeStatus;
  }
  return "pending";
}

type QueueStatusPayloads = {
  queued: Pick<
    QueueItem,
    | "taskProgress"
    | "transcribeError"
  >;
  processing: Pick<
    QueueItem,
    | "taskProgress"
    | "transcribeError"
  >;
  done: Pick<
    QueueItem,
    | "subtitleSegmentsJson"
    | "taskProgress"
    | "resultText"
    | "resultSrt"
    | "transcribeError"
  >;
  error: Pick<
    QueueItem,
    | "taskProgress"
    | "transcribeError"
  >;
};

/**
 * Applies a guarded status transition. If transition is illegal, the original
 * item is returned unchanged.
 */
export type QueueStatusTransitionTarget = keyof QueueStatusPayloads;
export type QueueStatusTransitionPayload<T extends QueueStatusTransitionTarget> =
  QueueStatusPayloads[T];

export function transitionQueueItemStatus<T extends QueueStatusTransitionTarget>(
  item: QueueItem,
  to: T,
  payload: QueueStatusPayloads[T],
): QueueItem {
  if (!canTransitionTranscribeStatus(item.transcribeStatus, to)) {
    if (import.meta.env.DEV) {
      // Keep invalid transitions observable during development.
      console.warn(
        `[queue-state] invalid transition: ${item.id} ${item.transcribeStatus} -> ${to}`,
      );
    }
    return item;
  }
  return {
    ...item,
    ...payload,
    transcribeStatus: to,
  };
}
