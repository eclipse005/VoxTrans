import type { AppState, SettingsAction } from "./appReducer";

export function reduceSettingsState(state: AppState, action: SettingsAction): AppState {
  switch (action.type) {
    case "set_settings":
      return { ...state, settings: action.settings };
    case "set_draft":
      return { ...state, ...action.payload };
    case "set_toast":
      return { ...state, toast: action.toast };
    default:
      return state;
  }
}
