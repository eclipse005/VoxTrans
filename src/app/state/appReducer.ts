import type { QueueItem, SavedSettings, SubtitleCue } from "../../features/media/types";
import { normalizeProvider } from "../../features/media/provider";
import type { SubtitleSaveState, ToastState, UploadTab } from "../types";
import { reduceQueueState } from "./queueReducer";
import { reduceSettingsState } from "./settingsReducer";
import { reduceSubtitleState } from "./subtitleReducer";

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
  draftEnablePunctuationOptimization: boolean;
  youtubeUrl: string;
  youtubeQuality: string;
  toast: ToastState | null;
  showSubtitleEditor: boolean;
  subtitleTaskId: string;
  subtitleTaskName: string;
  subtitleMediaPath: string;
  subtitleSrtPath: string;
  subtitleDraftPath: string;
  subtitleCues: SubtitleCue[];
  subtitleCueWarnings: Record<string, string[]>;
  subtitleSelectedCueId: string;
  subtitleSaveState: SubtitleSaveState;
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
      | "subtitleDraftPath"
      | "subtitleCues"
      | "subtitleCueWarnings"
      | "subtitleSaveState"
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
          | "draftEnablePunctuationOptimization"
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
  enablePunctuationOptimization: false,
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
  draftEnablePunctuationOptimization: defaultSettings.enablePunctuationOptimization,
  youtubeUrl: "",
  youtubeQuality: "",
  toast: null,
  showSubtitleEditor: false,
  subtitleTaskId: "",
  subtitleTaskName: "",
  subtitleMediaPath: "",
  subtitleSrtPath: "",
  subtitleDraftPath: "",
  subtitleCues: [],
  subtitleCueWarnings: {},
  subtitleSelectedCueId: "",
  subtitleSaveState: "idle",
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
