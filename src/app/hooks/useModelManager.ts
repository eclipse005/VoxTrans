import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { cancelModelDownload as cancelModelDownloadApi, getModelStatus, startModelDownload as startModelDownloadApi } from "../api/model";
import { openModelDir as openModelDirApi } from "../api/system";
import type { ModelDownloadStateSnapshot } from "../../features/media/types";
import type { ToastTone } from "../types";

type PushToast = (message: string, tone?: ToastTone) => void;

type UseModelManagerArgs = {
  pushToast: PushToast;
};

const initialDownloadState: ModelDownloadStateSnapshot = {
  phase: "idle",
  downloadedBytes: 0,
  totalBytes: 0,
  speedBytesPerSec: 0,
  message: "",
};

export function useModelManager({ pushToast }: UseModelManagerArgs) {
  const [modelDir, setModelDir] = useState("");
  const [modelReady, setModelReady] = useState(false);
  const [modelDownload, setModelDownload] = useState<ModelDownloadStateSnapshot>(initialDownloadState);
  const [modelBusy, setModelBusy] = useState(false);
  const lastModelStatusRefreshAtRef = useRef(0);

  const refreshModelStatus = useCallback(async () => {
    try {
      const status = await getModelStatus();
      setModelDir(status.modelDir);
      setModelReady(status.ready);
      setModelDownload(status.download);
      setModelBusy(status.download.phase === "downloading");
    } catch (error) {
      const message = error instanceof Error ? error.message : "读取模型状态失败";
      pushToast(message, "error");
    }
  }, [pushToast]);

  useEffect(() => {
    void refreshModelStatus();
  }, [refreshModelStatus]);

  useEffect(() => {
    let unlisten: undefined | (() => void);
    listen<ModelDownloadStateSnapshot>("model-download-progress", (event) => {
      const payload = event.payload;
      if (!payload) return;
      setModelDownload(payload);
      if (payload.phase === "downloading") {
        const now = Date.now();
        if (now - lastModelStatusRefreshAtRef.current >= 1000) {
          lastModelStatusRefreshAtRef.current = now;
          void refreshModelStatus();
        }
      } else {
        setModelBusy(false);
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

  const startModelDownload = useCallback(async () => {
    setModelBusy(true);
    try {
      await startModelDownloadApi();
      pushToast("开始后台下载模型", "info");
      await refreshModelStatus();
    } catch (error) {
      setModelBusy(false);
      const message = error instanceof Error ? error.message : "启动模型下载失败";
      pushToast(message, "error");
    }
  }, [pushToast, refreshModelStatus]);

  const cancelModelDownload = useCallback(async () => {
    setModelBusy(true);
    try {
      await cancelModelDownloadApi();
      pushToast("已请求取消下载", "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : "取消下载失败";
      pushToast(message, "error");
    } finally {
      setModelBusy(false);
    }
  }, [pushToast, refreshModelStatus]);

  const openModelDir = useCallback(async () => {
    try {
      await openModelDirApi();
    } catch (error) {
      const message = error instanceof Error ? error.message : "打开模型目录失败";
      pushToast(message, "error");
    }
  }, [pushToast]);

  return {
    modelDir,
    modelReady,
    modelDownload,
    modelBusy,
    refreshModelStatus,
    startModelDownload,
    cancelModelDownload,
    openModelDir,
  };
}
