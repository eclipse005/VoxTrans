import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import {
  checkForUpdate,
  downloadUpdate,
  cancelUpdate,
  onUpdateProgress,
  skipUpdateVersion,
  getSkippedVersion,
} from "../api/updater";
import type { UpdateCheckResult } from "../api/updater";

type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

export function useAutoUpdateCheck(pushToast: PushToast) {
  const { t } = useTranslation(["toasts", "updater"]);
  const [availableUpdate, setAvailableUpdate] = useState<UpdateCheckResult | null>(null);
  const [showUpdateDialog, setShowUpdateDialog] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installProgress, setInstallProgress] = useState<number | null>(null);
  const hasAvailableUpdate = useMemo(() => availableUpdate?.hasUpdate ?? false, [availableUpdate]);
  const taskIdRef = useRef<string>("");

  // 监听下载进度 — 使用 disposed 标志位防止 effect 重跑时旧监听器泄漏
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: undefined | (() => void);

    onUpdateProgress(([, progress]) => {
      setInstallProgress(Math.round(progress.percent));
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
  }, []);

  // 启动时检测更新，并对比已忽略版本
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void Promise.all([
        checkForUpdate(),
        getSkippedVersion().catch(() => null),
      ]).then(([result, skippedVersion]) => {
        if (cancelled) return;
        if (result?.hasUpdate && result.latestVersion !== skippedVersion) {
          setAvailableUpdate(result);
        }
      }).catch((e) => {
        if (!cancelled) console.warn(`[updater] check failed: ${e}`);
      });
    }, 1200);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, []);

  const skipVersion = useCallback(async () => {
    if (!availableUpdate?.hasUpdate) return;
    try {
      await skipUpdateVersion(availableUpdate.latestVersion);
    } catch (e) {
      console.error(`[updater] skip version failed: ${e}`);
    }
    setAvailableUpdate(null);
    setShowUpdateDialog(false);
  }, [availableUpdate]);

  return {
    availableUpdate,
    hasAvailableUpdate,
    showUpdateDialog,
    installing,
    installProgress,
    openUpdateDialog: () => {
      if (hasAvailableUpdate) setShowUpdateDialog(true);
    },
    closeUpdateDialog: () => {
      setShowUpdateDialog(false);
    },
    installUpdate: async () => {
      if (!availableUpdate?.hasUpdate || installing) return;
      setInstalling(true);
      setInstallProgress(0);
      taskIdRef.current = `update_${Date.now()}`;
      try {
        await downloadUpdate(availableUpdate.downloadUrl, taskIdRef.current);
      } catch (e) {
        pushToast(t("toasts:updater.downloadFailed", { message: e instanceof Error ? e.message : String(e) }), "error");
      } finally {
        setInstalling(false);
        setInstallProgress(null);
        setShowUpdateDialog(false);
      }
    },
    cancelInstall: async () => {
      try {
        if (taskIdRef.current) await cancelUpdate(taskIdRef.current);
      } catch (e) {
        console.error(`[updater] cancel failed: ${e}`);
      }
      setInstalling(false);
      setInstallProgress(null);
      setShowUpdateDialog(false);
    },
    skipVersion,
  };
}
