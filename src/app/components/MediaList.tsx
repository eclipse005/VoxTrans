import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import i18n from "../../i18n";
import {
  DEFAULT_SOURCE_LANGUAGE,
  DEFAULT_TARGET_LANGUAGE,
  normalizeSourceLanguage,
  normalizeTargetLanguage,
  SOURCE_LANGUAGE_OPTIONS,
  TARGET_LANGUAGE_OPTIONS,
  targetLanguageOption,
} from "../../features/media/languages";
import type {
  LanguageTag,
  QueueItem,
  QueueStatus,
  TargetLanguage,
  TerminologyGroup,
} from "../../features/media/types";
import { canDeleteQueueItem } from "../../features/media/queuePolicy";
import { formatBytes, statusLabel } from "../../features/media/utils";
import { useClickOutside } from "../hooks/useClickOutside";
import { useSourceLanguages } from "../hooks/useSourceLanguages";
import type { AsrModel } from "../../generated/bindings/AsrModel";
import type { AlignModel } from "../../generated/bindings/AlignModel";
import type { SourceLanguageOption } from "../../generated/bindings/SourceLanguageOption";
import { AudioFileIcon, BookIcon, ChevronDownIcon, MicIcon, TranslateIcon, TrashIcon, VideoFileIcon } from "./Icons";

function findSourceLanguageOption(
  tag: string,
  dynamicOptions: SourceLanguageOption[],
): SourceLanguageOption {
  const dynamic = dynamicOptions.find((option) => option.tag === tag);
  if (dynamic) return dynamic;
  const staticOption = SOURCE_LANGUAGE_OPTIONS.find((option) => option.id === tag);
  if (staticOption) {
    return { tag: staticOption.id, label: staticOption.label, short: staticOption.short };
  }
  return { tag: tag as LanguageTag, label: tag, short: tag };
}

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
  asrModel: AsrModel;
  alignModel: AlignModel;
  pushToast: (message: string, tone?: "info" | "success" | "error") => void;
  onSetActiveId: (id: string) => void;
  onProcessQueue: (mode: QueueBatchMode) => void | Promise<void>;
  onClearQueue: () => void;
  onProcessSingle: (item: QueueItem) => void | Promise<void>;
  onProcessSingleTranscribeTranslate: (item: QueueItem) => void | Promise<void>;
  onUpdateTaskLanguages: (
    item: QueueItem,
    sourceLang: LanguageTag,
    targetLang: TargetLanguage,
  ) => void | Promise<void>;
  onUpdateAllTaskLanguages: (
    sourceLang?: LanguageTag,
    targetLang?: TargetLanguage,
  ) => void | Promise<void>;
  onUpdateTaskTerminology: (
    item: QueueItem,
    terminologyGroupId: string,
  ) => void | Promise<void>;
  terminologyGroups: TerminologyGroup[];
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

function commonSourceLanguage(queue: QueueItem[]): LanguageTag | null {
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
    return `${label || i18n.t("tasks:stage.processing")} ${stage.current}/${stage.total}`;
  }
  if (label) return label;
  return i18n.t("tasks:stage.preparing");
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
  if (code === "subtitleLayout") return "";
  return i18n.t(`errors:stage.${code}`);
}

export default function MediaList({
  queue,
  queueCount,
  workspaceHydrated,
  activeId,
  isProcessing,
  asrModel,
  alignModel,
  pushToast,
  onSetActiveId,
  onProcessQueue,
  onClearQueue,
  onProcessSingle,
  onProcessSingleTranscribeTranslate,
  onUpdateTaskLanguages,
  onUpdateAllTaskLanguages,
  onUpdateTaskTerminology,
  terminologyGroups,
  onRemoveItem,
}: MediaListProps) {
  const { t } = useTranslation(["tasks", "common", "toasts"]);
  const listBusy = isProcessing || !workspaceHydrated;
  const [batchMode, setBatchMode] = useState<QueueBatchMode>(() => loadSavedBatchMode());
  const [batchMenuOpen, setBatchMenuOpen] = useState(false);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const [languageMenuTaskId, setLanguageMenuTaskId] = useState("");
  const [terminologyMenuTaskId, setTerminologyMenuTaskId] = useState("");
  const batchMenuRef = useRef<HTMLDivElement | null>(null);
  const languageMenuRef = useRef<HTMLDivElement | null>(null);
  const terminologyMenuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    saveBatchMode(batchMode);
  }, [batchMode]);

  useClickOutside(batchMenuRef, batchMenuOpen, () => setBatchMenuOpen(false));
  useClickOutside(languageMenuRef, Boolean(languageMenuTaskId), () => setLanguageMenuTaskId(""));
  useClickOutside(terminologyMenuRef, Boolean(terminologyMenuTaskId), () => setTerminologyMenuTaskId(""));

  const {
    data: rawSourceLanguageOptions = [],
    isLoading: sourceLanguagesLoading,
    error: sourceLanguagesError,
  } = useSourceLanguages(asrModel, alignModel);

  const sourceLanguageOptions: SourceLanguageOption[] = sourceLanguagesError
    ? SOURCE_LANGUAGE_OPTIONS.map(({ id, short, label }) => ({ tag: id, short, label }))
    : rawSourceLanguageOptions;

  const sourceLanguagesErrorShown = useRef(false);
  useEffect(() => {
    if (sourceLanguagesError && !sourceLanguagesErrorShown.current) {
      sourceLanguagesErrorShown.current = true;
      pushToast(t("toasts:sourceLanguage.loadFailed"), "error");
    }
    if (!sourceLanguagesError) {
      sourceLanguagesErrorShown.current = false;
    }
  }, [sourceLanguagesError, pushToast]);

  // When the model combination changes, some previously-selected source
  // languages may no longer be supported. Auto-correct editable items to the
  // first supported option and notify the user.
  const queueRef = useRef(queue);
  useEffect(() => {
    queueRef.current = queue;
  });
  useEffect(() => {
    if (!rawSourceLanguageOptions.length) return;
    const editableItems = queueRef.current.filter(
      (item) =>
        item.transcribeStatus !== "processing" && item.transcribeStatus !== "queued",
    );
    for (const item of editableItems) {
      const stillSupported = rawSourceLanguageOptions.some((o) => o.tag === item.sourceLang);
      if (!stillSupported) {
        const fallback = rawSourceLanguageOptions[0];
        void onUpdateTaskLanguages(item, fallback.tag, item.targetLang);
        pushToast(t("toasts:sourceLanguage.unsupported", { label: fallback.label }), "info");
      }
    }
  }, [rawSourceLanguageOptions, onUpdateTaskLanguages, pushToast]);

  const modeLabel = batchMode === "transcribe"
    ? t("tasks:batch.transcribe")
    : t("tasks:batch.translate");
  const batchSourceLang = commonSourceLanguage(queue);
  const batchTargetLang = commonTargetLanguage(queue);
  const batchSourceOption = findSourceLanguageOption(
    batchSourceLang ?? DEFAULT_SOURCE_LANGUAGE,
    rawSourceLanguageOptions,
  );
  const batchTargetOption = targetLanguageOption(batchTargetLang ?? DEFAULT_TARGET_LANGUAGE);
  const batchLanguageMenuOpen = languageMenuTaskId === BATCH_LANGUAGE_MENU_ID;
  const batchLanguageDisabled = !workspaceHydrated || queue.length === 0;
  const batchLanguageChipText = batchSourceLang && batchTargetLang
    ? `${batchSourceOption.short} -> ${batchTargetOption.short}`
    : t("tasks:language.mixed");

  return (
    <div className="apple-animate-on-scroll apple-delay-200 file-list-section animated">
      <div className="file-list-header">
        <span className="file-count">{workspaceHydrated ? t("tasks:mediaCount", { count: queueCount }) : t("common:loading")}</span>
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
                : t("tasks:language.batchTitle")}
              aria-label={t("tasks:language.batchSet")}
              onClick={() => setLanguageMenuTaskId((current) => (
                current === BATCH_LANGUAGE_MENU_ID ? "" : BATCH_LANGUAGE_MENU_ID
              ))}
            >
              {batchLanguageChipText}
            </button>
            {batchLanguageMenuOpen ? (
              <div className="task-language-popover">
                <label className="task-language-field">
                  <span>{t("tasks:language.audio")}</span>
                  <select
                    className="task-language-select"
                    value={batchSourceLang ?? MIXED_LANGUAGE_VALUE}
                    disabled={sourceLanguagesLoading}
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
                      <option value={MIXED_LANGUAGE_VALUE} disabled>{t("tasks:language.mixedChip")}</option>
                    )}
                    {sourceLanguageOptions.map((option) => (
                      <option key={option.tag} value={option.tag}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="task-language-field">
                  <span>{t("tasks:language.translate")}</span>
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
                      <option value={MIXED_LANGUAGE_VALUE} disabled>{t("tasks:language.mixedChip")}</option>
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
              title={t("tasks:batch.startAll", { mode: modeLabel })}
              aria-label={t("tasks:batch.startAll", { mode: modeLabel })}
            >
              {batchMode === "transcribe" ? <MicIcon /> : <TranslateIcon />}
            </button>
            <button
              className="file-list-icon-btn file-list-split-btn-toggle"
              disabled={listBusy}
              onClick={() => setBatchMenuOpen((prev) => !prev)}
              title={t("tasks:batch.chooseMode")}
              aria-label={t("tasks:batch.chooseMode")}
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
                  {t("tasks:batch.transcribeAll")}
                </button>
                <button
                  className={`file-list-split-menu-item ${batchMode === "transcribe_translate" ? "active" : ""}`}
                  onClick={() => {
                    setBatchMode("transcribe_translate");
                    setBatchMenuOpen(false);
                  }}
                  role="menuitem"
                >
                  {t("tasks:batch.translateAll")}
                </button>
              </div>
            ) : null}
          </div>
          <button
            className="file-list-icon-btn file-list-icon-btn-danger"
            disabled={listBusy}
            onClick={() => setClearConfirmOpen(true)}
            title={t("tasks:action.clear")}
            aria-label={t("tasks:action.clear")}
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
            const sourceOption = findSourceLanguageOption(item.sourceLang, rawSourceLanguageOptions);
            const targetOption = targetLanguageOption(item.targetLang);
            const languageBusy = item.transcribeStatus === "processing" || item.transcribeStatus === "queued";
            const languageMenuOpen = languageMenuTaskId === item.id;
            const terminologyMenuOpen = terminologyMenuTaskId === item.id;
            const terminologyGroupId = item.terminologyGroupId ?? "";
            const terminologySelectedGroup =
              terminologyGroups.find((group) => group.id === terminologyGroupId) ?? null;

            return (
              <div key={item.id} className={`file-item ${item.id === activeId ? "active" : ""}`} onClick={() => onSetActiveId(item.id)}>
                {/* Row 1: inline media tag + title (no truncation). */}
                <div className="file-title-row">
                  <span className="file-type-tag" aria-hidden="true">
                    {inferMediaKind(item) === "video" ? <VideoFileIcon /> : <AudioFileIcon />}
                  </span>
                  <span className="file-name" title={item.name}>
                    {item.name}
                  </span>
                </div>

                {/* Row 2: file size (left) + status/progress badge (right). */}
                <div className="file-status-row">
                  <span className="file-meta">{formatBytes(item.sizeBytes)}</span>
                  {transcribeProcessing ? (
                    <span className="task-step task-step-progress">
                      {transcribeProgressText}
                    </span>
                  ) : primaryStatus === "processing" ? (
                    <span className="task-status status-processing">{t("tasks:stage.preparing")}</span>
                  ) : (
                    <span className={`task-status status-${primaryStatus}`}>
                      {t(statusLabel(primaryStatus))}
                    </span>
                  )}
                </div>

                {/* Row 3: language (and later terminology) chips (left) + action buttons (right). */}
                <div className="file-actions-row">
                  <div className="file-chips">
                    <div
                      className="task-language-menu"
                      ref={languageMenuOpen ? languageMenuRef : undefined}
                      onClick={(event) => event.stopPropagation()}
                    >
                      <button
                        className="task-language-chip"
                        disabled={languageBusy}
                        title={`${sourceOption.label} -> ${targetOption.label}`}
                        aria-label={t("tasks:language.itemAria", { source: sourceOption.label, target: targetOption.label })}
                        onClick={() => setLanguageMenuTaskId((current) => (
                          current === item.id ? "" : item.id
                        ))}
                      >
                        {sourceOption.short} -&gt; {targetOption.short}
                      </button>
                      {languageMenuOpen ? (
                        <div className="task-language-popover">
                          <label className="task-language-field">
                            <span>{t("tasks:language.audio")}</span>
                            <select
                              className="task-language-select"
                              value={normalizeSourceLanguage(item.sourceLang)}
                              disabled={sourceLanguagesLoading}
                              onChange={(event) => {
                                void onUpdateTaskLanguages(
                                  item,
                                  normalizeSourceLanguage(event.currentTarget.value),
                                  normalizeTargetLanguage(item.targetLang),
                                );
                              }}
                            >
                              {sourceLanguageOptions.map((option) => (
                                <option key={option.tag} value={option.tag}>
                                  {option.label}
                                </option>
                              ))}
                            </select>
                          </label>
                          <label className="task-language-field">
                            <span>{t("tasks:language.translate")}</span>
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
                    <div
                      className="task-terminology-menu"
                      ref={terminologyMenuOpen ? terminologyMenuRef : undefined}
                      onClick={(event) => event.stopPropagation()}
                    >
                      <button
                        className={`task-terminology-chip ${terminologySelectedGroup ? "has-group" : "no-group"}`}
                        disabled={languageBusy}
                        title={terminologySelectedGroup ? terminologySelectedGroup.name : t("tasks:terminology.unused")}
                        aria-label={terminologySelectedGroup ? t("tasks:terminology.groupLabel", { name: terminologySelectedGroup.name }) : t("tasks:terminology.unused")}
                        onClick={() => setTerminologyMenuTaskId((current) => (
                          current === item.id ? "" : item.id
                        ))}
                      >
                        <BookIcon />
                        {terminologySelectedGroup ? terminologySelectedGroup.name : t("tasks:terminology.notSet")}
                      </button>
                      {terminologyMenuOpen ? (
                        <div className="task-terminology-popover">
                          <label className="task-language-field">
                            <span>{t("tasks:terminology.group")}</span>
                            <select
                              className="task-language-select"
                              value={terminologyGroupId}
                              onChange={(event) => {
                                setTerminologyMenuTaskId("");
                                void onUpdateTaskTerminology(item, event.currentTarget.value);
                              }}
                            >
                              <option value="">{t("tasks:terminology.none")}</option>
                              {terminologyGroups.length === 0 ? (
                                <option value="" disabled>{t("tasks:terminology.notConfigured")}</option>
                              ) : null}
                              {terminologyGroups.map((group) => (
                                <option key={group.id} value={group.id}>
                                  {group.name}
                                </option>
                              ))}
                            </select>
                          </label>
                        </div>
                      ) : null}
                    </div>
                  </div>
                  <div className="file-actions">
                    <button
                      className="file-action-btn"
                      title={t("tasks:action.translate")}
                      aria-label={t("tasks:action.translate")}
                      disabled={item.transcribeStatus === "processing"}
                      onClick={(event) => { event.stopPropagation(); void onProcessSingleTranscribeTranslate(item); }}
                    >
                      <TranslateIcon />
                    </button>
                    <button
                      className="file-action-btn"
                      title={t("tasks:action.transcribe")}
                      aria-label={t("tasks:action.transcribe")}
                      disabled={item.transcribeStatus === "processing"}
                      onClick={(event) => { event.stopPropagation(); void onProcessSingle(item); }}
                    >
                      <MicIcon />
                    </button>
                    <button
                      className="file-action-btn delete"
                      title={t("tasks:action.delete")}
                      aria-label={t("tasks:action.delete")}
                      disabled={!canDeleteQueueItem(item)}
                      onClick={(event) => { event.stopPropagation(); onRemoveItem(item.id); }}
                    >
                      <TrashIcon />
                    </button>
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      {clearConfirmOpen ? (
        <div className="file-list-confirm-backdrop" role="dialog" aria-modal="true" aria-label={t("tasks:confirm.clearTitle")}>
          <div className="file-list-confirm-card">
            <div className="file-list-confirm-title">{t("tasks:confirm.clearTitle")}</div>
            <div className="file-list-confirm-text">{t("tasks:confirm.clearText")}</div>
            <div className="file-list-confirm-actions">
              <button
                className="file-list-confirm-btn"
                onClick={() => setClearConfirmOpen(false)}
              >
                {t("common:button.cancel")}
              </button>
              <button
                className="file-list-confirm-btn file-list-confirm-btn-danger"
                onClick={() => {
                  setClearConfirmOpen(false);
                  void onClearQueue();
                }}
              >
                {t("tasks:confirm.clearConfirm")}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
