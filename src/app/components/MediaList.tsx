import type { QueueItem } from "../../features/media/types";
import { formatBytes, statusLabel } from "../../features/media/utils";
import { AudioFileIcon, FolderIcon, MicIcon, PlayIcon, TranslateIcon, TrashIcon, VideoFileIcon } from "./Icons";

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
  onOpenFolder: () => void | Promise<void>;
  onRemoveItem: (id: string) => void;
};

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
  onOpenFolder,
  onRemoveItem,
}: MediaListProps) {
  return (
    <div className="apple-animate-on-scroll apple-delay-200 file-list-section animated">
      <div className="file-list-header">
        <h3 className="apple-heading-medium">媒体列表</h3>
        <div className="file-list-actions">
          <span className="file-count">共 {queueCount} 个媒体</span>
          <button className="apple-button" disabled={isProcessing} onClick={onProcessQueue}>
            <PlayIcon />
            {isProcessing ? "处理中..." : "全部开始"}
          </button>
          <button className="apple-button apple-button-secondary" disabled={isProcessing} onClick={onClearQueue}>
            <TrashIcon />
            清空
          </button>
        </div>
      </div>

      <div className="file-list">
        {queue.length === 0 ? (
          <div className="file-item file-item-empty">
            <div className="file-info">
              <div className="file-details">
                <div className="file-name">媒体列表为空</div>
                <div className="file-meta">上传本地文件后，任务会显示在这里。</div>
              </div>
            </div>
          </div>
        ) : (
          queue.map((item) => (
            <div key={item.id} className={`file-item ${item.id === activeId ? "active" : ""}`} onClick={() => onSetActiveId(item.id)}>
              <div className="file-info">
                <div className="file-icon">{item.mediaKind === "video" ? <VideoFileIcon /> : <AudioFileIcon />}</div>
                <div className="file-details">
                  <div className="file-name" title={item.name}>
                    {item.name}
                  </div>
                  <div className="file-meta">{formatBytes(item.sizeBytes)}</div>
                  <div className="file-task-info">
                    <span className={`task-status status-${item.status}`}>{statusLabel(item.status)}</span>
                    {item.rtfx ? <span className="task-step">RTFx {item.rtfx.toFixed(2)}</span> : null}
                  </div>
                </div>
              </div>
              <div className="file-actions">
                <button className="file-action-btn" title="转译" onClick={(e) => { e.stopPropagation(); onTranslateSingle(item); }}>
                  <TranslateIcon />
                </button>
                <button className="file-action-btn" title="转录" disabled={isProcessing} onClick={(e) => { e.stopPropagation(); void onProcessSingle(item); }}>
                  <MicIcon />
                </button>
                <button className="file-action-btn" title="打开目录" onClick={(e) => { e.stopPropagation(); void onOpenFolder(); }}>
                  <FolderIcon />
                </button>
                <button className="file-action-btn delete" title="删除" disabled={isProcessing} onClick={(e) => { e.stopPropagation(); onRemoveItem(item.id); }}>
                  <TrashIcon />
                </button>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
