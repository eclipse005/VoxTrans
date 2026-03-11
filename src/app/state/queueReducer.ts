import type { AppState, QueueAction } from "./appReducer";

export function reduceQueueState(state: AppState, action: QueueAction): AppState {
  switch (action.type) {
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
    default:
      return state;
  }
}
