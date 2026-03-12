import type { QueueItem, QueueStatus, TranslateStatus } from "../../features/media/types";
import { formatBytes, statusLabel } from "../../features/media/utils";
import { AudioFileIcon, MicIcon, PlayIcon, TranslateIcon, TrashIcon, VideoFileIcon } from "./Icons";

type MediaListProps = {
  queue: QueueItem[];
  queueCount: number;
  activeId: string;
  isProcessing: boolean;
  onSetActiveId: (id: string) => void;
  onProcessQueue: () => void | Promise<void>;
  onClearQueue: () => void;
  onTranslateSingle: (item: QueueItem) => void;
  onProcessSingle: (item: QueueItem) => void | Promise<void>;
  onRemoveItem: (id: string) => void;
};

function inferMediaKind(item: QueueItem): "audio" | "video" {
  const probe = `${item.path} ${item.name}`.toLowerCase();
  if (/\.(mp4|mkv|mov|avi|webm|m4v)\b/.test(probe)) return "video";
  if (/\.(mp3|wav|m4a|flac|aac|ogg|opus)\b/.test(probe)) return "audio";
  return item.mediaKind;
}

function resolvePrimaryStatus(item: QueueItem): QueueStatus {
  if (item.transcribeStatus !== "done") {
    return item.transcribeStatus;
  }

  return mapTranslateToQueueStatus(item.translateStatus);
}

function mapTranslateToQueueStatus(status: TranslateStatus): QueueStatus {
  if (status === "idle") return "done";
  if (status === "queued") return "queued";
  if (status === "processing") return "processing";
  if (status === "done") return "done";
  return "error";
}

function getTranscribeProcessingText(item: QueueItem): string {
  if (item.transcribePhase === "hotword") {
    return "术语矫正中";
  }
  if (item.transcribePhase === "punctuation") {
    return "标点恢复中";
  }
  if (item.transcribePhase === "initializing") {
    return "转录准备中";
  }
  if (item.transcribeSegmentTotal > 1) {
    return `转录处理中 ${Math.min(item.transcribeSegmentCurrent || 0, item.transcribeSegmentTotal)}/${item.transcribeSegmentTotal}`;
  }
  return "转录处理中";
}

export default function MediaList({
  queue,
  queueCount,
  activeId,
  isProcessing,
  onSetActiveId,
  onProcessQueue,
  onClearQueue,
  onTranslateSingle,
  onProcessSingle,
  onRemoveItem,
}: MediaListProps) {
  return (
    <div className="apple-animate-on-scroll apple-delay-200 file-list-section animated">
      <div className="file-list-header">
        <span className="file-count">共 {queueCount} 个媒体</span>
        <div className="file-list-actions">
          <button className="file-list-icon-btn" disabled={isProcessing} onClick={onProcessQueue} title="全部开始" aria-label="全部开始">
            <PlayIcon />
          </button>
          <button className="file-list-icon-btn" disabled={isProcessing} onClick={onClearQueue} title="清空" aria-label="清空">
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
                            <span key={`${item.id}-${item.transcribePhase || ""}-${item.transcribeSegmentCurrent}-${item.transcribeSegmentTotal}`} className="task-step task-step-progress">
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
                        <button className="file-action-btn" title="转译" onClick={(e) => { e.stopPropagation(); onTranslateSingle(item); }}>
                          <TranslateIcon />
                        </button>
                        <button className="file-action-btn" title="转录" disabled={item.transcribeStatus === "processing"} onClick={(e) => { e.stopPropagation(); void onProcessSingle(item); }}>
                          <MicIcon />
                        </button>
                        <button className="file-action-btn delete" title="删除" disabled={item.transcribeStatus === "processing" || item.translateStatus === "processing"} onClick={(e) => { e.stopPropagation(); onRemoveItem(item.id); }}>
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
