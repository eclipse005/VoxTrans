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

export type UploadTab = "local" | "youtube";
export type SettingsTab = "basic" | "transcribe" | "advanced";
