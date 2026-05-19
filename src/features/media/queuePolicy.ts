import type { QueueItem } from "./types";
import { isYoutubePlaceholderPath } from "./youtubeUtils";

export function canDeleteQueueItem(item: QueueItem): boolean {
  const busy = item.transcribeStatus === "processing" || item.transcribeStatus === "queued";
  return !busy || isYoutubePlaceholderPath(item.path);
}
