import type { QueueItem } from "./types";
import { isBusyStatus } from "./taskStatus";
import { isYoutubePlaceholderPath } from "./youtubeUtils";

export function canDeleteQueueItem(item: QueueItem): boolean {
  // Review parks still hold the pipeline slot for *other* tasks, but the
  // item itself remains deletable (abandon review). Only machine queue
  // states block delete.
  const busy = isBusyStatus(item.transcribeStatus);
  return !busy || isYoutubePlaceholderPath(item.path);
}
