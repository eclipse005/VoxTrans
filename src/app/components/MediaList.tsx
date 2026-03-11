import type { QueueItem } from "../../features/media/types";
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
          queue.map((item) => (
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
                        {item.status === "processing" && item.segmentTotal <= 0 ? (
                          <span className="task-status status-processing">准备中</span>
                        ) : item.status === "processing" ? null : (
                          <span className={`task-status status-${item.status}`}>
                            {statusLabel(item.status)}
                          </span>
                        )}
                        {item.status === "processing" && item.segmentTotal > 1 ? (
                          <span key={`${item.id}-${item.segmentCurrent}-${item.segmentTotal}`} className="task-step task-step-progress">
                            {`处理中 ${Math.min(item.segmentCurrent || 0, item.segmentTotal)}/${item.segmentTotal}`}
                          </span>
                        ) : null}
                      </div>
                    </div>
                    <div className="file-actions">
                      <button className="file-action-btn" title="转译" onClick={(e) => { e.stopPropagation(); onTranslateSingle(item); }}>
                        <TranslateIcon />
                      </button>
                      <button className="file-action-btn" title="转录" disabled={item.status === "processing"} onClick={(e) => { e.stopPropagation(); void onProcessSingle(item); }}>
                        <MicIcon />
                      </button>
                      <button className="file-action-btn delete" title="删除" disabled={item.status === "processing"} onClick={(e) => { e.stopPropagation(); onRemoveItem(item.id); }}>
                        <TrashIcon />
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
