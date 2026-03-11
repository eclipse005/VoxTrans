import type { AppState, SettingsAction } from "./appReducer";

export function reduceSettingsState(state: AppState, action: SettingsAction): AppState {
  switch (action.type) {
    case "set_settings":
      return { ...state, settings: action.settings };
    case "set_draft":
      return { ...state, ...action.payload };
    case "set_toast":
      return { ...state, toast: action.toast };
    case "set_term_form":
      return { ...state, ...action.payload };
    case "set_term_editing":
      return { ...state, ...action.payload };
    case "set_terms":
      return { ...state, terms: action.terms };
    case "add_term":
      return { ...state, terms: [action.term, ...state.terms] };
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
