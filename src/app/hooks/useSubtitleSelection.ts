import { type MouseEvent, useMemo, useState } from "react";

type UseSubtitleSelectionArgs = {
  cueIds: string[];
  onSelectedCueChanged?: (cueId: string) => void;
};

export function useSubtitleSelection({
  cueIds,
  onSelectedCueChanged,
}: UseSubtitleSelectionArgs) {
  const [selectedCueIds, setSelectedCueIds] = useState<string[]>([]);
  const [anchorCueId, setAnchorCueId] = useState<string>("");

  const validSelectedCueIds = useMemo(() => {
    return selectedCueIds.filter((id) => cueIds.includes(id));
  }, [cueIds, selectedCueIds]);

  const primarySelectedCueId = validSelectedCueIds[0] ?? null;

  const orderedSelectedCueIds = useMemo(() => {
    return [...validSelectedCueIds].sort((a, b) => cueIds.indexOf(a) - cueIds.indexOf(b));
  }, [cueIds, validSelectedCueIds]);

  const clearSelection = () => {
    setSelectedCueIds([]);
    setAnchorCueId("");
  };

  const selectForEdit = (cueId: string) => {
    setSelectedCueIds([cueId]);
    setAnchorCueId(cueId);
  };

  const handleCueClick = (cueId: string, event: MouseEvent<HTMLElement>) => {
    const isToggle = event.ctrlKey || event.metaKey;
    const isRange = event.shiftKey;

    if (isRange) {
      const startId = anchorCueId || primarySelectedCueId || cueId;
      const startIndex = cueIds.indexOf(startId);
      const endIndex = cueIds.indexOf(cueId);
      if (startIndex < 0 || endIndex < 0) {
        setSelectedCueIds([cueId]);
        setAnchorCueId(cueId);
        onSelectedCueChanged?.(cueId);
        return;
      }
      const [from, to] = startIndex <= endIndex ? [startIndex, endIndex] : [endIndex, startIndex];
      setSelectedCueIds(cueIds.slice(from, to + 1));
      onSelectedCueChanged?.(cueId);
      return;
    }

    if (isToggle) {
      setSelectedCueIds((old) => {
        if (old.includes(cueId)) return old.filter((id) => id !== cueId);
        return [...old, cueId];
      });
      setAnchorCueId(cueId);
      onSelectedCueChanged?.(cueId);
      return;
    }

    setSelectedCueIds([cueId]);
    setAnchorCueId(cueId);
    onSelectedCueChanged?.(cueId);
  };

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
