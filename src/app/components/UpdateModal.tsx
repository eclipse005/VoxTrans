import { useCallback, useEffect, useMemo, useRef } from "react";
import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { useDialogA11y } from "./useDialogA11y";
import { invoke } from "@tauri-apps/api/core";
import i18n from "../../i18n";
import type { UpdateCheckResult } from "../api/updater";

const POPOVER_WIDTH = 400;
const POPOVER_GAP = 8;
const VIEWPORT_PAD = 12;
const MAX_POPOVER_HEIGHT = 520;

type UpdateModalProps = {
  visible: boolean;
  update: UpdateCheckResult | null;
  installing: boolean;
  installProgress: number | null;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  onInstall: () => void | Promise<void>;
  onCancelInstall?: () => void | Promise<void>;
  onSkipVersion?: () => void | Promise<void>;
};

function positionPopover(el: HTMLElement, anchor: HTMLElement) {
  const rect = anchor.getBoundingClientRect();
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  let left = rect.left;
  if (left + POPOVER_WIDTH > vw - VIEWPORT_PAD) {
    left = Math.max(VIEWPORT_PAD, vw - POPOVER_WIDTH - VIEWPORT_PAD);
  }
  if (left < VIEWPORT_PAD) left = VIEWPORT_PAD;

  const spaceBelow = vh - rect.bottom - POPOVER_GAP - VIEWPORT_PAD;
  const spaceAbove = rect.top - POPOVER_GAP - VIEWPORT_PAD;
  const preferBelow = spaceBelow >= 240 || spaceBelow >= spaceAbove;

  const originX = Math.min(
    Math.max(rect.left - left + rect.width / 2, 16),
    POPOVER_WIDTH - 16,
  );

  // Cap height first so offsetHeight reflects the constrained layout.
  const maxHeight = preferBelow
    ? Math.min(MAX_POPOVER_HEIGHT, Math.max(200, spaceBelow))
    : Math.min(MAX_POPOVER_HEIGHT, Math.max(200, spaceAbove));
  el.style.maxHeight = `${maxHeight}px`;
  el.style.left = `${left}px`;

  if (preferBelow) {
    el.style.top = `${rect.bottom + POPOVER_GAP}px`;
    el.style.transformOrigin = `${originX}px 0`;
  } else {
    // Align to actual rendered height so short content sits just above the badge.
    const measured = el.offsetHeight || maxHeight;
    const height = Math.min(measured, maxHeight);
    el.style.top = `${Math.max(VIEWPORT_PAD, rect.top - POPOVER_GAP - height)}px`;
    el.style.transformOrigin = `${originX}px 100%`;
  }

  el.dataset.ready = "true";
}

export default function UpdateModal({
  visible,
  update,
  installing,
  installProgress,
  anchorRef,
  onClose,
  onInstall,
  onCancelInstall,
  onSkipVersion,
}: UpdateModalProps) {
  const { t } = useTranslation(["updater", "common"]);
  const installingRef = useRef(installing);
  const onCancelInstallRef = useRef(onCancelInstall);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    installingRef.current = installing;
  }, [installing]);
  useEffect(() => {
    onCancelInstallRef.current = onCancelInstall;
  }, [onCancelInstall]);
  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  // Esc / × : during download → cancel install (don't orphan a background download).
  // Outside click while installing is ignored (see document listener).
  const requestDismiss = useCallback(() => {
    if (installingRef.current) {
      void onCancelInstallRef.current?.();
      return;
    }
    onCloseRef.current();
  }, []);

  const dialogRef = useDialogA11y(visible, requestDismiss);
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

  const setDialogNode = useCallback(
    (node: HTMLDivElement | null) => {
      dialogRef.current = node;
      if (node && anchorRef.current) {
        positionPopover(node, anchorRef.current);
      }
    },
    [anchorRef, dialogRef],
  );

  useEffect(() => {
    if (!visible) return;

    const onReposition = () => {
      const el = dialogRef.current;
      const anchor = anchorRef.current;
      if (el && anchor) positionPopover(el, anchor);
    };

    window.addEventListener("resize", onReposition);
    window.addEventListener("scroll", onReposition, true);
    return () => {
      window.removeEventListener("resize", onReposition);
      window.removeEventListener("scroll", onReposition, true);
    };
  }, [visible, anchorRef, dialogRef]);

  // Outside click: close when idle; ignore while installing.
  // Exclude the version anchor so its onClick can toggle the popover.
  // Delay attach so the opening click does not immediately dismiss.
  useEffect(() => {
    if (!visible) return;

    const onPointerDown = (event: PointerEvent) => {
      if (installingRef.current) return;
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (dialogRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onCloseRef.current();
    };

    const timer = window.setTimeout(() => {
      document.addEventListener("pointerdown", onPointerDown, true);
    }, 0);

    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [visible, anchorRef, dialogRef]);

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
    <div className="update-popover-layer" aria-hidden={false}>
      <div
        ref={setDialogNode}
        className="update-popover"
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-modal-title"
        tabIndex={-1}
      >
        <button
          className="modal-close"
          onClick={requestDismiss}
          aria-label={
            installing
              ? t("updater:button.cancelDownload")
              : t("updater:modal.closeAriaLabel")
          }
        >
          ×
        </button>

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
                : { __html: `<p>${t("updater:modal.noNotes")}</p>` }
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
              {t("updater:modal.viewReleaseNotes")} ↗
            </a>
          </div>
        ) : null}

        <div className="settings-footer update-popover-footer">
          {installing ? (
            <>
              <button className="nav-button" onClick={() => { void onCancelInstall?.(); }}>
                {t("updater:button.cancelDownload")}
              </button>
              <button className="nav-button" disabled>
                {t("updater:status.downloading")} {installProgress != null ? `${installProgress}%` : ""}
              </button>
            </>
          ) : (
            <>
              <button className="nav-button" onClick={onClose}>{t("common:button.cancel")}</button>
              {onSkipVersion ? (
                <button className="nav-button" onClick={() => { void onSkipVersion(); }}>{t("updater:button.skipVersion")}</button>
              ) : null}
              <button className="nav-button" onClick={() => { void onInstall(); }}>{t("updater:button.downloadInstall")}</button>
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

  if (diffMins < 1) return i18n.t("updater:relative.justNow");
  if (diffMins < 60) return i18n.t("updater:relative.minutesAgo", { n: diffMins });
  if (diffHours < 24) return i18n.t("updater:relative.hoursAgo", { n: diffHours });
  if (diffDays < 30) return i18n.t("updater:relative.daysAgo", { n: diffDays });

  // Fall back to an absolute date, localized to the active UI language.
  return date.toLocaleDateString(i18n.language, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}
