import type { QueueItem } from "../../features/media/types";
import {
  transitionQueueItemStatus,
  type QueueStatusTransitionPayload,
  type QueueStatusTransitionTarget,
} from "../../features/media/stateMachine";
import type { AppAction } from "./appReducer";

type DispatchState = (action: AppAction) => void;

export function addQueueItems(dispatch: DispatchState, items: QueueItem[]): void {
  dispatch({ type: "add_queue_items", items });
}

export function removeQueueItem(dispatch: DispatchState, id: string): void {
  dispatch({ type: "remove_queue_item", id });
}

export function clearQueueItems(dispatch: DispatchState): void {
  dispatch({ type: "clear_queue" });
}

export function patchQueueItem(
  dispatch: DispatchState,
  id: string,
  updater: (item: QueueItem) => QueueItem,
): void {
  dispatch({
    type: "patch_queue_item",
    id,
    updater,
  });
}

export function transitionQueueStatus<T extends QueueStatusTransitionTarget>(
  dispatch: DispatchState,
  id: string,
  to: T,
  payloadFactory: (item: QueueItem) => QueueStatusTransitionPayload<T>,
): void {
  patchQueueItem(dispatch, id, (item) =>
    transitionQueueItemStatus(item, to, payloadFactory(item)));
}
