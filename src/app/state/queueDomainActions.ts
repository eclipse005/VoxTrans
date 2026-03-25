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

export function setQueuedState(dispatch: DispatchState, id: string): void {
  transitionQueueStatus(dispatch, id, "queued", () => ({
    transcribeProgress: 0,
    transcribeSegmentCurrent: 0,
    transcribeSegmentTotal: 0,
    transcribePhase: "",
    transcribePhaseDetail: "",
    transcribeError: "",
  }));
}

export function setProcessingState(dispatch: DispatchState, id: string): void {
  transitionQueueStatus(dispatch, id, "processing", () => ({
    transcribeProgress: 0,
    transcribeSegmentCurrent: 0,
    transcribeSegmentTotal: 0,
    transcribePhase: "initializing",
    transcribePhaseDetail: "",
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
    transcribePhaseDetail: "",
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
    transcribePhaseDetail: "",
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
  patchQueueItem(dispatch, params.taskId, (item) => {
    // Stage switching is owned by transcribe-phase events.
    // Progress events only update details while already in recognizing.
    if (item.transcribePhase !== "recognizing") {
      return item;
    }
    return {
      ...item,
      transcribeStatus: "processing",
      transcribeSegmentCurrent: Math.max(0, params.currentSegment || 0),
      transcribeSegmentTotal: Math.max(0, params.totalSegments || 0),
      transcribePhaseDetail:
        params.totalSegments > 0
          ? `${Math.max(0, params.currentSegment || 0)}/${params.totalSegments}`
          : "",
      transcribeProgress:
        params.totalSegments > 0
          ? Math.min(
            99,
            Math.round((Math.max(0, params.currentSegment || 0) / params.totalSegments) * 100),
          )
          : item.transcribeProgress,
    };
  });
}

export function applySeparationProgress(
  dispatch: DispatchState,
  params: {
    taskId: string;
    percent: number;
  },
): void {
  patchQueueItem(dispatch, params.taskId, (item) => {
    if (!canAdvanceOrStayPhase(item.transcribePhase, "separating")) {
      return item;
    }
    const percent = Math.max(0, Math.min(100, Math.round(params.percent || 0)));
    return {
      ...item,
      transcribeStatus: "processing",
      transcribeSegmentCurrent: percent,
      transcribeSegmentTotal: 100,
      transcribePhase: "separating",
      transcribePhaseDetail: `${percent}%`,
      transcribeProgress: Math.min(99, percent),
    };
  });
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

export function applyTranslateProgress(
  dispatch: DispatchState,
  params: {
    taskId: string;
    currentBatch: number;
    totalBatches: number;
  },
): void {
  patchQueueItem(dispatch, params.taskId, (item) => {
    if (!canAdvanceOrStayPhase(item.transcribePhase, "translate")) {
      return item;
    }
    const total = Math.max(0, Math.round(params.totalBatches || 0));
    const current = Math.max(0, Math.min(total, Math.round(params.currentBatch || 0)));
    return {
      ...item,
      transcribeStatus: "processing",
      transcribePhase: "translate",
      transcribePhaseDetail: total > 0 ? `${current}/${total}` : "",
      transcribeSegmentCurrent: current,
      transcribeSegmentTotal: total,
      transcribeProgress:
        total > 0
          ? Math.min(99, Math.round((current / total) * 100))
          : item.transcribeProgress,
    };
  });
}
