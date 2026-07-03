import { useEffect, type RefObject } from "react";

/**
 * Close-on-outside-click helper. Calls `onClose` when a `mousedown` event
 * fires outside the referenced element. The effect is a no-op while
 * `active` is false, so callers can gate it on a boolean state (e.g. a
 * menu-open flag) without conditional hooks.
 *
 * Centralizes the three identical inline implementations that previously
 * lived in MediaList.tsx (batchMenu / languageMenu / terminologyMenu).
 */
export function useClickOutside<T extends HTMLElement>(
  ref: RefObject<T | null>,
  active: boolean,
  onClose: () => void,
): void {
  useEffect(() => {
    if (!active) return;
    const onMouseDown = (event: MouseEvent) => {
      const el = ref.current;
      if (!el) return;
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (!el.contains(target)) {
        onClose();
      }
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, [ref, active, onClose]);
}
