export type ToastTone = "info" | "success" | "error";

export type ToastState = {
  id: number;
  message: string;
  tone: ToastTone;
};

export type TermEntry = {
  id: string;
  source: string;
  target: string;
  note: string;
};

export type HotwordGroup = {
  id: string;
  name: string;
  keyterms: string[];
};

export type HotwordCorrection = {
  enabled: boolean;
  activeGroupId: string;
  groups: HotwordGroup[];
};

export type UploadTab = "local" | "youtube";
export type SettingsTab = "transcribe" | "llm" | "hotword" | "advanced";
export type SubtitleSaveState = "idle" | "saving" | "saved" | "error";
