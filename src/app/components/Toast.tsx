import type { ToastState } from "../types";

type ToastProps = {
  toast: ToastState | null;
};

export default function Toast({ toast }: ToastProps) {
  if (!toast) return null;

  return (
    <div className={`app-toast app-toast-${toast.tone}`}>
      {toast.message}
    </div>
  );
}
