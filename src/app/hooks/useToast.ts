import { useCallback, useEffect, useRef } from "react";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";

type DispatchState = (action: AppAction) => void;
type PushToastOptions = {
  id?: number;
  sticky?: boolean;
  durationMs?: number;
};

export function useToast(dispatch: DispatchState) {
  const toastTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current != null) {
        window.clearTimeout(toastTimerRef.current);
      }
    };
  }, []);

  const pushToast = useCallback((
    message: string,
    tone: ToastTone = "info",
    options: PushToastOptions = {},
  ) => {
    if (toastTimerRef.current) {
      window.clearTimeout(toastTimerRef.current);
    }
    const id = options.id ?? Date.now();
    dispatch({ type: "set_toast", toast: { id, message, tone } });

    if (!options.sticky) {
      const duration = Number.isFinite(options.durationMs) ? Math.max(600, Math.round(options.durationMs ?? 2200)) : 2200;
      toastTimerRef.current = window.setTimeout(() => {
        dispatch({ type: "set_toast", toast: null });
        toastTimerRef.current = null;
      }, duration);
    } else {
      toastTimerRef.current = null;
    }

    return id;
  }, [dispatch]);

  return { pushToast };
}
