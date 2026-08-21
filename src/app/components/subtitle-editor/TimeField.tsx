import {
  useCallback,
  useRef,
  useState,
  type ChangeEvent,
  type FocusEvent,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import { formatSrtTime, parseSrtTime } from "../../../features/media/srt";

type TimeFieldProps = {
  valueMs: number;
  label: string;
  canEdit: boolean;
  invalid: boolean;
  describedBy?: string;
  onCommit: (ms: number) => void;
  onReject: () => void;
  onCancel: () => void;
  onFocusCue: () => void;
};

/**
 * String parsing lives here; the parent only receives milliseconds.
 * Session ends on successful commit or Escape — not on blur-after-reject.
 */
export function TimeField({
  valueMs,
  label,
  canEdit,
  invalid,
  describedBy,
  onCommit,
  onReject,
  onCancel,
  onFocusCue,
}: TimeFieldProps) {
  const committed = formatSrtTime(valueMs);
  const [sessionDraft, setSessionDraft] = useState<string | null>(null);
  const sessionDraftRef = useRef<string | null>(null);
  const display = sessionDraft ?? committed;

  const endSession = useCallback(() => {
    sessionDraftRef.current = null;
    setSessionDraft(null);
  }, []);

  const tryCommit = useCallback((): boolean => {
    const pending = sessionDraftRef.current;
    if (pending === null) return true;
    if (!canEdit) {
      endSession();
      return true;
    }
    const parsed = parseSrtTime(pending);
    if (parsed == null) {
      onReject();
      return false;
    }
    endSession();
    onCommit(parsed);
    return true;
  }, [canEdit, endSession, onCommit, onReject]);

  const handleFocus = useCallback(
    (event: FocusEvent<HTMLInputElement>) => {
      onFocusCue();
      if (canEdit) {
        event.currentTarget.select();
      }
    },
    [canEdit, onFocusCue],
  );

  const handleChange = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    const next = event.currentTarget.value;
    sessionDraftRef.current = next;
    setSessionDraft(next);
  }, []);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "Enter") {
        event.preventDefault();
        if (tryCommit()) event.currentTarget.blur();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        // Null the ref before blur so tryCommit sees an idle field.
        endSession();
        onCancel();
        event.currentTarget.blur();
      }
    },
    [endSession, onCancel, tryCommit],
  );

  const handleClick = useCallback((event: MouseEvent<HTMLElement>) => {
    event.stopPropagation();
  }, []);

  return (
    <input
      type="text"
      className="subtitle-time-input"
      value={display}
      aria-label={label}
      title={label}
      aria-invalid={invalid || undefined}
      aria-describedby={describedBy}
      spellCheck={false}
      readOnly={!canEdit}
      onClick={handleClick}
      onFocus={handleFocus}
      onChange={handleChange}
      onBlur={() => {
        tryCommit();
      }}
      onKeyDown={handleKeyDown}
    />
  );
}
