import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  cancelModelDownload as cancelModelDownloadApi,
  getModelStatus,
  startModelDownload as startModelDownloadApi,
} from "../api/model";
import { openModelDir as openModelDirApi } from "../api/system";
import type {
  AlignModel,
  AsrModel,
  DemucsModel,
  ModelDownloadStateSnapshot,
  ModelStatusResponse,
  ModelTarget,
} from "../../features/media/types";
import type { ToastTone } from "../types";

type PushToast = (message: string, tone?: ToastTone) => void;

type UseModelManagerArgs = {
  pushToast: PushToast;
  asrModel: AsrModel;
  alignModel: AlignModel;
  demucsModel: DemucsModel;
};

type ModelDownloadProgressEvent = ModelDownloadStateSnapshot & {
  target: ModelTarget;
  model: string;
};

type ModelStatusByTarget = Record<ModelTarget, ModelStatusResponse | null>;
type AsrStatusByModel = Record<AsrModel, ModelStatusResponse | null>;

const initialModelStatus: ModelStatusByTarget = {
  asr: null,
  align: null,
  demucs: null,
};

const initialAsrStatusByModel: AsrStatusByModel = {
  "Qwen3-ASR-0.6B": null,
  "Qwen3-ASR-1.7B": null,
  "cohere-transcribe-03-2026": null,
};

export function useModelManager({ pushToast, asrModel, alignModel, demucsModel }: UseModelManagerArgs) {
  const { t } = useTranslation(["toasts", "tasks", "models"]);
  const [statusByTarget, setStatusByTarget] = useState<ModelStatusByTarget>(initialModelStatus);
  const [asrStatusByModel, setAsrStatusByModel] = useState<AsrStatusByModel>(initialAsrStatusByModel);
  const asrModelRef = useRef<AsrModel>(asrModel);
  const alignModelRef = useRef<AlignModel>(alignModel);
  const demucsModelRef = useRef<DemucsModel>(demucsModel);
  const lastModelStatusRefreshAtRef = useRef<Record<ModelTarget, number>>({
    asr: 0,
    align: 0,
    demucs: 0,
  });

  const refreshModelStatus = useCallback(async () => {
    try {
      const [asr06b, asr17b, cohere, align, demucs] = await Promise.all([
        getModelStatus("asr", "Qwen3-ASR-0.6B"),
        getModelStatus("asr", "Qwen3-ASR-1.7B"),
        getModelStatus("asr", "cohere-transcribe-03-2026"),
        getModelStatus("align", alignModelRef.current),
        getModelStatus("demucs", demucsModelRef.current),
      ]);
      const nextAsrStatusByModel = {
        "Qwen3-ASR-0.6B": asr06b,
        "Qwen3-ASR-1.7B": asr17b,
        "cohere-transcribe-03-2026": cohere,
      };
      setAsrStatusByModel(nextAsrStatusByModel);
      setStatusByTarget({
        asr: nextAsrStatusByModel[asrModelRef.current],
        align,
        demucs,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : t("models.status.readFailed");
      pushToast(message, "error");
    }
  }, [pushToast, t]);

  useEffect(() => {
    asrModelRef.current = asrModel;
    alignModelRef.current = alignModel;
    demucsModelRef.current = demucsModel;
    const timer = setTimeout(() => {
      void refreshModelStatus();
    }, 0);
    return () => clearTimeout(timer);
  }, [alignModel, asrModel, demucsModel, refreshModelStatus]);

  useEffect(() => {
    let disposed = false;
    let unlisten: undefined | (() => void);
    listen<ModelDownloadProgressEvent>("model-download-progress", (event) => {
      const payload = event.payload;
      if (!payload?.target) return;
      const target = payload.target;
      if (
        target === "asr" &&
        payload.model !== "Qwen3-ASR-0.6B" &&
        payload.model !== "Qwen3-ASR-1.7B" &&
        payload.model !== "cohere-transcribe-03-2026"
      )
        return;
      if (target === "align" && payload.model !== alignModelRef.current) return;
      if (target === "demucs" && payload.model !== demucsModelRef.current) return;
      if (target === "asr") {
        const model = payload.model as AsrModel;
        setAsrStatusByModel((prev) => {
          const current = prev[model];
          if (!current) return prev;
          return {
            ...prev,
            [model]: {
              ...current,
              download: {
                phase: payload.phase,
                downloadedBytes: payload.downloadedBytes,
                totalBytes: payload.totalBytes,
                speedBytesPerSec: payload.speedBytesPerSec,
                message: payload.message,
              },
            },
          };
        });
      }
      setStatusByTarget((prev) => {
        if (target === "asr" && payload.model !== asrModelRef.current) return prev;
        const current = prev[target];
        if (!current) return prev;
        return {
          ...prev,
          [target]: {
            ...current,
            download: {
              phase: payload.phase,
              downloadedBytes: payload.downloadedBytes,
              totalBytes: payload.totalBytes,
              speedBytesPerSec: payload.speedBytesPerSec,
              message: payload.message,
            },
          },
        };
      });
      if (payload.phase === "downloading") {
        const now = Date.now();
        if (now - lastModelStatusRefreshAtRef.current[target] >= 1000) {
          lastModelStatusRefreshAtRef.current[target] = now;
          void refreshModelStatus();
        }
      } else {
        void refreshModelStatus();
      }
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [refreshModelStatus]);

  const startModelDownload = useCallback(async (target: ModelTarget, model?: string) => {
    try {
      await startModelDownloadApi(
        target,
        modelForTarget(target, asrModelRef.current, alignModelRef.current, demucsModelRef.current, model),
      );
      pushToast(t("models.download.start"), "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : t("models.download.startFailed");
      pushToast(message, "error");
    }
  }, [pushToast, refreshModelStatus, t]);

  const cancelModelDownload = useCallback(async (target: ModelTarget, model?: string) => {
    try {
      await cancelModelDownloadApi(
        target,
        modelForTarget(target, asrModelRef.current, alignModelRef.current, demucsModelRef.current, model),
      );
      pushToast(t("models.download.cancelRequest"), "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : t("models.download.cancelFailed");
      pushToast(message, "error");
    }
  }, [pushToast, refreshModelStatus, t]);

  const openModelDir = useCallback(async (target: ModelTarget, model?: string) => {
    try {
      await openModelDirApi(
        target,
        modelForTarget(target, asrModelRef.current, alignModelRef.current, demucsModelRef.current, model),
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : t("models.dir.openFailed");
      pushToast(message, "error");
    }
  }, [pushToast, t]);

  return useMemo(() => ({
    statusByTarget,
    asrStatus: statusByTarget.asr,
    asrStatusByModel,
    alignStatus: statusByTarget.align,
    demucsStatus: statusByTarget.demucs,
    refreshModelStatus,
    startModelDownload,
    cancelModelDownload,
    openModelDir,
  }), [
    asrStatusByModel,
    cancelModelDownload,
    openModelDir,
    refreshModelStatus,
    startModelDownload,
    statusByTarget,
  ]);
}

function modelForTarget(
  target: ModelTarget,
  asrModel: AsrModel,
  alignModel: AlignModel,
  demucsModel: DemucsModel,
  overrideModel?: string,
): AsrModel | AlignModel | DemucsModel {
  if (
    target === "asr" &&
    (overrideModel === "Qwen3-ASR-0.6B" ||
      overrideModel === "Qwen3-ASR-1.7B" ||
      overrideModel === "cohere-transcribe-03-2026")
  ) {
    return overrideModel;
  }
  if (target === "asr") return asrModel;
  if (target === "align") return alignModel;
  return demucsModel;
}
