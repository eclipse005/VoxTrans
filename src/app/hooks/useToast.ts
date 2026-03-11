import { useCallback, useEffect, useRef } from "react";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";

type DispatchState = (action: AppAction) => void;

export function useToast(dispatch: DispatchState) {
  const toastTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current != null) {
        window.clearTimeout(toastTimerRef.current);
      }
    };
  }, []);

  const pushToast = useCallback((message: string, tone: ToastTone = "info") => {
    if (toastTimerRef.current) {
      window.clearTimeout(toastTimerRef.current);
    }
    const id = Date.now();
    dispatch({ type: "set_toast", toast: { id, message, tone } });
    toastTimerRef.current = window.setTimeout(() => {
      dispatch({ type: "set_toast", toast: null });
      toastTimerRef.current = null;
    }, 2200);
  }, [dispatch]);

  return { pushToast };
}
