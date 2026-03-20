import type { QueueItem, SavedSettings, SubtitleCue } from "../../features/media/types";
import { normalizeProvider } from "../../features/media/provider";
import type { ToastState, UploadTab } from "../types";
import { reduceQueueState } from "./queueReducer";
import { reduceSettingsState } from "./settingsReducer";
import { reduceSubtitleState } from "./subtitleReducer";
import { createTerminologyGroup } from "../utils/terminology";

export type AppState = {
  queue: QueueItem[];
  activeId: string;
  dragActive: boolean;
  activeTab: UploadTab;
  showSettings: boolean;
  showLogs: boolean;
  settings: SavedSettings;
  draftProvider: SavedSettings["provider"];
  draftChunkInput: string;
  draftSubtitleMaxWordsInput: string;
  draftAsrModel: SavedSettings["asrModel"];
  draftDemucsModel: SavedSettings["demucsModel"];
  draftEnableVocalSeparation: boolean;
  draftTranslateApiKey: string;
  draftTranslateBaseUrl: string;
  draftTranslateModel: string;
  draftLlmConcurrencyInput: string;
  draftTerminologyGroups: SavedSettings["terminologyGroups"];
  draftEnableTerminology: boolean;
  draftEnablePunctuationOptimization: boolean;
  draftEnableAsrCorrection: boolean;
  draftEnableSubtitleBeautify: boolean;
  youtubeUrl: string;
  youtubeQuality: string;
  toast: ToastState | null;
  showSubtitleEditor: boolean;
  subtitleTaskId: string;
  subtitleTaskName: string;
  subtitleMediaPath: string;
  subtitleSrtPath: string;
  subtitleCues: SubtitleCue[];
  subtitleCueWarnings: Record<string, string[]>;
  subtitleSelectedCueId: string;
  subtitleDirty: boolean;
};

export type UiAction = {
  type: "set_ui";
  payload: Partial<
    Pick<
      AppState,
      | "activeId"
      | "dragActive"
      | "activeTab"
      | "showSettings"
      | "showLogs"
      | "youtubeUrl"
      | "youtubeQuality"
    >
  >;
};

export type QueueAction =
  | { type: "add_queue_items"; items: QueueItem[] }
  | { type: "patch_queue_item"; id: string; updater: (item: QueueItem) => QueueItem }
  | { type: "remove_queue_item"; id: string }
  | { type: "clear_queue" };

export type SubtitleAction = {
  type: "set_subtitle";
  payload: Partial<
    Pick<
      AppState,
      | "subtitleTaskId"
      | "subtitleTaskName"
      | "subtitleMediaPath"
      | "subtitleSrtPath"
      | "subtitleCues"
      | "subtitleCueWarnings"
      | "subtitleDirty"
    >
  >;
};

export type SettingsAction =
  | { type: "set_settings"; settings: SavedSettings }
  | {
      type: "set_draft";
      payload: Partial<
        Pick<
          AppState,
          | "draftProvider"
          | "draftChunkInput"
          | "draftSubtitleMaxWordsInput"
          | "draftAsrModel"
          | "draftDemucsModel"
          | "draftEnableVocalSeparation"
          | "draftTranslateApiKey"
          | "draftTranslateBaseUrl"
          | "draftTranslateModel"
          | "draftLlmConcurrencyInput"
          | "draftTerminologyGroups"
          | "draftEnableTerminology"
          | "draftEnablePunctuationOptimization"
          | "draftEnableAsrCorrection"
          | "draftEnableSubtitleBeautify"
        >
      >;
    }
  | { type: "set_toast"; toast: ToastState | null };

export type AppAction = UiAction | QueueAction | SubtitleAction | SettingsAction;

export const defaultSettings: SavedSettings = {
  provider: normalizeProvider(undefined),
  chunkTargetSeconds: 300,
  subtitleMaxWordsPerSegment: 20,
  asrModel: "parakeet-tdt-0.6b-v2",
  demucsModel: "htdemucs_ft",
  enableVocalSeparation: false,
  translateApiKey: "",
  translateBaseUrl: "https://api.openai.com/v1",
  translateModel: "gpt-4.1-mini",
  llmConcurrency: 4,
  terminologyGroups: [createTerminologyGroup()],
  enableTerminology: true,
  enablePunctuationOptimization: false,
  enableAsrCorrection: true,
  enableSubtitleBeautify: true,
};

export const initialAppState: AppState = {
  queue: [],
  activeId: "",
  dragActive: false,
  activeTab: "local",
  showSettings: false,
  showLogs: false,
  settings: defaultSettings,
  draftProvider: defaultSettings.provider,
  draftChunkInput: String(defaultSettings.chunkTargetSeconds),
  draftSubtitleMaxWordsInput: String(defaultSettings.subtitleMaxWordsPerSegment),
  draftAsrModel: defaultSettings.asrModel,
  draftDemucsModel: defaultSettings.demucsModel,
  draftEnableVocalSeparation: defaultSettings.enableVocalSeparation,
  draftTranslateApiKey: defaultSettings.translateApiKey,
  draftTranslateBaseUrl: defaultSettings.translateBaseUrl,
  draftTranslateModel: defaultSettings.translateModel,
  draftLlmConcurrencyInput: String(defaultSettings.llmConcurrency),
  draftTerminologyGroups: defaultSettings.terminologyGroups,
  draftEnableTerminology: defaultSettings.enableTerminology,
  draftEnablePunctuationOptimization: defaultSettings.enablePunctuationOptimization,
  draftEnableAsrCorrection: defaultSettings.enableAsrCorrection,
  draftEnableSubtitleBeautify: defaultSettings.enableSubtitleBeautify,
  youtubeUrl: "",
  youtubeQuality: "",
  toast: null,
  showSubtitleEditor: false,
  subtitleTaskId: "",
  subtitleTaskName: "",
  subtitleMediaPath: "",
  subtitleSrtPath: "",
  subtitleCues: [],
  subtitleCueWarnings: {},
  subtitleSelectedCueId: "",
  subtitleDirty: false,
};

export function appReducer(state: AppState, action: AppAction): AppState {
  if (action.type === "set_ui") {
    return { ...state, ...action.payload };
  }

  let nextState = state;
  if (isQueueAction(action)) {
    nextState = reduceQueueState(nextState, action);
  }
  if (isSubtitleAction(action)) {
    nextState = reduceSubtitleState(nextState, action);
  }
  if (isSettingsAction(action)) {
    nextState = reduceSettingsState(nextState, action);
  }
  return nextState;
}

function isQueueAction(action: AppAction): action is QueueAction {
  return (
    action.type === "add_queue_items"
    || action.type === "patch_queue_item"
    || action.type === "remove_queue_item"
    || action.type === "clear_queue"
  );
}

function isSubtitleAction(action: AppAction): action is SubtitleAction {
  return action.type === "set_subtitle";
}

function isSettingsAction(action: AppAction): action is SettingsAction {
  return (
    action.type === "set_settings"
    || action.type === "set_draft"
    || action.type === "set_toast"
  );
}
