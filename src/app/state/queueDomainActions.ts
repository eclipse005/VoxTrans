import type {
  QueueItem,
  TranscribePhase,
} from "../../features/media/types";
import {
  transitionQueueItemStatus,
  type QueueStatusTransitionPayload,
  type QueueStatusTransitionTarget,
} from "../../features/media/stateMachine";
import type { AppAction } from "./appReducer";

type DispatchState = (action: AppAction) => void;

function phaseOrder(phase: string | undefined): number {
  switch (phase ?? "") {
    case "downloading":
      return 0;
    case "initializing":
      return 10;
    case "separating":
      return 20;
    case "recognizing":
      return 30;
    case "punctuate":
      return 40;
    case "segment":
      return 50;
    case "summarize":
      return 60;
    case "translate":
      return 70;
    case "segment_optimize":
      return 80;
    case "burning":
      return 90;
    default:
      return -1;
  }
}

function canAdvanceOrStayPhase(current: string | undefined, incoming: string | undefined): boolean {
  const currentOrder = phaseOrder(current);
  const incomingOrder = phaseOrder(incoming);
  if (incomingOrder < 0 || currentOrder < 0) return true;
  return incomingOrder >= currentOrder;
}

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

export function applyTranscribePhase(
  dispatch: DispatchState,
  params: {
    taskId: string;
    phase: TranscribePhase;
    phaseDetail?: string;
  },
): void {
  patchQueueItem(dispatch, params.taskId, (item) => {
    const nextPhase = params.phase || item.transcribePhase;
    if (!canAdvanceOrStayPhase(item.transcribePhase, nextPhase)) {
      return item;
    }
    const phaseChanged = nextPhase !== item.transcribePhase;
    const hasIncomingDetail = typeof params.phaseDetail === "string";
    const incomingDetail: string = typeof params.phaseDetail === "string"
      ? params.phaseDetail
      : "";
    if (phaseChanged) {
      return {
        ...item,
        transcribeStatus: "processing",
        transcribePhase: nextPhase,
        transcribePhaseDetail: incomingDetail,
        transcribeSegmentCurrent: 0,
        transcribeSegmentTotal: 0,
      };
    }
    return {
      ...item,
      transcribeStatus: "processing",
      transcribePhase: nextPhase,
      transcribePhaseDetail: hasIncomingDetail ? incomingDetail : item.transcribePhaseDetail,
    };
  });
}
