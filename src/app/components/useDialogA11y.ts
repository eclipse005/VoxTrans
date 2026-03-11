import { useEffect, useRef } from "react";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function getFocusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (el) => !el.hasAttribute("disabled") && el.getAttribute("aria-hidden") !== "true",
  );
}

export function useDialogA11y(visible: boolean, onClose: () => void) {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const lastFocusedRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  const wasVisibleRef = useRef(false);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!visible) {
      wasVisibleRef.current = false;
      return;
    }

    // Only apply initial focus when dialog transitions from hidden -> visible.
    const isFirstOpen = !wasVisibleRef.current;
    wasVisibleRef.current = true;

    const dialog = dialogRef.current;
    if (!dialog) return;

    if (isFirstOpen) {
      lastFocusedRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      const focusables = getFocusableElements(dialog);
      if (focusables.length > 0) {
        focusables[0].focus();
      } else {
        dialog.focus();
      }
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (!dialogRef.current) return;
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;

      const targets = getFocusableElements(dialogRef.current);
      if (targets.length === 0) {
        event.preventDefault();
        dialogRef.current.focus();
        return;
      }

      const first = targets[0];
      const last = targets[targets.length - 1];
      const current = document.activeElement as HTMLElement | null;

      if (event.shiftKey) {
        if (!current || current === first || !dialogRef.current.contains(current)) {
          event.preventDefault();
          last.focus();
        }
      } else if (!current || current === last || !dialogRef.current.contains(current)) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      const last = lastFocusedRef.current;
      if (last && document.contains(last)) {
        last.focus();
      }
    };
  }, [visible]);

  return dialogRef;
}
