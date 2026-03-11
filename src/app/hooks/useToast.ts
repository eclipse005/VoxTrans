import { useCallback, useEffect, useRef } from "react";
import type { AppState } from "../state/appReducer";
import type { ToastTone } from "../types";

type PatchState = (payload: Partial<AppState>) => void;

export function useToast(patch: PatchState) {
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
    patch({ toast: { id, message, tone } });
    toastTimerRef.current = window.setTimeout(() => {
      patch({ toast: null });
      toastTimerRef.current = null;
    }, 2200);
  }, [patch]);

  return { pushToast };
}
