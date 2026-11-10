import { useCallback, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertIcon, ChevronDownIcon, SparkleIcon } from "../Icons";
import type {
  Diagnostic,
  DiagnosticKind,
  DiagnosticSummary,
} from "../../../features/media/subtitleDiagnostics";
import { describeDiagnostic, kindLabel } from "./diagnosticLabels";

type SubtitleQualityBarProps = {
  diagnostics: Diagnostic[];
  summary: DiagnosticSummary;
  canEdit: boolean;
  applying: boolean;
  /** Active chip filter, `null` = show every cue. */
  activeKind: DiagnosticKind | null;
  onActiveKindChange: (kind: DiagnosticKind | null) => void;
  onLocate: (cueId: string) => void;
  onApply: () => void;
};


/**
 * Aggregate quality strip for the subtitle editor.
 *
 * Only renders when something is wrong: a clean list must stay quiet, so the
 * green state lives in the css of the "all clear" outcome rather than as a
 * badge on every row. Per-row marking happens in `SubtitleCueList` and is
 * exception-only for the same reason.
 */
export default function SubtitleQualityBar({
  diagnostics,
  summary,
  canEdit,
  applying,
  activeKind,
  onActiveKindChange,
  onLocate,
  onApply,
}: SubtitleQualityBarProps) {
  const { t } = useTranslation(["subtitles", "common"]);
  const [open, setOpen] = useState(false);
  const panelId = useId();
  const summaryRef = useRef<HTMLButtonElement | null>(null);

  const tone = summary.errors > 0 ? "error" : "warning";

  const summaryLabel =
    summary.errors > 0 && summary.warnings > 0
      ? t("subtitles:quality.summary.mixed", {
          errors: summary.errors,
          warnings: summary.warnings,
        })
      : summary.errors > 0
        ? t("subtitles:quality.summary.needsFix", { errors: summary.errors })
        : t("subtitles:quality.summary.canImprove", { warnings: summary.warnings });

  const visible = activeKind
    ? diagnostics.filter((diagnostic) => diagnostic.kind === activeKind)
    : diagnostics;

  const handleChip = useCallback(
    (kind: DiagnosticKind) => {
      onActiveKindChange(activeKind === kind ? null : kind);
    },
    [activeKind, onActiveKindChange],
  );

  const handleLocate = useCallback(
    (cueId: string) => {
      onLocate(cueId);
      setOpen(false);
      summaryRef.current?.focus();
    },
    [onLocate],
  );

  return (
    <section className="subtitle-quality-bar" aria-label={t("subtitles:quality.regionLabel")}>
      <div className={`subtitle-quality-strip ${tone}`}>
        <button
          ref={summaryRef}
          type="button"
          className="subtitle-quality-summary"
          aria-expanded={open}
          aria-controls={panelId}
          onClick={() => setOpen((previous) => !previous)}
        >
          <span className="subtitle-quality-icon" aria-hidden="true">
            <AlertIcon />
          </span>
          <span className="subtitle-quality-label">{summaryLabel}</span>
          <ChevronDownIcon />
        </button>

        <div className="subtitle-quality-chips" role="group" aria-label={t("subtitles:quality.filtersAria")}>
          {summary.byKind.map(({ kind, count }) => {
            const isActive = activeKind === kind;
            return (
              <button
                key={kind}
                type="button"
                className={`subtitle-quality-chip${isActive ? " is-active" : ""}`}
                aria-pressed={isActive}
                onClick={() => handleChip(kind)}
              >
                <span>{kindLabel(t, kind)}</span>
                <span className="subtitle-quality-chip-count">{count}</span>
              </button>
            );
          })}
        </div>

        <div className="subtitle-quality-actions">
          {summary.manual > 0 ? (
            <span
              className="subtitle-quality-manual"
              title={t("subtitles:quality.manualHint")}
            >
              {t("subtitles:quality.manualCount", { count: summary.manual })}
            </span>
          ) : null}
          <button
            type="button"
            className="apple-button apple-button-secondary subtitle-quality-apply"
            disabled={!canEdit || applying || summary.autoFixable === 0}
            title={
              !canEdit
                ? t("subtitles:quality.applyReadOnly")
                : summary.autoFixable === 0
                  ? t("subtitles:quality.applyNothing")
                  : t("subtitles:quality.applyHint")
            }
            onClick={onApply}
          >
            <SparkleIcon />
            <span>
              {applying
                ? t("subtitles:quality.applying")
                : t("subtitles:quality.apply", { count: summary.autoFixable })}
            </span>
          </button>
        </div>
      </div>

      <div
        id={panelId}
        className="subtitle-quality-panel"
        data-open={open ? "true" : "false"}
        aria-hidden={!open}
        role="group"
        aria-label={t("subtitles:quality.listLabel")}
      >
        <div className="subtitle-quality-scroll">
          <ul className="subtitle-quality-list">
            {visible.map((diagnostic) => (
              <li key={`${diagnostic.cueId}-${diagnostic.kind}`}>
                <button
                  type="button"
                  className={`subtitle-quality-item ${diagnostic.severity}`}
                  tabIndex={open ? 0 : -1}
                  onClick={() => handleLocate(diagnostic.cueId)}
                >
                  <span className="subtitle-quality-item-number">#{diagnostic.cueNumber}</span>
                  <span className="subtitle-quality-item-kind">
                    {kindLabel(t, diagnostic.kind)}
                  </span>
                  <span className="subtitle-quality-item-detail">
                    {describeDiagnostic(t, diagnostic)}
                  </span>
                  <span className="subtitle-quality-item-locate">
                    {t("subtitles:quality.locate")}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </section>
  );
}
