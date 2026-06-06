export type ToastTone = "info" | "success" | "error";

export type ToastState = {
  id: number;
  message: string;
  tone: ToastTone;
};

export type UploadTab = "local" | "youtube";
