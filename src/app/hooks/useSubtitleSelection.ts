import { type MouseEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";

type UseSubtitleSelectionArgs = {
  cueIds: string[];
  onSelectedCueChanged?: (cueId: string) => void;
};

export function useSubtitleSelection({
  cueIds,
  onSelectedCueChanged,
}: UseSubtitleSelectionArgs) {
  const [selectedCueIds, setSelectedCueIds] = useState<string[]>([]);
  // The anchor is only read inside click handling, so it lives in a ref to
  // keep the handlers referentially stable for memoized cue rows.
  const anchorCueIdRef = useRef<string>("");
  const cueIdsRef = useRef(cueIds);
  const selectedCueIdsRef = useRef(selectedCueIds);
  const onSelectedCueChangedRef = useRef(onSelectedCueChanged);

  useEffect(() => {
    cueIdsRef.current = cueIds;
  }, [cueIds]);

  useEffect(() => {
    selectedCueIdsRef.current = selectedCueIds;
  }, [selectedCueIds]);

  useEffect(() => {
    onSelectedCueChangedRef.current = onSelectedCueChanged;
  }, [onSelectedCueChanged]);

  const validSelectedCueIds = useMemo(() => {
    return selectedCueIds.filter((id) => cueIds.includes(id));
  }, [cueIds, selectedCueIds]);

  const primarySelectedCueId = validSelectedCueIds[0] ?? null;

  const orderedSelectedCueIds = useMemo(() => {
    return [...validSelectedCueIds].sort((a, b) => cueIds.indexOf(a) - cueIds.indexOf(b));
  }, [cueIds, validSelectedCueIds]);

  const clearSelection = useCallback(() => {
    setSelectedCueIds([]);
    anchorCueIdRef.current = "";
  }, []);

  const selectForEdit = useCallback((cueId: string) => {
    setSelectedCueIds([cueId]);
    anchorCueIdRef.current = cueId;
  }, []);

  const handleCueClick = useCallback((cueId: string, event: MouseEvent<HTMLElement>) => {
    const currentCueIds = cueIdsRef.current;
    const primaryCueId = selectedCueIdsRef.current.find((id) => currentCueIds.includes(id)) ?? null;
    const isToggle = event.ctrlKey || event.metaKey;
    const isRange = event.shiftKey;

    if (isRange) {
      const startId = anchorCueIdRef.current || primaryCueId || cueId;
      const startIndex = currentCueIds.indexOf(startId);
      const endIndex = currentCueIds.indexOf(cueId);
      if (startIndex < 0 || endIndex < 0) {
        setSelectedCueIds([cueId]);
        anchorCueIdRef.current = cueId;
        onSelectedCueChangedRef.current?.(cueId);
        return;
      }
      const [from, to] = startIndex <= endIndex ? [startIndex, endIndex] : [endIndex, startIndex];
      setSelectedCueIds(currentCueIds.slice(from, to + 1));
      onSelectedCueChangedRef.current?.(cueId);
      return;
    }

    if (isToggle) {
      setSelectedCueIds((old) => {
        if (old.includes(cueId)) return old.filter((id) => id !== cueId);
        return [...old, cueId];
      });
      anchorCueIdRef.current = cueId;
      onSelectedCueChangedRef.current?.(cueId);
      return;
    }

    setSelectedCueIds([cueId]);
    anchorCueIdRef.current = cueId;
    onSelectedCueChangedRef.current?.(cueId);
  }, []);

  return {
    selectedCueIds,
    validSelectedCueIds,
    primarySelectedCueId,
    orderedSelectedCueIds,
    clearSelection,
    selectForEdit,
    handleCueClick,
  };
}
