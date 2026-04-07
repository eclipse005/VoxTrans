import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState, useRef } from "react";
import {
  checkForUpdate,
  downloadUpdate,
  cancelUpdate,
  onUpdateProgress,
} from "../api/updater";
import type { UpdateCheckResult } from "../api/updater";

export function useAutoUpdateCheck() {
  const [availableUpdate, setAvailableUpdate] = useState<UpdateCheckResult | null>(null);
  const [showUpdateDialog, setShowUpdateDialog] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installProgress, setInstallProgress] = useState<number | null>(null);
  const hasAvailableUpdate = useMemo(() => availableUpdate?.hasUpdate ?? false, [availableUpdate]);
  const taskIdRef = useRef<string>("");

  // 监听下载进度
  useEffect(() => {
    if (!isTauri()) return;
    let cleanup: (() => void) | undefined;
    onUpdateProgress(([, progress]) => {
      setInstallProgress(Math.round(progress.percent));
    }).then((unlisten) => { cleanup = unlisten; });
    return () => { cleanup?.(); };
  }, []);

  // 启动时检测更新
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void checkForUpdate().then((result) => {
        if (!cancelled && result?.hasUpdate) {
          setAvailableUpdate(result);
        }
      }).catch((e) => {
        if (!cancelled) console.warn(`[updater] check failed: ${e}`);
      });
    }, 1200);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, []);

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
        console.error(`[updater] download failed: ${e}`);
      } finally {
        setInstalling(false);
        setInstallProgress(null);
        setShowUpdateDialog(false);
      }
    },
    cancelInstall: async () => {
      if (taskIdRef.current) await cancelUpdate(taskIdRef.current);
      setInstalling(false);
      setInstallProgress(null);
      setShowUpdateDialog(false);
    },
  };
}
