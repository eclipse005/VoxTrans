import type { CSSProperties } from "react";
import type {
  SubtitleBurnMode,
  SubtitleLineStyle,
  SubtitleRenderStyle,
} from "../../../features/media/types";

const SUBTITLE_PREVIEW_BG = "/subtitle-preview-bg.svg";

type SubtitleStylePreviewProps = {
  mode: SubtitleBurnMode;
  style: SubtitleRenderStyle;
};

type PreviewRow = {
  kind: "source" | "target";
  text: string;
};

export function SubtitleStylePreview({ mode, style }: SubtitleStylePreviewProps) {
  const previewRows = buildPreviewRows(mode);
  const previewStyle = buildSubtitlePreviewStyle(style);
  const previewClass = style.layout.alignment === 1
    ? "subtitle-preview-text is-left"
    : style.layout.alignment === 3
      ? "subtitle-preview-text is-right"
      : "subtitle-preview-text is-center";

  return (
    <div className="subtitle-style-preview-card">
      <div className="subtitle-style-preview-head">实时预览</div>
      <div className="subtitle-style-preview-stage">
        <img className="subtitle-preview-bg" src={SUBTITLE_PREVIEW_BG} alt="字幕样式预览背景" />
        <div className={previewClass} style={previewStyle.wrapper}>
          {previewRows.map((row, idx) => (
            <div key={`${row.text}-${idx}`} style={row.kind === "source" ? previewStyle.source : previewStyle.target}>
              {row.text}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function buildPreviewRows(mode: SubtitleBurnMode): PreviewRow[] {
  const source = "The morning rain has settled down.";
  const target = "清晨的雨已经停了。";
  if (mode === "source") {
    return [{ kind: "source", text: source }];
  }
  if (mode === "target") {
    return [{ kind: "target", text: target }];
  }
  if (mode === "bilingualTargetFirst") {
    return [
      { kind: "target", text: target },
      { kind: "source", text: source },
    ];
  }
  return [
    { kind: "source", text: source },
    { kind: "target", text: target },
  ];
}

function buildSubtitlePreviewStyle(style: SubtitleRenderStyle): {
  wrapper: CSSProperties;
  source: CSSProperties;
  target: CSSProperties;
} {
  const source = toPreviewLineStyle(style.source);
  const target = toPreviewLineStyle(style.target);
  return {
    wrapper: {
      bottom: `${style.layout.marginV}px`,
      gap: `${style.layout.bilingualLineGap}px`,
    },
    source,
    target,
  };
}

// Clamp ranges mirror the backend ASS writer (subtitle_render.rs:215-217) so
// the preview never shows values the burn-in would silently truncate.
const FONT_SIZE_MIN = 16;
const FONT_SIZE_MAX = 96;
const OUTLINE_MAX = 8;
const SHADOW_MAX = 8;

function toPreviewLineStyle(style: SubtitleLineStyle): CSSProperties {
  // ASS encodes Bold as 0 (off); the burn always renders non-bold, so the
  // preview must too — otherwise users see bold text that bakes out thin.
  const outline = Math.max(0, Math.min(OUTLINE_MAX, style.outline));
  const shadow = Math.max(0, Math.min(SHADOW_MAX, style.shadow));
  const borderOpacity = Math.max(0, Math.min(100, style.borderOpacity)) / 100;
  const outlineColor = hexToRgba(style.outlineColor, borderOpacity);
  const backColor = hexToRgba(style.backColor, borderOpacity);
  const textShadows = [
    `${outline}px 0 0 ${outlineColor}`,
    `${-outline}px 0 0 ${outlineColor}`,
    `0 ${outline}px 0 ${outlineColor}`,
    `0 ${-outline}px 0 ${outlineColor}`,
    `${shadow}px ${shadow}px 2px ${backColor}`,
  ];
  const boxStyle = style.borderStyle === "box"
    ? {
      backgroundColor: outlineColor,
      border: `${Math.max(1, outline)}px solid ${outlineColor}`,
      borderRadius: "6px",
      padding: "2px 10px",
    }
    : undefined;
  return {
    fontFamily: style.fontFamily,
    fontSize: `${Math.max(FONT_SIZE_MIN, Math.min(FONT_SIZE_MAX, style.fontSize))}px`,
    color: style.primaryColor,
    textShadow: style.borderStyle === "box" ? `${shadow}px ${shadow}px 2px ${backColor}` : textShadows.join(", "),
    lineHeight: 1.2,
    fontWeight: 400,
    display: "inline-block",
    ...boxStyle,
  };
}

function hexToRgba(raw: string, alpha: number): string {
  const value = String(raw ?? "").trim();
  if (!/^#[0-9a-fA-F]{6}$/.test(value)) {
    return `rgba(0, 0, 0, ${alpha})`;
  }
  const r = Number.parseInt(value.slice(1, 3), 16);
  const g = Number.parseInt(value.slice(3, 5), 16);
  const b = Number.parseInt(value.slice(5, 7), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
