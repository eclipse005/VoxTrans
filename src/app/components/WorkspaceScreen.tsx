import type { AppAction } from "../state/appReducer";
import type { QueueItem, SubtitleCue } from "../../features/media/types";
import type { QueueBatchMode } from "../hooks/queue/useQueueScheduler";
import MediaList from "./MediaList";
import SubtitleEditorModal from "./SubtitleEditorModal";
import UploadPanel from "./UploadPanel";

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
          onSetActiveId={(id) => dispatch({ type: "set_ui", payload: { activeId: activeId === id ? "" : id } })}
          onProcessQueue={onProcessQueue}
          onClearQueue={onClearQueue}
          onProcessSingle={onProcessSingle}
          onProcessSingleTranscribeTranslate={onProcessSingleTranscribeTranslate}
          onUpdateTaskLanguages={onUpdateTaskLanguages}
          onUpdateAllTaskLanguages={onUpdateAllTaskLanguages}
          onRemoveItem={onRemoveItem}
        />
      </section>

      <section className="workspace-right">
        <div className={`subtitle-panel-layer subtitle-panel-layer-editor ${activeItem ? "is-visible" : "is-hidden"}`}>
          <SubtitleEditorModal
            embedded
            visible
            canEdit={canEditSubtitle}
            readOnlyReason={canEditSubtitle ? "" : "任务完成后才可编辑字幕"}
            taskName={subtitleTaskName}
            cues={subtitleCues}
            cueWarningsById={subtitleCueWarnings}
            onUpdateCue={onUpdateCue}
            onAddCueAfter={onAddCueAfter}
            onMergeSelected={onMergeSelected}
            onSplitSelected={onSplitSelected}
            onReplaceText={onReplaceText}
            onDeleteCue={onDeleteCue}
            onOpenSrtDir={onOpenSubtitleDir}
            onExportSrt={onOpenSubtitleExport}
            onOpenLogs={onOpenLogs}
            onClose={() => {}}
          />
        </div>
        <div className={`subtitle-panel-layer subtitle-panel-layer-empty ${activeItem ? "is-hidden" : "is-visible"}`}>
          <div className="subtitle-panel-empty">
            <h3 className="apple-heading-medium">字幕编辑器</h3>
            <p className="apple-body">请在左侧任务列表中选择一个媒体任务开始编辑字幕。</p>
          </div>
        </div>
      </section>
    </main>
  );
}
