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

export function setQueuedState(dispatch: DispatchState, id: string): void {
  transitionQueueStatus(dispatch, id, "queued", () => ({
    transcribeProgress: 0,
    transcribeSegmentCurrent: 0,
    transcribeSegmentTotal: 0,
    transcribePhase: "",
    transcribeError: "",
  }));
}

export function setProcessingState(dispatch: DispatchState, id: string): void {
  transitionQueueStatus(dispatch, id, "processing", () => ({
    transcribeProgress: 0,
    transcribeSegmentCurrent: 0,
    transcribeSegmentTotal: 0,
    transcribePhase: "initializing",
    transcribeError: "",
  }));
}

type DoneStatePayload = {
  subtitleSegmentsJson: string;
  resultText: string;
  resultSrt: string;
  segmentTotal: number;
};

export function setDoneState(
  dispatch: DispatchState,
  id: string,
  payload: DoneStatePayload,
): void {
  transitionQueueStatus(dispatch, id, "done", (item) => ({
    subtitleSegmentsJson: payload.subtitleSegmentsJson,
    transcribeProgress: 100,
    transcribeSegmentCurrent:
      payload.segmentTotal > 0
        ? payload.segmentTotal
        : item.transcribeSegmentCurrent,
    transcribeSegmentTotal:
      payload.segmentTotal > 0
        ? payload.segmentTotal
        : item.transcribeSegmentTotal,
    transcribePhase: "",
    resultText: payload.resultText,
    resultSrt: payload.resultSrt,
    transcribeError: "",
  }));
}

export function setErrorState(
  dispatch: DispatchState,
  id: string,
  errorMessage: string,
): void {
  transitionQueueStatus(dispatch, id, "error", () => ({
    transcribeProgress: 0,
    transcribeSegmentCurrent: 0,
    transcribeSegmentTotal: 0,
    transcribePhase: "",
    transcribeError: errorMessage,
  }));
}

export function applyTranscribeProgress(
  dispatch: DispatchState,
  params: {
    taskId: string;
    currentSegment: number;
    totalSegments: number;
  },
): void {
  patchQueueItem(dispatch, params.taskId, (item) => ({
    ...item,
    transcribeSegmentCurrent: Math.max(0, params.currentSegment || 0),
    transcribeSegmentTotal: Math.max(0, params.totalSegments || 0),
    transcribePhase: "recognizing",
    transcribeProgress:
      params.totalSegments > 0
        ? Math.min(
          99,
          Math.round((Math.max(0, params.currentSegment || 0) / params.totalSegments) * 100),
        )
        : item.transcribeProgress,
  }));
}

export function applyTranscribePhase(
  dispatch: DispatchState,
  params: {
    taskId: string;
    phase: TranscribePhase;
  },
): void {
  patchQueueItem(dispatch, params.taskId, (item) => ({
    ...item,
    transcribePhase: params.phase || item.transcribePhase,
  }));
}

