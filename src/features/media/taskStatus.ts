import type { TranscribeStatus } from "./types";
import { isSubtitleEditMode } from "./subtitleEditorMode";

/**
 * Holds the single pipeline slot: no other task may start machine work.
 * Includes human review parks — review is a pause of the *current* job, not
 * a free slot for the rest of the queue.
 */
export function holdsPipelineSlot(status: TranscribeStatus | string): boolean {
  return (
    status === "processing"
    || status === "review_source"
    || status === "review_target"
  );
}

/**
 * Blocks destructive list ops (delete) / language edits while the job is
 * in the machine queue. Review is editable and deletable, so it is excluded.
 */
export function isBusyStatus(status: TranscribeStatus | string): boolean {
  return status === "queued" || status === "processing";
}

export function isAwaitingReviewStatus(status: TranscribeStatus | string): boolean {
  return status === "review_source" || status === "review_target";
}

/** Subtitle editor may write SoT — same rule as subtitle editor edit mode. */
export function isEditableStatus(status: TranscribeStatus | string): boolean {
  return isSubtitleEditMode(status);
}
