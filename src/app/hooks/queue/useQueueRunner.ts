import { useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  runPostAsrPipeline,
  separateVocals,
  saveSrt,
  transcribeMedia,
} from "../../api/transcribe";
import type {
  BuildSegmentsResponse,
  QueueItem,
  SavedSettings,
  SubtitleSegment,
  TranscribePhase,
} from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
import {
  applyTranscribePhase,
  applyTranscribeProgress,
  applySeparationProgress,
  setDoneState,
  setErrorState,
  setProcessingState,
} from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type TranscribeProgressEvent = {
  taskId: string;
  currentSegment: number;
  totalSegments: number;
};

type TranscribePhaseEvent = {
  taskId: string;
  phase: TranscribePhase;
};

type SeparateProgressEvent = {
  taskId: string;
  percent: number;
};

type UseQueueRunnerArgs = {
  settings: SavedSettings;
  dispatch: DispatchState;
  pushToast: PushToast;
  isTaskPresent: (taskId: string) => boolean;
};

export function useQueueRunner({
  settings,
  dispatch,
  pushToast,
  isTaskPresent,
}: UseQueueRunnerArgs) {
  useEffect(() => {
    let disposed = false;
    let unlistenProgress: undefined | (() => void);

    listen<TranscribeProgressEvent>("transcribe-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      applyTranscribeProgress(dispatch, {
        taskId: payload.taskId,
        currentSegment: payload.currentSegment,
        totalSegments: payload.totalSegments,
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenProgress = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlistenProgress) unlistenProgress();
    };
  }, [dispatch]);

  useEffect(() => {
    let disposed = false;
    let unlistenSeparation: undefined | (() => void);
    listen<SeparateProgressEvent>("separate-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      applySeparationProgress(dispatch, {
        taskId: payload.taskId,
        percent: payload.percent,
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenSeparation = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      if (unlistenSeparation) unlistenSeparation();
    };
  }, [dispatch]);

  useEffect(() => {
    let disposed = false;
    let unlistenPhase: undefined | (() => void);
    listen<TranscribePhaseEvent>("transcribe-phase", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      applyTranscribePhase(dispatch, {
        taskId: payload.taskId,
        phase: payload.phase,
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenPhase = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      if (unlistenPhase) unlistenPhase();
    };
  }, [dispatch]);

  const runTranscribe = useCallback(async (item: QueueItem) => {
    if (!isTaskPresent(item.id)) return;
    setProcessingState(dispatch, item.id);
    if (!isTaskPresent(item.id)) return;

    try {
      let transcribeAudioPath = item.path;
      if (settings.enableVocalSeparation) {
        applyTranscribePhase(dispatch, {
          taskId: item.id,
          phase: "separating",
        });
        const separation = await separateVocals({
          taskId: item.id,
          audioPath: item.path,
          model: settings.demucsModel,
        });
        if (!isTaskPresent(item.id)) return;
        transcribeAudioPath = separation.vocalsPath;
      }

      const response = await transcribeMedia({
        taskId: item.id,
        audioPath: transcribeAudioPath,
        provider: settings.provider,
        chunkTargetSeconds: settings.chunkTargetSeconds,
      });
      if (!isTaskPresent(item.id)) return;
      const processed = await runPostAsrPipeline({
        taskId: item.id,
        audioPath: item.path,
        words: response.words,
        subtitleMaxWordsPerSegment: settings.subtitleMaxWordsPerSegment,
        enablePunctuationOptimization: settings.enablePunctuationOptimization,
        translateApiKey: settings.translateApiKey,
        translateBaseUrl: settings.translateBaseUrl,
        translateModel: settings.translateModel,
        llmConcurrency: settings.llmConcurrency,
      });
      if (!isTaskPresent(item.id)) return;

      await saveSrt({
        taskId: item.id,
        mediaPath: item.path,
        outputPath: processed.srtOutputPath,
        content: processed.srt,
      });
      if (!isTaskPresent(item.id)) return;

      const normalizedSegments = toSubtitleSegmentsFromBuilt(processed.segments);
      setDoneState(dispatch, item.id, {
        subtitleSegmentsJson: JSON.stringify(normalizedSegments),
        resultText: processed.text,
        resultSrt: processed.srt,
        segmentTotal: response.segmentTotal,
      });
      if (!isTaskPresent(item.id)) return;
      pushToast(`已完成：${item.name}，SRT 已保存到 ${processed.srtOutputPath}`, "success");
    } catch (err) {
      if (!isTaskPresent(item.id)) return;
      reportError(err, "runTranscribe");
      const errorMessage = toUserErrorMessage(err, "转录失败，请检查模型和运行时配置");
      setErrorState(dispatch, item.id, errorMessage);
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
    }
  }, [
    dispatch,
    pushToast,
    isTaskPresent,
    settings.chunkTargetSeconds,
    settings.demucsModel,
    settings.enableVocalSeparation,
    settings.enablePunctuationOptimization,
    settings.provider,
    settings.subtitleMaxWordsPerSegment,
    settings.translateApiKey,
    settings.translateBaseUrl,
    settings.translateModel,
    settings.llmConcurrency,
  ]);

  return {
    runTranscribe,
  };
}

function toSubtitleSegmentsFromBuilt(segments: BuildSegmentsResponse["segments"]): SubtitleSegment[] {
  return segments.map((segment) => ({
    startMs: Math.max(0, Math.round(segment.start * 1000)),
    endMs: Math.max(0, Math.round(segment.end * 1000)),
    sourceText: segment.text ?? "",
    translatedText: "",
  }));
}
