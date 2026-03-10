export function toUserErrorMessage(error: unknown, fallback = "操作失败，请稍后重试"): string {
  if (typeof error === "string") {
    return error.trim() || fallback;
  }

  if (error instanceof Error) {
    return error.message.trim() || fallback;
  }

  if (error && typeof error === "object") {
    const maybeMessage = (error as { message?: unknown }).message;
    if (typeof maybeMessage === "string" && maybeMessage.trim()) {
      return maybeMessage.trim();
    }
  }

  return fallback;
}

export function reportError(error: unknown, context: string): void {
  // Keep console logging centralized so we can switch to external telemetry later.
  console.error(`[voxtrans] ${context}`, error);
}
