import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import type { SubtitleCue } from "../../features/media/types";

type SplitResultItem = {
  sourceCueId: string;
  bornCueId: string;
};

type UseSubtitleBatchAnimationsArgs = {
  cues: SubtitleCue[];
  listContainerRef: RefObject<HTMLDivElement | null>;
  cardRefs: RefObject<Record<string, HTMLElement | null>>;
  currentMatchCueId: string | null;
};

export function useSubtitleBatchAnimations({
  cues,
  listContainerRef,
  cardRefs,
  currentMatchCueId,
}: UseSubtitleBatchAnimationsArgs) {
  const [isBatchAnimating, setIsBatchAnimating] = useState(false);
  const batchTimerRef = useRef<number | null>(null);
  const scrollAnimRafRef = useRef<number | null>(null);
  const pendingSplitRef = useRef<Array<{ bornCueId: string; fromRect: DOMRect }>>([]);

  const scrollToCueWithFixedDuration = useCallback((cueId: string, durationMs = 260) => {
    const container = listContainerRef.current;
    const node = cardRefs.current[cueId];
    if (!container || !node) return;

    if (scrollAnimRafRef.current != null) {
      window.cancelAnimationFrame(scrollAnimRafRef.current);
      scrollAnimRafRef.current = null;
    }

    const containerRect = container.getBoundingClientRect();
    const nodeRect = node.getBoundingClientRect();
    const nodeTopInContainer = nodeRect.top - containerRect.top + container.scrollTop;
    const targetCenterTop = nodeTopInContainer - (container.clientHeight - nodeRect.height) / 2;
    const maxTop = Math.max(container.scrollHeight - container.clientHeight, 0);
    const targetTop = Math.min(Math.max(targetCenterTop, 0), maxTop);
    const startTop = container.scrollTop;
    const delta = targetTop - startTop;
    if (Math.abs(delta) < 0.5) return;

    const start = performance.now();
    const easeOutCubic = (t: number) => 1 - (1 - t) * (1 - t) * (1 - t);

    const step = (now: number) => {
      const progress = Math.min((now - start) / durationMs, 1);
      container.scrollTop = startTop + delta * easeOutCubic(progress);
      if (progress < 1) {
        scrollAnimRafRef.current = window.requestAnimationFrame(step);
      } else {
        scrollAnimRafRef.current = null;
      }
    };

    scrollAnimRafRef.current = window.requestAnimationFrame(step);
  }, [cardRefs, listContainerRef]);

  const runSplitAnimation = (
    orderedIds: string[],
    splitFn: (selectedCueIds: string[]) => SplitResultItem[],
  ) => {
    const sourceRectByCueId = new Map<string, DOMRect>();
    for (const cueId of orderedIds) {
      const node = cardRefs.current[cueId];
      if (!node) continue;
      sourceRectByCueId.set(cueId, node.getBoundingClientRect());
    }

    const splitResult = splitFn(orderedIds);
    const pending = splitResult
      .map((item) => {
        const fromRect = sourceRectByCueId.get(item.sourceCueId);
        if (!fromRect) return null;
        return { bornCueId: item.bornCueId, fromRect };
      })
      .filter((item): item is { bornCueId: string; fromRect: DOMRect } => item !== null);

    if (pending.length > 0) {
      setIsBatchAnimating(true);
      pendingSplitRef.current = pending;
    }
  };

  useEffect(() => {
    if (!currentMatchCueId) return;
    window.requestAnimationFrame(() => {
      scrollToCueWithFixedDuration(currentMatchCueId);
    });
  }, [currentMatchCueId, scrollToCueWithFixedDuration]);

  useEffect(() => {
    return () => {
      if (batchTimerRef.current != null) {
        window.clearTimeout(batchTimerRef.current);
      }
      if (scrollAnimRafRef.current != null) {
        window.cancelAnimationFrame(scrollAnimRafRef.current);
        scrollAnimRafRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    const pending = pendingSplitRef.current;
    if (pending.length === 0) return;
    pendingSplitRef.current = [];

    let raf1 = 0;
    let raf2 = 0;
    raf1 = window.requestAnimationFrame(() => {
      raf2 = window.requestAnimationFrame(() => {
        let hasAnimation = false;

        for (const item of pending) {
          const node = cardRefs.current[item.bornCueId];
          if (!node) continue;
          const targetRect = node.getBoundingClientRect();
          const dx = item.fromRect.left - targetRect.left;
          const dy = item.fromRect.top - targetRect.top;

          node.animate(
            [
              {
                transform: `translate(${dx}px, ${dy}px) scale(1, 0.62)`,
                opacity: 0.14,
                filter: "saturate(0.86)",
              },
              {
                transform: "translate(0, 3px) scale(1, 1.02)",
                opacity: 0.95,
                filter: "saturate(1.02)",
                offset: 0.8,
              },
              {
                transform: "translate(0, 0) scale(1, 1)",
                opacity: 1,
                filter: "saturate(1)",
              },
            ],
            {
              duration: 520,
              easing: "cubic-bezier(0.22, 1, 0.36, 1)",
              fill: "both",
            },
          );
          hasAnimation = true;
        }

        if (batchTimerRef.current != null) {
          window.clearTimeout(batchTimerRef.current);
        }
        batchTimerRef.current = window.setTimeout(() => {
          setIsBatchAnimating(false);
          batchTimerRef.current = null;
        }, hasAnimation ? 540 : 0);
      });
    });

    return () => {
      window.cancelAnimationFrame(raf1);
      window.cancelAnimationFrame(raf2);
    };
  }, [cues, cardRefs]);

  return {
    isBatchAnimating,
    runSplitAnimation,
  };
}
