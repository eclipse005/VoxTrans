import { PROVIDER_IDS, type Provider } from "./types";

export const PROVIDER_OPTIONS: ReadonlyArray<{
  id: Provider;
  label: string;
  title: string;
  kind: "cpu" | "gpu";
}> = [
  { id: "cpu", label: "CPU", title: "CPU", kind: "cpu" },
  { id: "cuda", label: "CUDA", title: "NVIDIA CUDA", kind: "gpu" },
];

export function normalizeProvider(raw: unknown, fallback: Provider = "cpu"): Provider {
  const normalized = String(raw ?? "").trim().toLowerCase();
  if (normalized === "directml") {
    return "cuda";
  }
  if (PROVIDER_IDS.includes(normalized as Provider)) {
    return normalized as Provider;
  }
  return fallback;
}
