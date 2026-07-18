import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../features/media/types";
import { formatSrtTime } from "../../features/media/srt";

type MatchState = {
  cueId: string;
  cueIndex: number;
  cursor: number;
};

type UseSubtitleFindReplaceArgs = {
  cues: SubtitleCue[];
  onReplaceText: (findText: string, replaceText: string, scopeCueIds: string[] | null, maxReplacements?: number) => number;
};

export function useSubtitleFindReplace({
  cues,
  onReplaceText,
}: UseSubtitleFindReplaceArgs) {
  const { t } = useTranslation(["subtitles"]);
  const [findText, setFindText] = useState("");
  const [replaceText, setReplaceText] = useState("");
  const [findStatus, setFindStatus] = useState("");
  const [findCursor, setFindCursor] = useState(0);
  const [isReplaceMenuOpen, setIsReplaceMenuOpen] = useState(false);
  const replaceMenuRef = useRef<HTMLDivElement | null>(null);

  const findKeyword = useMemo(() => findText.trim().toLowerCase(), [findText]);
  const matchCueIndexes = useMemo(() => {
    if (!findKeyword) return [] as number[];
    const indexes: number[] = [];
    for (let idx = 0; idx < cues.length; idx += 1) {
      const cue = cues[idx];
      if (!cue) continue;
      const seq = String(idx + 1);
      const start = formatSrtTime(cue.startMs);
      const end = formatSrtTime(cue.endMs);
      const range = `${start} --> ${end}`;
      const haystack = [seq, `#${seq}`, start, end, range, cue.text, cue.translatedText]
        .join(" ")
        .toLowerCase();
      if (haystack.includes(findKeyword)) {
        indexes.push(idx);
      }
    }
    return indexes;
  }, [cues, findKeyword]);

  const matchCueIdToCursor = useMemo(() => {
    const map = new Map<string, number>();
    for (let cursor = 0; cursor < matchCueIndexes.length; cursor += 1) {
      const cue = cues[matchCueIndexes[cursor]];
      if (!cue) continue;
      map.set(cue.id, cursor);
    }
    return map;
  }, [cues, matchCueIndexes]);

  const currentMatch: MatchState | null = useMemo(() => {
    if (!findKeyword || matchCueIndexes.length === 0) return null;
    const cursor = Math.min(Math.max(findCursor, 0), matchCueIndexes.length - 1);
    const cueIndex = matchCueIndexes[cursor];
    const cue = cues[cueIndex];
    if (!cue) return null;
    return { cueId: cue.id, cueIndex, cursor };
  }, [cues, findCursor, findKeyword, matchCueIndexes]);

  const findCounterLabel = useMemo(() => {
    if (!findKeyword || matchCueIndexes.length === 0) return "0/0";
    const cursor = currentMatch ? currentMatch.cursor + 1 : 1;
    return `${cursor}/${matchCueIndexes.length}`;
  }, [currentMatch, findKeyword, matchCueIndexes.length]);

  const findStatusLabel = useMemo(() => {
    if (!findKeyword) return findStatus;
    if (matchCueIndexes.length === 0) return findStatus || t("subtitles:findReplace.noMatch");
    return findStatus;
  }, [findKeyword, findStatus, matchCueIndexes.length]);

  const currentMatchCueId = currentMatch?.cueId ?? null;

  const renderHighlightedText = useCallback((text: string, fallback: string, cueId: string): ReactNode => {
    if (!text) return fallback;
    if (!findKeyword) return text;

    const lower = text.toLowerCase();
    const parts: ReactNode[] = [];
    let cursor = 0;
    let partIndex = 0;

    while (cursor < text.length) {
      const index = lower.indexOf(findKeyword, cursor);
      if (index < 0) break;
      if (index > cursor) {
        parts.push(text.slice(cursor, index));
      }
      const match = text.slice(index, index + findKeyword.length);
      parts.push(
        <mark
          key={`${cueId}-${partIndex}`}
          className={`subtitle-inline-hit ${currentMatchCueId === cueId ? "current" : ""}`}
        >
          {match}
        </mark>,
      );
      partIndex += 1;
      cursor = index + findKeyword.length;
    }

    if (parts.length === 0) return text;
    if (cursor < text.length) {
      parts.push(text.slice(cursor));
    }
    return parts;
  }, [currentMatchCueId, findKeyword]);

  const onFindTextChange = (value: string) => {
    setFindText(value);
    setFindStatus("");
  };

  const onToggleReplaceMenu = () => {
    setIsReplaceMenuOpen((old) => !old);
  };

  const onReplaceOne = () => {
    setIsReplaceMenuOpen(false);
    const keyword = findText.trim();
    if (!keyword) {
      setFindStatus(t("subtitles:findReplace.enterFindText"));
      return;
    }
    if (!currentMatch) {
      setFindStatus(t("subtitles:findReplace.noMatchForReplace"));
      return;
    }

    const count = onReplaceText(keyword, replaceText, [currentMatch.cueId], 1);
    if (count <= 0) {
      setFindStatus(t("subtitles:findReplace.nothingReplaced"));
      return;
    }

    setFindStatus(t("subtitles:findReplace.replacedOne"));
    if (matchCueIndexes.length > 0) {
      setFindCursor((old) => {
        const next = old + 1;
        return next >= matchCueIndexes.length ? 0 : next;
      });
    }
  };

  const onReplaceAll = () => {
    const keyword = findText.trim();
    if (!keyword) {
      setFindStatus(t("subtitles:findReplace.enterFindText"));
      setIsReplaceMenuOpen(false);
      return;
    }

    const count = onReplaceText(keyword, replaceText, null);
    if (count > 0) {
      setFindStatus(t("subtitles:findReplace.replacedCount", { count }));
    } else {
      setFindStatus(t("subtitles:findReplace.nothingReplaced"));
    }
    setIsReplaceMenuOpen(false);
  };

  const onPrevMatch = () => {
    if (!findKeyword) {
      setFindStatus(t("subtitles:findReplace.enterFindText"));
      return;
    }
    if (matchCueIndexes.length === 0) {
      setFindStatus(t("subtitles:findReplace.noMatch"));
      return;
    }
    setFindCursor((old) => (old <= 0 ? matchCueIndexes.length - 1 : old - 1));
    setFindStatus("");
  };

  const onNextMatch = () => {
    if (!findKeyword) {
      setFindStatus(t("subtitles:findReplace.enterFindText"));
      return;
    }
    if (matchCueIndexes.length === 0) {
      setFindStatus(t("subtitles:findReplace.noMatch"));
      return;
    }
    setFindCursor((old) => (old + 1 >= matchCueIndexes.length ? 0 : old + 1));
    setFindStatus("");
  };

  const matchCueIdToCursorRef = useRef(matchCueIdToCursor);
  useEffect(() => {
    matchCueIdToCursorRef.current = matchCueIdToCursor;
  }, [matchCueIdToCursor]);

  const moveCursorToCue = useCallback((cueId: string) => {
    const cursor = matchCueIdToCursorRef.current.get(cueId);
    if (cursor != null) {
      setFindCursor(cursor);
    }
  }, []);

  useEffect(() => {
    if (!isReplaceMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (replaceMenuRef.current?.contains(target)) return;
      setIsReplaceMenuOpen(false);
    };
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [isReplaceMenuOpen]);

  return {
    findText,
    replaceText,
    findKeyword,
    findCounterLabel,
    findStatusLabel,
    isReplaceMenuOpen,
    replaceMenuRef,
    currentMatch,
    matchCount: matchCueIndexes.length,
    onFindTextChange,
    onReplaceTextChange: setReplaceText,
    onToggleReplaceMenu,
    onReplaceOne,
    onReplaceAll,
    onPrevMatch,
    onNextMatch,
    moveCursorToCue,
    renderHighlightedText,
  };
}
