/**
 * @deprecated Prefer {@link ModelCenter} + {@link ModelActions}.
 * Kept as a thin wrapper so older call sites / tests do not break mid-refactor.
 */
import type { ModelStatusResponse, ModelTarget } from "../../../features/media/types";
import { ModelActions } from "./ModelActions";

type ModelDownloadCardProps = {
  target: ModelTarget;
  title: string;
  description: string;
  modelName: string;
  selected: boolean;
  status: ModelStatusResponse | null;
  onSelect: () => void;
  onOpenModelDir: (target: ModelTarget, model?: string) => void | Promise<void>;
  onStartModelDownload: (target: ModelTarget, model?: string) => void | Promise<void>;
  onCancelModelDownload: (target: ModelTarget, model?: string) => void | Promise<void>;
};

export function ModelDownloadCard({
  target,
  title,
  description,
  modelName,
  selected,
  status,
  onSelect,
  onOpenModelDir,
  onStartModelDownload,
  onCancelModelDownload,
}: ModelDownloadCardProps) {
  return (
    <div className={`model-task-card ${selected ? "is-selected" : ""}`}>
      <div className="model-task-card-head">
        <h4 className="apple-heading-small">{title}</h4>
      </div>
      <p className="apple-body-small">{description}</p>
      <div className="model-inline-row">
        <button type="button" className={`device-toggle-btn ${selected ? "active" : ""}`} onClick={onSelect}>
          {modelName}
        </button>
        <ModelActions
          target={target}
          modelName={modelName}
          status={status}
          onOpenModelDir={onOpenModelDir}
          onStartModelDownload={onStartModelDownload}
          onCancelModelDownload={onCancelModelDownload}
        />
      </div>
    </div>
  );
}
