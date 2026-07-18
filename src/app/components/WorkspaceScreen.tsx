import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import type { AppAction } from "../state/appReducer";
import type { QueueItem, SubtitleCue } from "../../features/media/types";
import type { QueueBatchMode } from "../hooks/queue/useQueueScheduler";
import { updateTaskReviewFlags } from "../api/workspace";
import { reportError, toUserErrorMessage } from "../utils/errors";
import MediaList from "./MediaList";
import SubtitleEditorModal from "./SubtitleEditorModal";
import UploadPanel from "./UploadPanel";

// Embedded editor never closes; a shared noop keeps the prop referentially
// stable so the memoized editor does not re-render on every parent render.
function noop() {}

type WorkspaceScreenProps = {
  queue: QueueItem[];
  queueCount: number;
  workspaceHydrated: boolean;
  activeId: string;
  activeItem: QueueItem | null;
  activeTab: "local" | "youtube";
  dragActive: boolean;
  youtubeUrl: string;
  ytDlpVersion: string;
  ytDlpUpdating: boolean;
  queueBusy: boolean;
  canEditSubtitle: boolean;
  subtitleTaskName: string;
  subtitleCues: SubtitleCue[];
  subtitleCueWarnings: Record<string, string[]>;
  asrModel: Parameters<typeof MediaList>[0]["asrModel"];
  alignModel: Parameters<typeof MediaList>[0]["alignModel"];
  pushToast: Parameters<typeof MediaList>[0]["pushToast"];
  dispatch: (action: AppAction) => void;
  onPickFiles: () => void | Promise<void>;
  onYoutubeDownload: () => void;
  onUpdateYtDlp: () => void;
  onProcessQueue: (mode?: QueueBatchMode) => void | Promise<void>;
  onClearQueue: () => void | Promise<void>;
  onProcessSingle: (item: QueueItem) => void | Promise<void>;
  onProcessSingleTranscribeTranslate: (item: QueueItem) => void | Promise<void>;
  onUpdateTaskLanguages: Parameters<typeof MediaList>[0]["onUpdateTaskLanguages"];
  onUpdateAllTaskLanguages: Parameters<typeof MediaList>[0]["onUpdateAllTaskLanguages"];
  onUpdateTaskTerminology: Parameters<typeof MediaList>[0]["onUpdateTaskTerminology"];
  terminologyGroups: Parameters<typeof MediaList>[0]["terminologyGroups"];
  onRemoveItem: (id: string) => void | Promise<void>;
  onUpdateCue: Parameters<typeof SubtitleEditorModal>[0]["onUpdateCue"];
  onAddCueAfter: Parameters<typeof SubtitleEditorModal>[0]["onAddCueAfter"];
  onMergeSelected: Parameters<typeof SubtitleEditorModal>[0]["onMergeSelected"];
  onSplitSelected: Parameters<typeof SubtitleEditorModal>[0]["onSplitSelected"];
  onReplaceText: Parameters<typeof SubtitleEditorModal>[0]["onReplaceText"];
  onDeleteCue: Parameters<typeof SubtitleEditorModal>[0]["onDeleteCue"];
  onOpenSubtitleDir: () => void | Promise<void>;
  onOpenSubtitleExport: () => void;
  onOpenLogs: () => void | Promise<void>;
};

export function WorkspaceScreen({
  queue,
  queueCount,
  workspaceHydrated,
  activeId,
  activeItem,
  activeTab,
  dragActive,
  youtubeUrl,
  ytDlpVersion,
  ytDlpUpdating,
  queueBusy,
  canEditSubtitle,
  subtitleTaskName,
  subtitleCues,
  subtitleCueWarnings,
  asrModel,
  alignModel,
  pushToast,
  dispatch,
  onPickFiles,
  onYoutubeDownload,
  onUpdateYtDlp,
  onProcessQueue,
  onClearQueue,
  onProcessSingle,
  onProcessSingleTranscribeTranslate,
  onUpdateTaskLanguages,
  onUpdateAllTaskLanguages,
  onUpdateTaskTerminology,
  terminologyGroups,
  onRemoveItem,
  onUpdateCue,
  onAddCueAfter,
  onMergeSelected,
  onSplitSelected,
  onReplaceText,
  onDeleteCue,
  onOpenSubtitleDir,
  onOpenSubtitleExport,
  onOpenLogs,
}: WorkspaceScreenProps) {
  const { t } = useTranslation(["tasks", "subtitles", "toasts"]);
  // Single source of truth for editor status strings, derived from task
  // state. Keeps the editor components presentational (they just render
  // these) instead of each guessing wording from a boolean.
  const isProcessing = activeItem?.transcribeStatus === "processing";
  const readOnlyReason = canEditSubtitle
    ? ""
    : isProcessing
      ? t("tasks:editor.readOnlyProcessing")
      : t("tasks:editor.readOnlyDone");
  const emptyText = canEditSubtitle
    ? t("tasks:editor.emptyEditable")
    : isProcessing
      ? t("tasks:editor.emptyProcessing")
      : t("tasks:editor.emptyDone");
  const reviewBanner = activeItem?.transcribeStatus === "review_source"
    ? t("subtitles:review.bannerSource")
    : activeItem?.transcribeStatus === "review_target"
      ? t("subtitles:review.bannerTarget")
      : "";

  const activeItemId = activeItem?.id ?? "";
  const onToggleReviewSource = useCallback(async (value: boolean) => {
    if (!activeItemId) return;
    try {
      const updated = await updateTaskReviewFlags({ taskId: activeItemId, reviewSource: value });
      dispatch({
        type: "patch_queue_item",
        id: activeItemId,
        updater: (item) => ({
          ...item,
          reviewSource: updated.reviewSource ?? value,
          reviewTarget: updated.reviewTarget ?? item.reviewTarget,
        }),
      });
    } catch (error) {
      reportError(error, "updateTaskReviewFlags");
      pushToast(toUserErrorMessage(error, t("toasts:queue.enqueueFailed")), "error");
    }
  }, [activeItemId, dispatch, pushToast, t]);

  const onToggleReviewTarget = useCallback(async (value: boolean) => {
    if (!activeItemId) return;
    try {
      const updated = await updateTaskReviewFlags({ taskId: activeItemId, reviewTarget: value });
      dispatch({
        type: "patch_queue_item",
        id: activeItemId,
        updater: (item) => ({
          ...item,
          reviewSource: updated.reviewSource ?? item.reviewSource,
          reviewTarget: updated.reviewTarget ?? value,
        }),
      });
    } catch (error) {
      reportError(error, "updateTaskReviewFlags");
      pushToast(toUserErrorMessage(error, t("toasts:queue.enqueueFailed")), "error");
    }
  }, [activeItemId, dispatch, pushToast, t]);

  const handleSetActiveId = useCallback((id: string) => {
    dispatch({ type: "set_ui", payload: { activeId: activeId === id ? "" : id } });
  }, [activeId, dispatch]);

  return (
    <main className="apple-container apple-section">
      <section className="workspace-left">
        <UploadPanel
          activeTab={activeTab}
          dragActive={dragActive}
          youtubeUrl={youtubeUrl}
          ytDlpVersion={ytDlpVersion}
          ytDlpUpdating={ytDlpUpdating}
          onTabChange={(tab) => dispatch({ type: "set_ui", payload: { activeTab: tab } })}
          onPickFiles={onPickFiles}
          onYoutubeUrlChange={(value) => dispatch({ type: "set_ui", payload: { youtubeUrl: value } })}
          onYoutubeDownload={onYoutubeDownload}
          onUpdateYtDlp={onUpdateYtDlp}
        />

        <MediaList
          queue={queue}
          queueCount={queueCount}
          workspaceHydrated={workspaceHydrated}
          activeId={activeId}
          isProcessing={queueBusy}
          asrModel={asrModel}
          alignModel={alignModel}
          pushToast={pushToast}
          onSetActiveId={handleSetActiveId}
          onProcessQueue={onProcessQueue}
          onClearQueue={onClearQueue}
          onProcessSingle={onProcessSingle}
          onProcessSingleTranscribeTranslate={onProcessSingleTranscribeTranslate}
          onUpdateTaskLanguages={onUpdateTaskLanguages}
          onUpdateAllTaskLanguages={onUpdateAllTaskLanguages}
          onUpdateTaskTerminology={onUpdateTaskTerminology}
          terminologyGroups={terminologyGroups}
          onRemoveItem={onRemoveItem}
        />
      </section>

      <section className="workspace-right">
        <div className={`subtitle-panel-layer subtitle-panel-layer-editor ${activeItem ? "is-visible" : "is-hidden"}`}>
          <SubtitleEditorModal
            embedded
            visible
            canEdit={canEditSubtitle}
            readOnlyReason={readOnlyReason}
            emptyText={emptyText}
            taskName={subtitleTaskName}
            cues={subtitleCues}
            cueWarningsById={subtitleCueWarnings}
            reviewSource={Boolean(activeItem?.reviewSource)}
            reviewTarget={Boolean(activeItem?.reviewTarget)}
            reviewBanner={reviewBanner}
            onToggleReviewSource={activeItem ? onToggleReviewSource : undefined}
            onToggleReviewTarget={activeItem ? onToggleReviewTarget : undefined}
            onUpdateCue={onUpdateCue}
            onAddCueAfter={onAddCueAfter}
            onMergeSelected={onMergeSelected}
            onSplitSelected={onSplitSelected}
            onReplaceText={onReplaceText}
            onDeleteCue={onDeleteCue}
            onOpenSrtDir={onOpenSubtitleDir}
            onExportSrt={onOpenSubtitleExport}
            onOpenLogs={onOpenLogs}
            onClose={noop}
          />
        </div>
        <div className={`subtitle-panel-layer subtitle-panel-layer-empty ${activeItem ? "is-hidden" : "is-visible"}`}>
          <div className="subtitle-panel-empty">
            <h3 className="apple-heading-medium">{t("tasks:editor.title")}</h3>
            <p className="apple-body">{t("tasks:editor.emptyPrompt")}</p>
          </div>
        </div>
      </section>
    </main>
  );
}
