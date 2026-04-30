import { useEffect } from "react";
import clickSoundUrl from "../../assets/sounds/voxtrans-click.ogg";

const INTERACTIVE_SELECTOR = [
  "button",
  "select",
  "input[type='button']",
  "input[type='checkbox']",
  "input[type='radio']",
  "input[type='range']",
  "input[type='submit']",
  "label.setting-toggle",
  "[role='button']",
].join(",");

export function useClickSound(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;

    const audio = new Audio(clickSoundUrl);
    audio.volume = 0.28;
    audio.preload = "auto";

    const onPointerUp = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) return;

      const interactive = target.closest(INTERACTIVE_SELECTOR);
      if (!(interactive instanceof HTMLElement)) return;
      if (interactive.hasAttribute("disabled")) return;
      if (interactive.getAttribute("aria-disabled") === "true") return;

      audio.currentTime = 0;
      void audio.play().catch(() => {});
    };

    window.addEventListener("pointerup", onPointerUp, true);
    return () => {
      window.removeEventListener("pointerup", onPointerUp, true);
    };
  }, [enabled]);
}
