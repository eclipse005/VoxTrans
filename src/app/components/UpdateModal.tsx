import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { useDialogA11y } from "./useDialogA11y";
import { invoke } from "@tauri-apps/api/core";
import i18n from "../../i18n";
import type { UpdateCheckResult } from "../api/updater";

type UpdateModalProps = {
  visible: boolean;
  update: UpdateCheckResult | null;
  installing: boolean;
  installProgress: number | null;
  onClose: () => void;
  onInstall: () => void | Promise<void>;
  onCancelInstall?: () => void | Promise<void>;
  onSkipVersion?: () => void | Promise<void>;
};

export default function UpdateModal({
  visible,
  update,
  installing,
  installProgress,
  onClose,
  onInstall,
  onCancelInstall,
  onSkipVersion,
}: UpdateModalProps) {
  const { t } = useTranslation(["updater", "common"]);
  const dialogRef = useDialogA11y(visible, onClose);
  const publishedAt = update ? formatDateRelative(update.publishedAt) : "";
  const notes = update?.notes.trim() ?? "";
  const releaseUrl = update?.htmlUrl ?? "";

  // ponytail: GitHub release body is untrusted markdown → sanitize before inject.
  // 同步分支：未启用 async 扩展时 marked.parse 返回 string,这里用断言收口类型。
  const notesHtml = useMemo(
    () => (notes.length > 0
      ? DOMPurify.sanitize(marked.parse(notes, { async: false }) as string)
      : ""),
    [notes],
  );

  const handleNotesClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const anchor = (e.target as HTMLElement).closest("a");
    if (!anchor) return;
    const href = anchor.getAttribute("href");
    if (!href) return;
    e.preventDefault();
    void invoke("open_external_url", { url: href }).catch((err) => {
      console.error("无法打开链接:", href, err);
    });
  };

  if (!visible || !update) {
    return null;
  }

  return (
    <div className="modal-overlay">
      <div
        ref={dialogRef}
        className="modal-content modal-content-update"
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-modal-title"
        tabIndex={-1}
      >
        <button className="modal-close" onClick={onClose} aria-label={t("updater.modal.closeAriaLabel")}>×</button>

        <div className="update-header">
          <h3 id="update-modal-title" className="apple-heading-medium">
            {update.releaseName || `v${update.latestVersion}`}
          </h3>
        </div>
        <div className="update-meta">
          <span className="app-version">v{update.latestVersion}</span>
          {publishedAt ? <span className="update-published">{publishedAt}</span> : null}
        </div>

        <div className="update-notes">
          <div
            className="update-notes-body update-notes-markdown"
            onClick={handleNotesClick}
            dangerouslySetInnerHTML={
              notesHtml
                ? { __html: notesHtml }
                : { __html: `<p>${t("updater.modal.noNotes")}</p>` }
            }
          />
        </div>

        {releaseUrl ? (
          <div className="update-release-link">
            <a
              href="#"
              onClick={async (e) => {
                e.preventDefault();
                try {
                  await invoke("open_external_url", { url: releaseUrl });
                } catch (err) {
                  console.error("无法打开链接:", releaseUrl, err);
                }
              }}
            >
              {t("updater.modal.viewReleaseNotes")} ↗
            </a>
          </div>
        ) : null}

        <div className="settings-footer">
          {installing ? (
            <>
              <button className="nav-button" onClick={onCancelInstall ?? onClose}>{t("updater.button.cancelDownload")}</button>
              <button className="nav-button" disabled>
                {t("updater.status.downloading")} {installProgress != null ? `${installProgress}%` : ""}
              </button>
            </>
          ) : (
            <>
              <button className="nav-button" onClick={onClose}>{t("common:button.cancel")}</button>
              {onSkipVersion ? (
                <button className="nav-button" onClick={() => { void onSkipVersion(); }}>{t("updater.button.skipVersion")}</button>
              ) : null}
              <button className="nav-button" onClick={() => { void onInstall(); }}>{t("updater.button.downloadInstall")}</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function formatDateRelative(value: string | null): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  // publishedAt is an ISO 8601 UTC timestamp from GitHub. Both Date.now() and
  // date.getTime() are UTC-based, so the elapsed difference is correct in any
  // timezone — no Asia/Shanghai hardcoding needed.
  const diffMs = Date.now() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return i18n.t("updater.relative.justNow");
  if (diffMins < 60) return i18n.t("updater.relative.minutesAgo", { n: diffMins });
  if (diffHours < 24) return i18n.t("updater.relative.hoursAgo", { n: diffHours });
  if (diffDays < 30) return i18n.t("updater.relative.daysAgo", { n: diffDays });

  // Fall back to an absolute date, localized to the active UI language.
  return date.toLocaleDateString(i18n.language, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}
