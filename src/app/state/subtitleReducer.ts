import type { AppState, SubtitleAction } from "./appReducer";

export function reduceSubtitleState(state: AppState, action: SubtitleAction): AppState {
  switch (action.type) {
    case "set_subtitle":
      return { ...state, ...action.payload };
    default:
      return state;
  }
}
