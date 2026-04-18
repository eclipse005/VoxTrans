import { invoke } from "@tauri-apps/api/core";

import type { QueueItem, SubtitleCue } from "../../../features/media/types";
import {
  buildCueListFromSubtitleSegments,
  buildSubtitleSegmentsFromCues,
  parseSubtitleSegments,
  subtitleSegmentsToSrt,
} from "../../../features/media/subtitleSegments";
import { exportTaskSrts, type ExportSrtItem } from "../../api/transcribe";

export async function saveSubtitleEditor(
  taskId: string,
  cues: SubtitleCue[],
): Promise<void> {
  const segments = buildSubtitleSegmentsFromCues(cues);
  return invoke<void>("save_subtitle_editor", {
    request: {
      taskId,
      content: subtitleSegmentsToSrt(segments),
      subtitleSegmentsJson: JSON.stringify(segments),
    },
  });
}

export async function loadSubtitleEditorData(item: QueueItem): Promise<{
  response: { srtPath: string; warnings: string[] };
  hydratedCues: SubtitleCue[];
}> {
  const segments = parseSubtitleSegments(item.subtitleSegmentsJson);
  const response = {
    srtPath: "",
    warnings: [],
  };

  if (segments.length > 0) {
    return {
      response,
      hydratedCues: buildCueListFromSubtitleSegments(item.id, segments),
    };
  }
  return { response, hydratedCues: [] };
}

export async function exportSubtitleVariantsToDirectory(
  taskId: string,
  targetDir: string,
  taskName: string,
  items: ExportSrtItem[],
): Promise<string[]> {
  return exportTaskSrts({
    taskId,
    targetDir,
    taskName,
    items,
  });
}
