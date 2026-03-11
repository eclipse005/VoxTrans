import type { QueueItem, SavedSettings, SubtitleCue } from "../../features/media/types";
import type { HotwordCorrection, SettingsTab, SubtitleSaveState, TermEntry, ToastState, UploadTab } from "../types";

export type AppState = {
  queue: QueueItem[];
  activeId: string;
  dragActive: boolean;
  activeTab: UploadTab;
  showSettings: boolean;
  showGlossary: boolean;
  settings: SavedSettings;
  draftProvider: SavedSettings["provider"];
  draftChunkInput: string;
  settingsTab: SettingsTab;
  draftApiKey: string;
  draftApiBase: string;
  draftApiModel: string;
  draftAutoPunc: boolean;
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

export type AppAction =
  | { type: "patch"; payload: Partial<AppState> }
  | { type: "add_queue_items"; items: QueueItem[] }
  | { type: "patch_queue_item"; id: string; updater: (item: QueueItem) => QueueItem }
  | { type: "remove_queue_item"; id: string }
  | { type: "clear_queue" }
  | { type: "set_terms"; terms: TermEntry[] }
  | { type: "add_term"; term: TermEntry }
  | { type: "remove_term"; id: string }
  | { type: "update_term"; id: string; source: string; target: string; note: string };

export const defaultSettings: SavedSettings = {
  provider: "cuda",
  chunkTargetSeconds: 300,
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
  settings: defaultSettings,
  draftProvider: defaultSettings.provider,
  draftChunkInput: String(defaultSettings.chunkTargetSeconds),
  settingsTab: "transcribe",
  draftApiKey: "",
  draftApiBase: "",
  draftApiModel: "",
  draftAutoPunc: true,
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
  switch (action.type) {
    case "patch":
      return { ...state, ...action.payload };
    case "add_queue_items": {
      const existed = new Set(state.queue.map((item) => item.path));
      const toAdd = action.items.filter((item) => !existed.has(item.path));
      return {
        ...state,
        queue: [...state.queue, ...toAdd],
        activeId: state.activeId || toAdd[0]?.id || state.activeId,
      };
    }
    case "patch_queue_item":
      return {
        ...state,
        queue: state.queue.map((item) => (item.id === action.id ? action.updater(item) : item)),
      };
    case "remove_queue_item":
      return {
        ...state,
        queue: state.queue.filter((item) => item.id !== action.id),
        activeId: state.activeId === action.id ? "" : state.activeId,
      };
    case "clear_queue":
      return {
        ...state,
        queue: [],
        activeId: "",
      };
    case "set_terms":
      return {
        ...state,
        terms: action.terms,
      };
    case "add_term":
      return {
        ...state,
        terms: [action.term, ...state.terms],
      };
    case "remove_term":
      return {
        ...state,
        terms: state.terms.filter((item) => item.id !== action.id),
        selectedTermId: state.selectedTermId === action.id ? null : state.selectedTermId,
        editingTermId: state.editingTermId === action.id ? null : state.editingTermId,
      };
    case "update_term":
      return {
        ...state,
        terms: state.terms.map((item) =>
          item.id === action.id
            ? {
                ...item,
                source: action.source,
                target: action.target,
                note: action.note,
              }
            : item,
        ),
      };
    default:
      return state;
  }
}
