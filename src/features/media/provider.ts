import { PROVIDER_IDS, type Provider } from "./types";

export const PROVIDER_OPTIONS: ReadonlyArray<{
  id: Provider;
  label: string;
  title: string;
  kind: "cpu" | "gpu";
}> = [
  { id: "cpu", label: "CPU", title: "CPU", kind: "cpu" },
  { id: "directml", label: "GPU", title: "GPU", kind: "gpu" },
];

export function normalizeProvider(raw: unknown, fallback: Provider = "cpu"): Provider {
  const normalized = String(raw ?? "").trim().toLowerCase();
  if (PROVIDER_IDS.includes(normalized as Provider)) {
    return normalized as Provider;
  }
  return fallback;
}
