import type { QueueItem } from "../../../features/media/types";

export function buildSubtitleVersion(item: QueueItem): string {
  return [item.id, item.transcribeStatus, item.subtitleSegmentsJson].join("|");
}
