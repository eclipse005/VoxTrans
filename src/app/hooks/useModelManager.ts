import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  cancelModelDownload as cancelModelDownloadApi,
  getModelStatus,
  startModelDownload as startModelDownloadApi,
} from "../api/model";
import { openModelDir as openModelDirApi } from "../api/system";
import {
  ALIGN_MODELS,
  ASR_MODELS,
  DEFAULT_ALIGN_MODEL,
  DEFAULT_DEMUCS_MODEL,
  isAlignModel,
  isAsrModel,
} from "../../features/media/modelCatalog";
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
type AlignStatusByModel = Record<AlignModel, ModelStatusResponse | null>;

function emptyAsrStatusMap(): AsrStatusByModel {
  return Object.fromEntries(ASR_MODELS.map((id) => [id, null])) as AsrStatusByModel;
}

function emptyAlignStatusMap(): AlignStatusByModel {
  return Object.fromEntries(ALIGN_MODELS.map((id) => [id, null])) as AlignStatusByModel;
}

const initialModelStatus: ModelStatusByTarget = {
  asr: null,
  align: null,
  demucs: null,
};

export function useModelManager({ pushToast, asrModel, alignModel, demucsModel }: UseModelManagerArgs) {
  const { t } = useTranslation(["toasts", "tasks", "models"]);
  const [statusByTarget, setStatusByTarget] = useState<ModelStatusByTarget>(initialModelStatus);
  const [asrStatusByModel, setAsrStatusByModel] = useState<AsrStatusByModel>(emptyAsrStatusMap);
  const [alignStatusByModel, setAlignStatusByModel] = useState<AlignStatusByModel>(emptyAlignStatusMap);
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
      const [asrStatuses, alignStatuses, demucs] = await Promise.all([
        Promise.all(ASR_MODELS.map((model) => getModelStatus("asr", model))),
        Promise.all(ALIGN_MODELS.map((model) => getModelStatus("align", model))),
        getModelStatus("demucs", demucsModelRef.current || DEFAULT_DEMUCS_MODEL),
      ]);
      const nextAsrStatusByModel = emptyAsrStatusMap();
      ASR_MODELS.forEach((model, index) => {
        nextAsrStatusByModel[model] = asrStatuses[index] ?? null;
      });
      const nextAlignStatusByModel = emptyAlignStatusMap();
      ALIGN_MODELS.forEach((model, index) => {
        nextAlignStatusByModel[model] = alignStatuses[index] ?? null;
      });
      setAsrStatusByModel(nextAsrStatusByModel);
      setAlignStatusByModel(nextAlignStatusByModel);
      setStatusByTarget({
        asr: nextAsrStatusByModel[asrModelRef.current],
        align: nextAlignStatusByModel[alignModelRef.current],
        demucs,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : t("models:status.readFailed");
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
      if (target === "asr" && !isAsrModel(payload.model)) return;
      if (target === "align" && !isAlignModel(payload.model)) return;
      if (target === "demucs" && payload.model !== demucsModelRef.current) return;
      if (target === "asr" && isAsrModel(payload.model)) {
        const model = payload.model;
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
      if (target === "align" && isAlignModel(payload.model)) {
        const model = payload.model;
        setAlignStatusByModel((prev) => {
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
        if (target === "align" && payload.model !== alignModelRef.current) return prev;
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
      pushToast(t("models:download.start"), "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : t("models:download.startFailed");
      pushToast(message, "error");
    }
  }, [pushToast, refreshModelStatus, t]);

  const cancelModelDownload = useCallback(async (target: ModelTarget, model?: string) => {
    try {
      await cancelModelDownloadApi(
        target,
        modelForTarget(target, asrModelRef.current, alignModelRef.current, demucsModelRef.current, model),
      );
      pushToast(t("models:download.cancelRequest"), "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : t("models:download.cancelFailed");
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
      const message = error instanceof Error ? error.message : t("models:dir.openFailed");
      pushToast(message, "error");
    }
  }, [pushToast, t]);

  return useMemo(() => ({
    statusByTarget,
    asrStatus: statusByTarget.asr,
    asrStatusByModel,
    alignStatus: statusByTarget.align,
    alignStatusByModel,
    demucsStatus: statusByTarget.demucs,
    refreshModelStatus,
    startModelDownload,
    cancelModelDownload,
    openModelDir,
  }), [
    alignStatusByModel,
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
  if (target === "asr") {
    if (overrideModel && isAsrModel(overrideModel)) return overrideModel;
    return asrModel;
  }
  if (target === "align") {
    if (overrideModel && isAlignModel(overrideModel)) return overrideModel;
    return alignModel || DEFAULT_ALIGN_MODEL;
  }
  return (overrideModel as DemucsModel | undefined) || demucsModel || DEFAULT_DEMUCS_MODEL;
}
