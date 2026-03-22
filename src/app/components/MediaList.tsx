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
  const detail = item.transcribePhaseDetail?.trim() ?? "";
  if (item.transcribePhase === "downloading") {
    return detail ? `下载中 ${detail}` : "下载中";
  }
  if (item.transcribePhase === "initializing") {
    return detail ? `转录准备中 ${detail}` : "转录准备中";
  }
  if (item.transcribePhase === "separating") {
    return detail ? `人声分离中 ${detail}` : "人声分离中";
  }
  if (item.transcribePhase === "recognizing") {
    return detail ? `转录中 ${detail}` : "转录中";
  }
  if (item.transcribePhase === "punctuate") {
    return detail ? `标点优化中 ${detail}` : "标点优化中";
  }
  if (item.transcribePhase === "correct") {
    return detail ? `识别矫正中 ${detail}` : "识别矫正中";
  }
  if (item.transcribePhase === "segment") {
    return detail ? `切分中 ${detail}` : "切分中";
  }
  if (item.transcribePhase === "summarize") {
    return detail ? `总结中 ${detail}` : "总结中";
  }
  if (item.transcribePhase === "translate") {
    return detail ? `翻译中 ${detail}` : "翻译中";
  }
  if (item.transcribePhase === "qa") {
    return detail ? `质量复核中 ${detail}` : "质量复核中";
  }
  if (item.transcribePhase === "qa_quality") {
    return detail ? `润色中 ${detail}` : "润色中";
  }
  if (item.transcribePhase === "qa_layout") {
    return detail ? `观感优化中 ${detail}` : "观感优化中";
  }
  if (detail) return detail;
  return "处理中";
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
          <button className="file-list-icon-btn file-list-icon-btn-danger" disabled={listBusy} onClick={onClearQueue} title="清空" aria-label="清空">
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
            const transcribeProcessing = item.transcribeStatus === "processing";
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
                          ) : primaryStatus === "processing" && item.transcribeSegmentTotal <= 0 ? (
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
    </div>
  );
}
