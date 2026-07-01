import type { Provider } from "./types";

export const PROVIDER_OPTIONS: ReadonlyArray<{
  id: Provider;
  label: string;
  title: string;
  kind: "cpu" | "gpu";
}> = [
  { id: "cpu", label: "CPU", title: "CPU", kind: "cpu" },
  { id: "cuda", label: "CUDA", title: "NVIDIA CUDA", kind: "gpu" },
];
