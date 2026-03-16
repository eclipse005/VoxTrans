import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  cancelModelDownload as cancelModelDownloadApi,
  getModelStatus,
  startModelDownload as startModelDownloadApi,
} from "../api/model";
import { openModelDir as openModelDirApi } from "../api/system";
import type {
  DemucsModel,
  ModelDownloadStateSnapshot,
  ModelStatusResponse,
  ModelTarget,
} from "../../features/media/types";
import type { ToastTone } from "../types";

type PushToast = (message: string, tone?: ToastTone) => void;

type UseModelManagerArgs = {
  pushToast: PushToast;
  demucsModel: DemucsModel;
};

type ModelDownloadProgressEvent = ModelDownloadStateSnapshot & {
  target: ModelTarget;
  model: string;
};

type ModelStatusByTarget = Record<ModelTarget, ModelStatusResponse | null>;

const initialModelStatus: ModelStatusByTarget = {
  asr: null,
  demucs: null,
};

const initialDownloadState: ModelDownloadStateSnapshot = {
  phase: "idle",
  downloadedBytes: 0,
  totalBytes: 0,
  speedBytesPerSec: 0,
  message: "",
};

export function useModelManager({ pushToast, demucsModel }: UseModelManagerArgs) {
  const [statusByTarget, setStatusByTarget] = useState<ModelStatusByTarget>(initialModelStatus);
  const demucsModelRef = useRef<DemucsModel>(demucsModel);
  const lastModelStatusRefreshAtRef = useRef<Record<ModelTarget, number>>({
    asr: 0,
    demucs: 0,
  });

  const refreshModelStatus = useCallback(async () => {
    try {
      const [asr, demucs] = await Promise.all([
        getModelStatus("asr"),
        getModelStatus("demucs", demucsModelRef.current),
      ]);
      setStatusByTarget({
        asr,
        demucs,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : "读取模型状态失败";
      pushToast(message, "error");
    }
  }, [pushToast]);

  useEffect(() => {
    demucsModelRef.current = demucsModel;
    const timer = setTimeout(() => {
      void refreshModelStatus();
    }, 0);
    return () => clearTimeout(timer);
  }, [demucsModel, refreshModelStatus]);

  useEffect(() => {
    let unlisten: undefined | (() => void);
    listen<ModelDownloadProgressEvent>("model-download-progress", (event) => {
      const payload = event.payload;
      if (!payload?.target) return;
      const target = payload.target;
      if (target === "demucs" && payload.model !== demucsModelRef.current) return;
      setStatusByTarget((prev) => {
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
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      if (unlisten) unlisten();
    };
  }, [refreshModelStatus]);

  const startModelDownload = useCallback(async (target: ModelTarget) => {
    try {
      await startModelDownloadApi(
        target,
        target === "demucs" ? demucsModelRef.current : undefined,
      );
      pushToast("开始后台下载模型", "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : "启动模型下载失败";
      pushToast(message, "error");
    }
  }, [pushToast, refreshModelStatus]);

  const cancelModelDownload = useCallback(async (target: ModelTarget) => {
    try {
      await cancelModelDownloadApi(
        target,
        target === "demucs" ? demucsModelRef.current : undefined,
      );
      pushToast("已请求取消下载", "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : "取消下载失败";
      pushToast(message, "error");
    }
  }, [pushToast, refreshModelStatus]);

  const openModelDir = useCallback(async (target: ModelTarget) => {
    try {
      await openModelDirApi(target);
    } catch (error) {
      const message = error instanceof Error ? error.message : "打开模型目录失败";
      pushToast(message, "error");
    }
  }, [pushToast]);

  return useMemo(() => ({
    statusByTarget,
    asrStatus: statusByTarget.asr,
    demucsStatus: statusByTarget.demucs,
    getDownloadState: (target: ModelTarget) => statusByTarget[target]?.download ?? initialDownloadState,
    getReady: (target: ModelTarget) => statusByTarget[target]?.ready ?? false,
    getModelDir: (target: ModelTarget) => statusByTarget[target]?.modelDir ?? "",
    refreshModelStatus,
    startModelDownload,
    cancelModelDownload,
    openModelDir,
  }), [
    cancelModelDownload,
    openModelDir,
    refreshModelStatus,
    startModelDownload,
    statusByTarget,
  ]);
}
