import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getYtDlpVersion, updateYtDlp } from "../api/youtube";
import { toUserErrorMessage } from "../utils/errors";

type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseYtDlpManagerArgs = {
  pushToast: PushToast;
};

export function useYtDlpManager({ pushToast }: UseYtDlpManagerArgs) {
  const { t } = useTranslation(["tasks"]);
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
        pushToast(t("tasks:ytdlp.updated", { from: result.fromVersion, to: result.toVersion }), "success");
      } else {
        pushToast(t("tasks:ytdlp.alreadyLatest", { version: result.toVersion || result.fromVersion || "unknown" }), "info");
      }
    } catch (error) {
      pushToast(toUserErrorMessage(error, t("tasks:ytdlp.updateFailed")), "error");
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
