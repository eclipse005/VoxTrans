import type { QueueItem, SavedSettings, SubtitleCue } from "../../features/media/types";
import type { HotwordCorrection, SettingsTab, SubtitleSaveState, TermEntry, ToastState, UploadTab } from "../types";
import { reduceQueueState } from "./queueReducer";
import { reduceSettingsState } from "./settingsReducer";
import { reduceSubtitleState } from "./subtitleReducer";

export type AppState = {
  queue: QueueItem[];
  activeId: string;
  dragActive: boolean;
  activeTab: UploadTab;
  showSettings: boolean;
  showGlossary: boolean;
  showLogs: boolean;
  settings: SavedSettings;
  draftProvider: SavedSettings["provider"];
  draftChunkInput: string;
  settingsTab: SettingsTab;
  draftApiKey: string;
  draftApiBase: string;
  draftApiModel: string;
  draftAutoPunc: boolean;
  draftThreadsInput: string;
  hotwordCorrection: HotwordCorrection;
  terms: TermEntry[];
  termSource: string;
  termTarget: string;
  termNote: string;
  termSearch: string;
  showImportTerms: boolean;
  importTermsText: string;
  selectedTermId: string | null;
  editingTermId: string | null;
  editSource: string;
  editTarget: string;
  editNote: string;
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
      | "showGlossary"
      | "showLogs"
      | "showImportTerms"
      | "youtubeUrl"
      | "youtubeQuality"
      | "settingsTab"
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
          | "draftApiKey"
          | "draftApiBase"
          | "draftApiModel"
          | "draftAutoPunc"
          | "draftThreadsInput"
          | "hotwordCorrection"
        >
      >;
    }
  | { type: "set_toast"; toast: ToastState | null }
  | {
      type: "set_term_form";
      payload: Partial<Pick<AppState, "termSource" | "termTarget" | "termNote" | "termSearch" | "importTermsText">>;
    }
  | {
      type: "set_term_editing";
      payload: Partial<Pick<AppState, "selectedTermId" | "editingTermId" | "editSource" | "editTarget" | "editNote">>;
    }
  | { type: "set_terms"; terms: TermEntry[] }
  | { type: "add_term"; term: TermEntry }
  | { type: "remove_term"; id: string }
  | { type: "update_term"; id: string; source: string; target: string; note: string };

export type AppAction = UiAction | QueueAction | SubtitleAction | SettingsAction;

export const defaultSettings: SavedSettings = {
  provider: "cuda",
  chunkTargetSeconds: 300,
  autoPunc: true,
  threads: 4,
};

const defaultHotwordCorrection: HotwordCorrection = {
  enabled: true,
  activeGroupId: "group-0",
  groups: [{ id: "group-0", name: "默认分组", keyterms: [] }],
};

export const initialAppState: AppState = {
  queue: [],
  activeId: "",
  dragActive: false,
  activeTab: "local",
  showSettings: false,
  showGlossary: false,
  showLogs: false,
  settings: defaultSettings,
  draftProvider: defaultSettings.provider,
  draftChunkInput: String(defaultSettings.chunkTargetSeconds),
  settingsTab: "transcribe",
  draftApiKey: "",
  draftApiBase: "",
  draftApiModel: "",
  draftAutoPunc: true,
  draftThreadsInput: String(defaultSettings.threads),
  hotwordCorrection: defaultHotwordCorrection,
  terms: [],
  termSource: "",
  termTarget: "",
  termNote: "",
  termSearch: "",
  showImportTerms: false,
  importTermsText: "",
  selectedTermId: null,
  editingTermId: null,
  editSource: "",
  editTarget: "",
  editNote: "",
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
    || action.type === "set_term_form"
    || action.type === "set_term_editing"
    || action.type === "set_terms"
    || action.type === "add_term"
    || action.type === "remove_term"
    || action.type === "update_term"
  );
}

