import { useCallback, useEffect, useState } from "react";
import { getYtDlpVersion, updateYtDlp } from "../api/youtube";
import { toUserErrorMessage } from "../utils/errors";

type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseYtDlpManagerArgs = {
  pushToast: PushToast;
};

export function useYtDlpManager({ pushToast }: UseYtDlpManagerArgs) {
  const [ytDlpVersion, setYtDlpVersion] = useState("");
  const [ytDlpUpdating, setYtDlpUpdating] = useState(false);

  const refreshYtDlpVersion = useCallback(async () => {
    try {
      const version = await getYtDlpVersion();
      setYtDlpVersion(version || "");
    } catch {
      setYtDlpVersion("");
    }
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void refreshYtDlpVersion();
    }, 600);
    return () => window.clearTimeout(timer);
  }, [refreshYtDlpVersion]);

  const updateYtDlpBinary = useCallback(async () => {
    if (ytDlpUpdating) return;
    setYtDlpUpdating(true);
    try {
      const result = await updateYtDlp();
      await refreshYtDlpVersion();
      if (result.updated) {
        pushToast(`yt-dlp 已更新: ${result.fromVersion} -> ${result.toVersion}`, "success");
      } else {
        pushToast(`yt-dlp 已是最新版本 (${result.toVersion || result.fromVersion || "unknown"})`, "info");
      }
    } catch (error) {
      pushToast(toUserErrorMessage(error, "yt-dlp 更新失败"), "error");
    } finally {
      setYtDlpUpdating(false);
    }
  }, [pushToast, refreshYtDlpVersion, ytDlpUpdating]);

  return {
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  };
}
