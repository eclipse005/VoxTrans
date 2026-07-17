import type { TranscribeStatus } from "./types";

/**
 * Subtitle editor has two exclusive data modes — never mix them:
 *
 * - **preview**: machine is running (or idle with transient JSON). Cues are a
 *   pure projection of `task.subtitleSegmentsJson` stream ticks. No dirty,
 *   no user draft, no blocking reloads.
 * - **edit**: task is parked for humans (`review_*` or `done`). Cues are a
 *   local draft; dirty protects against overwriting mid-edit.
 */
export type SubtitleEditorMode = "preview" | "edit";

export function resolveSubtitleEditorMode(
  status: TranscribeStatus | string | undefined,
): SubtitleEditorMode {
  switch (status) {
    case "done":
    case "review_source":
    case "review_target":
      return "edit";
    default:
      // processing | queued | pending | error | unknown → follow queue JSON
      return "preview";
  }
}

export function isSubtitleEditMode(status: TranscribeStatus | string | undefined): boolean {
  return resolveSubtitleEditorMode(status) === "edit";
}
