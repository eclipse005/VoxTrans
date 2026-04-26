import { useEffect, useRef, useState } from "react";
import type { QueueItem, QueueStatus } from "../../features/media/types";
import { formatBytes, statusLabel } from "../../features/media/utils";
import { AudioFileIcon, ChevronDownIcon, MicIcon, TranslateIcon, TrashIcon, VideoFileIcon } from "./Icons";

type QueueBatchMode = "transcribe" | "transcribe_translate";
const QUEUE_BATCH_MODE_KEY = "voxtrans.queueBatchMode.v1";

function loadSavedBatchMode(): QueueBatchMode {
  try {
    const raw = window.localStorage.getItem(QUEUE_BATCH_MODE_KEY);
    if (raw === "transcribe_translate") return "transcribe_translate";
  } catch {
    // Ignore storage errors.
  }
  return "transcribe";
}

function saveBatchMode(mode: QueueBatchMode): void {
  try {
    window.localStorage.setItem(QUEUE_BATCH_MODE_KEY, mode);
  } catch {
    // Ignore storage errors.
  }
}

type MediaListProps = {
  queue: QueueItem[];
  queueCount: number;
  workspaceHydrated: boolean;
  activeId: string;
  isProcessing: boolean;
  onSetActiveId: (id: string) => void;
  onProcessQueue: (mode: QueueBatchMode) => void | Promise<void>;
  onClearQueue: () => void;
  onProcessSingle: (item: QueueItem) => void | Promise<void>;
  onProcessSingleTranscribeTranslate: (item: QueueItem) => void | Promise<void>;
  onRemoveItem: (id: string) => void;
};

function inferMediaKind(item: QueueItem): "audio" | "video" {
  const probe = `${item.path} ${item.name}`.toLowerCase();
  if (/\.(mp4|mkv|mov|avi|webm|m4v)\b/.test(probe)) return "video";
  if (/\.(mp3|wav|m4a|flac|aac|ogg|opus)\b/.test(probe)) return "audio";
  return item.mediaKind;
}

function resolvePrimaryStatus(item: QueueItem): QueueStatus {
  return item.transcribeStatus;
}

function getTranscribeProcessingText(item: QueueItem): string {
  const stage = item.taskProgress.stage;
  const rawDetail = stage.detail?.trim() ?? "";
  const detail = rawDetail.startsWith("step_") ? "" : rawDetail;
  const label = stage.code === "subtitleLayout"
    ? ""
    : stage.label.trim() || resolveStageLabel(stage.code);
  if (detail) return label ? `${label} ${detail}` : detail;
  if (shouldShowStageCounter(stage.code) && stage.current > 0 && stage.total > 0) {
    return `${label || "处理中"} ${stage.current}/${stage.total}`;
  }
  if (label) return label;
  return "准备中";
}

function shouldShowStageCounter(code: QueueItem["taskProgress"]["stage"]["code"]): boolean {
  switch (code) {
    case "downloading":
    case "recognizing":
    case "segmenting":
    case "translating":
    case "subtitleLayout":
    case "finalCheck":
      return true;
    default:
      return false;
  }
}

function resolveStageLabel(code: QueueItem["taskProgress"]["stage"]["code"]): string {
  switch (code) {
    case "downloading":
      return "下载中";
    case "preparing":
      return "准备中";
    case "recognizing":
      return "语音识别中";
    case "segmenting":
      return "AI断句中";
    case "summarizing":
      return "总结中";
    case "terminology":
      return "术语提取中";
    case "translating":
      return "翻译中";
    case "subtitleLayout":
      return "";
    case "finalCheck":
      return "本地最终检查中";
    default:
      return "";
  }
}

export default function MediaList({
  queue,
  queueCount,
  workspaceHydrated,
  activeId,
  isProcessing,
  onSetActiveId,
  onProcessQueue,
  onClearQueue,
  onProcessSingle,
  onProcessSingleTranscribeTranslate,
  onRemoveItem,
}: MediaListProps) {
  const listBusy = isProcessing || !workspaceHydrated;
  const [batchMode, setBatchMode] = useState<QueueBatchMode>(() => loadSavedBatchMode());
  const [batchMenuOpen, setBatchMenuOpen] = useState(false);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const batchMenuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    saveBatchMode(batchMode);
  }, [batchMode]);

  useEffect(() => {
    if (!batchMenuOpen) return;
    const onMouseDown = (event: MouseEvent) => {
      if (!batchMenuRef.current) return;
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (!batchMenuRef.current.contains(target)) {
        setBatchMenuOpen(false);
      }
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, [batchMenuOpen]);

  const modeLabel = batchMode === "transcribe" ? "转录" : "转译";

  return (
    <div className="apple-animate-on-scroll apple-delay-200 file-list-section animated">
      <div className="file-list-header">
        <span className="file-count">{workspaceHydrated ? `共 ${queueCount} 个媒体` : "加载任务中..."}</span>
        <div className="file-list-actions">
          <div className="file-list-split-btn" ref={batchMenuRef}>
            <button
              className="file-list-icon-btn file-list-split-btn-main"
              disabled={listBusy}
              onClick={() => { void onProcessQueue(batchMode); }}
              title={`全部开始（${modeLabel}）`}
              aria-label={`全部开始（${modeLabel}）`}
            >
              {batchMode === "transcribe" ? <MicIcon /> : <TranslateIcon />}
            </button>
            <button
              className="file-list-icon-btn file-list-split-btn-toggle"
              disabled={listBusy}
              onClick={() => setBatchMenuOpen((prev) => !prev)}
              title="选择批量模式"
              aria-label="选择批量模式"
            >
              <ChevronDownIcon />
            </button>
            {batchMenuOpen ? (
              <div className="file-list-split-menu" role="menu">
                <button
                  className={`file-list-split-menu-item ${batchMode === "transcribe" ? "active" : ""}`}
                  onClick={() => {
                    setBatchMode("transcribe");
                    setBatchMenuOpen(false);
                  }}
                  role="menuitem"
                >
                  全部转录
                </button>
                <button
                  className={`file-list-split-menu-item ${batchMode === "transcribe_translate" ? "active" : ""}`}
                  onClick={() => {
                    setBatchMode("transcribe_translate");
                    setBatchMenuOpen(false);
                  }}
                  role="menuitem"
                >
                  全部转译
                </button>
              </div>
            ) : null}
          </div>
          <button
            className="file-list-icon-btn file-list-icon-btn-danger"
            disabled={listBusy}
            onClick={() => setClearConfirmOpen(true)}
            title="清空"
            aria-label="清空"
          >
            <TrashIcon />
          </button>
        </div>
      </div>

      <div className="file-list">
        {queue.length === 0 ? (
          <div className="file-item file-item-empty" />
        ) : (
          queue.map((item) => {
            const primaryStatus = resolvePrimaryStatus(item);
            const transcribeProcessing =
              item.transcribeStatus === "processing"
              && (
                item.taskProgress.stage.code !== ""
                || item.taskProgress.stage.detail.trim().length > 0
                || item.taskProgress.stage.current > 0
                || item.taskProgress.stage.total > 0
              );
            const transcribeProgressText = transcribeProcessing ? getTranscribeProcessingText(item) : "";

            return (
              <div key={item.id} className={`file-item ${item.id === activeId ? "active" : ""}`} onClick={() => onSetActiveId(item.id)}>
                <div className="file-info">
                  <div className="file-icon">{inferMediaKind(item) === "video" ? <VideoFileIcon /> : <AudioFileIcon />}</div>
                  <div className="file-details">
                    <div className="file-name" title={item.name}>
                      {item.name}
                    </div>
                    <div className="file-bottom-row">
                      <div className="file-meta-stack">
                        <div className="file-meta">{formatBytes(item.sizeBytes)}</div>
                        <div className="file-task-info">
                          {transcribeProcessing ? (
                            <span className="task-step task-step-progress">
                              {transcribeProgressText}
                            </span>
                          ) : primaryStatus === "processing" ? (
                            <span className="task-status status-processing">准备中</span>
                          ) : (
                            <span className={`task-status status-${primaryStatus}`}>
                              {statusLabel(primaryStatus)}
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="file-actions">
                        <button className="file-action-btn" title="转译" disabled={item.transcribeStatus === "processing"} onClick={(e) => { e.stopPropagation(); void onProcessSingleTranscribeTranslate(item); }}>
                          <TranslateIcon />
                        </button>
                        <button className="file-action-btn" title="转录" disabled={item.transcribeStatus === "processing"} onClick={(e) => { e.stopPropagation(); void onProcessSingle(item); }}>
                          <MicIcon />
                        </button>
                        <button className="file-action-btn delete" title="删除" onClick={(e) => { e.stopPropagation(); onRemoveItem(item.id); }}>
                          <TrashIcon />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      {clearConfirmOpen ? (
        <div className="file-list-confirm-backdrop" role="dialog" aria-modal="true" aria-label="确认清空任务列表">
          <div className="file-list-confirm-card">
            <div className="file-list-confirm-title">确认清空任务列表？</div>
            <div className="file-list-confirm-text">该操作不可恢复。</div>
            <div className="file-list-confirm-actions">
              <button
                className="file-list-confirm-btn"
                onClick={() => setClearConfirmOpen(false)}
              >
                取消
              </button>
              <button
                className="file-list-confirm-btn file-list-confirm-btn-danger"
                onClick={() => {
                  setClearConfirmOpen(false);
                  void onClearQueue();
                }}
              >
                确认清空
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
