import { useEffect, useRef, useState } from "react";
import {
  DEFAULT_SOURCE_LANGUAGE,
  DEFAULT_TARGET_LANGUAGE,
  normalizeSourceLanguage,
  normalizeTargetLanguage,
  SOURCE_LANGUAGE_OPTIONS,
  sourceLanguageOption,
  TARGET_LANGUAGE_OPTIONS,
  targetLanguageOption,
} from "../../features/media/languages";
import type {
  QueueItem,
  QueueStatus,
  SourceLanguage,
  TargetLanguage,
} from "../../features/media/types";
import { canDeleteQueueItem } from "../../features/media/queuePolicy";
import { formatBytes, statusLabel } from "../../features/media/utils";
import { AudioFileIcon, ChevronDownIcon, MicIcon, TranslateIcon, TrashIcon, VideoFileIcon } from "./Icons";

type QueueBatchMode = "transcribe" | "transcribe_translate";
const QUEUE_BATCH_MODE_KEY = "voxtrans.queueBatchMode.v1";
const BATCH_LANGUAGE_MENU_ID = "__batch_language__";
const MIXED_LANGUAGE_VALUE = "__mixed__";

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
  onUpdateTaskLanguages: (
    item: QueueItem,
    sourceLang: SourceLanguage,
    targetLang: TargetLanguage,
  ) => void | Promise<void>;
  onUpdateAllTaskLanguages: (
    sourceLang?: SourceLanguage,
    targetLang?: TargetLanguage,
  ) => void | Promise<void>;
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

function commonSourceLanguage(queue: QueueItem[]): SourceLanguage | null {
  if (queue.length === 0) return DEFAULT_SOURCE_LANGUAGE;
  const [first, ...rest] = queue.map((item) => normalizeSourceLanguage(item.sourceLang));
  return rest.every((value) => value === first) ? first : null;
}

function commonTargetLanguage(queue: QueueItem[]): TargetLanguage | null {
  if (queue.length === 0) return DEFAULT_TARGET_LANGUAGE;
  const [first, ...rest] = queue.map((item) => normalizeTargetLanguage(item.targetLang));
  return rest.every((value) => value === first) ? first : null;
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
    case "separating":
    case "recognizing":
    case "aligning":
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
    case "separating":
      return "人声分离中";
    case "recognizing":
      return "语音识别中";
    case "aligning":
      return "强制对齐中";
    case "segmenting":
      return "断句中";
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
  onUpdateTaskLanguages,
  onUpdateAllTaskLanguages,
  onRemoveItem,
}: MediaListProps) {
  const listBusy = isProcessing || !workspaceHydrated;
  const [batchMode, setBatchMode] = useState<QueueBatchMode>(() => loadSavedBatchMode());
  const [batchMenuOpen, setBatchMenuOpen] = useState(false);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const [languageMenuTaskId, setLanguageMenuTaskId] = useState("");
  const batchMenuRef = useRef<HTMLDivElement | null>(null);
  const languageMenuRef = useRef<HTMLDivElement | null>(null);

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

  useEffect(() => {
    if (!languageMenuTaskId) return;
    const onMouseDown = (event: MouseEvent) => {
      if (!languageMenuRef.current) return;
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (!languageMenuRef.current.contains(target)) {
        setLanguageMenuTaskId("");
      }
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, [languageMenuTaskId]);

  const modeLabel = batchMode === "transcribe" ? "转录" : "转译";
  const batchSourceLang = commonSourceLanguage(queue);
  const batchTargetLang = commonTargetLanguage(queue);
  const batchSourceOption = sourceLanguageOption(batchSourceLang ?? DEFAULT_SOURCE_LANGUAGE);
  const batchTargetOption = targetLanguageOption(batchTargetLang ?? DEFAULT_TARGET_LANGUAGE);
  const batchLanguageMenuOpen = languageMenuTaskId === BATCH_LANGUAGE_MENU_ID;
  const batchLanguageDisabled = !workspaceHydrated || queue.length === 0;
  const batchLanguageChipText = batchSourceLang && batchTargetLang
    ? `${batchSourceOption.short} -> ${batchTargetOption.short}`
    : "多语言";

  return (
    <div className="apple-animate-on-scroll apple-delay-200 file-list-section animated">
      <div className="file-list-header">
        <span className="file-count">{workspaceHydrated ? `共 ${queueCount} 个媒体` : "加载任务中..."}</span>
        <div className="file-list-actions">
          <div
            className="task-language-menu task-language-menu-batch"
            ref={batchLanguageMenuOpen ? languageMenuRef : undefined}
          >
            <button
              className="task-language-chip task-language-chip-batch"
              disabled={batchLanguageDisabled}
              title={batchSourceLang && batchTargetLang
                ? `${batchSourceOption.label} -> ${batchTargetOption.label}`
                : "批量任务语言"}
              aria-label="批量设置任务语言"
              onClick={() => setLanguageMenuTaskId((current) => (
                current === BATCH_LANGUAGE_MENU_ID ? "" : BATCH_LANGUAGE_MENU_ID
              ))}
            >
              {batchLanguageChipText}
            </button>
            {batchLanguageMenuOpen ? (
              <div className="task-language-popover">
                <label className="task-language-field">
                  <span>音频语言</span>
                  <select
                    className="task-language-select"
                    value={batchSourceLang ?? MIXED_LANGUAGE_VALUE}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      if (value === MIXED_LANGUAGE_VALUE) return;
                      void onUpdateAllTaskLanguages(
                        normalizeSourceLanguage(value),
                        undefined,
                      );
                    }}
                  >
                    {batchSourceLang ? null : (
                      <option value={MIXED_LANGUAGE_VALUE} disabled>多种语言</option>
                    )}
                    {SOURCE_LANGUAGE_OPTIONS.map((option) => (
                      <option key={option.id} value={option.id}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="task-language-field">
                  <span>翻译语言</span>
                  <select
                    className="task-language-select"
                    value={batchTargetLang ?? MIXED_LANGUAGE_VALUE}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      if (value === MIXED_LANGUAGE_VALUE) return;
                      void onUpdateAllTaskLanguages(
                        undefined,
                        normalizeTargetLanguage(value),
                      );
                    }}
                  >
                    {batchTargetLang ? null : (
                      <option value={MIXED_LANGUAGE_VALUE} disabled>多种语言</option>
                    )}
                    {TARGET_LANGUAGE_OPTIONS.map((option) => (
                      <option key={option.id} value={option.id}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            ) : null}
          </div>
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
            const sourceOption = sourceLanguageOption(item.sourceLang);
            const targetOption = targetLanguageOption(item.targetLang);
            const languageBusy = item.transcribeStatus === "processing" || item.transcribeStatus === "queued";
            const languageMenuOpen = languageMenuTaskId === item.id;

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
                        <div className="file-meta-line">
                          <span className="file-meta">{formatBytes(item.sizeBytes)}</span>
                          <div
                            className="task-language-menu"
                            ref={languageMenuOpen ? languageMenuRef : undefined}
                            onClick={(event) => event.stopPropagation()}
                          >
                            <button
                              className="task-language-chip"
                              disabled={languageBusy}
                              title={`${sourceOption.label} -> ${targetOption.label}`}
                              aria-label={`${sourceOption.label} 到 ${targetOption.label}`}
                              onClick={() => setLanguageMenuTaskId((current) => (
                                current === item.id ? "" : item.id
                              ))}
                            >
                              {sourceOption.short} -&gt; {targetOption.short}
                            </button>
                            {languageMenuOpen ? (
                              <div className="task-language-popover">
                                <label className="task-language-field">
                                  <span>音频语言</span>
                                  <select
                                    className="task-language-select"
                                    value={normalizeSourceLanguage(item.sourceLang)}
                                    onChange={(event) => {
                                      void onUpdateTaskLanguages(
                                        item,
                                        normalizeSourceLanguage(event.currentTarget.value),
                                        normalizeTargetLanguage(item.targetLang),
                                      );
                                    }}
                                  >
                                    {SOURCE_LANGUAGE_OPTIONS.map((option) => (
                                      <option key={option.id} value={option.id}>
                                        {option.label}
                                      </option>
                                    ))}
                                  </select>
                                </label>
                                <label className="task-language-field">
                                  <span>翻译语言</span>
                                  <select
                                    className="task-language-select"
                                    value={normalizeTargetLanguage(item.targetLang)}
                                    onChange={(event) => {
                                      void onUpdateTaskLanguages(
                                        item,
                                        normalizeSourceLanguage(item.sourceLang),
                                        normalizeTargetLanguage(event.currentTarget.value),
                                      );
                                    }}
                                  >
                                    {TARGET_LANGUAGE_OPTIONS.map((option) => (
                                      <option key={option.id} value={option.id}>
                                        {option.label}
                                      </option>
                                    ))}
                                  </select>
                                </label>
                              </div>
                            ) : null}
                          </div>
                        </div>
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
                        <button className="file-action-btn delete" title="删除" disabled={!canDeleteQueueItem(item)} onClick={(e) => { e.stopPropagation(); onRemoveItem(item.id); }}>
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
