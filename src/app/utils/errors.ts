import i18n from "../../i18n";

/**
 * Map a raw backend error to a localized user-facing message.
 *
 * The backend returns either a plain English string or a structured
 * `{ code, message }` JSON payload (see `domain::error` / `app_error`).
 * We resolve in priority order:
 *   1. stable error code  → `errors:code.<CODE>`
 *   2. message pattern    → `errors:<area>.<key>`
 *   3. raw message        → shown as-is (already English)
 *
 * The fallback covers null/empty inputs. All outputs go through i18n so
 * the active locale (zh-CN / en) is honored.
 */

const ERROR_PATTERNS: Array<{ pattern: RegExp; key: string }> = [
  { pattern: /task not found/i, key: "errors:code.taskNotFound" },
  { pattern: /task is processing or queued/i, key: "errors:code.taskBusy" },
  { pattern: /workspace store lock poisoned/i, key: "errors:code.lockPoisoned" },
  { pattern: /invalid request/i, key: "errors:code.invalidRequest" },
  { pattern: /serialization error/i, key: "errors:code.serializationError" },
  { pattern: /io error/i, key: "errors:code.ioError" },
  { pattern: /failed to create http client/i, key: "errors:config.httpClient" },
  { pattern: /llm call failed/i, key: "errors:config.llmFailed" },
  { pattern: /timeout|timed out/i, key: "errors:config.timeout" },
  { pattern: /cancelled|取消|已取消/i, key: "errors:cancelled" },
  { pattern: /api key is required|translateApiKey/i, key: "errors:config.apiKeyRequired" },
  { pattern: /base url is required|translateBaseUrl/i, key: "errors:config.baseUrlRequired" },
  { pattern: /model is required|translateModel/i, key: "errors:config.modelRequired" },
];

const ERROR_CODE_MESSAGES: Record<string, string> = {
  TASK_NOT_FOUND: "errors:code.taskNotFound",
  TASK_BUSY: "errors:code.taskBusy",
  WORKSPACE_LOCK_POISONED: "errors:code.lockPoisoned",
  INVALID_REQUEST: "errors:code.invalidRequest",
  TASK_FAILED: "errors:code.taskFailed",
  IO_ERROR: "errors:code.ioError",
  SERIALIZATION_ERROR: "errors:code.serializationError",
};

function classifyError(raw: string): string | null {
  const lower = raw.toLowerCase();
  for (const { pattern, key } of ERROR_PATTERNS) {
    if (pattern.test(lower)) {
      return key;
    }
  }
  return null;
}

function parseStructuredError(raw: string): { code?: string; message?: string } | null {
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return null;
    const code = (parsed as { code?: unknown }).code;
    const message = (parsed as { message?: unknown }).message;
    return {
      code: typeof code === "string" ? code.trim() : undefined,
      message: typeof message === "string" ? message.trim() : undefined,
    };
  } catch {
    return null;
  }
}

export function toUserErrorMessage(
  error: unknown,
  fallback = "errors:fallback",
): string {
  let raw = "";
  let structured: { code?: string; message?: string } | null = null;

  if (typeof error === "string") {
    raw = error;
  } else if (error instanceof Error) {
    raw = error.message;
  } else if (error && typeof error === "object") {
    const maybeCode = (error as { code?: unknown }).code;
    const maybeMessage = (error as { message?: unknown }).message;
    if (typeof maybeCode === "string") {
      structured = {
        code: maybeCode.trim(),
        message: typeof maybeMessage === "string" ? maybeMessage.trim() : undefined,
      };
    }
    if (typeof maybeMessage === "string") {
      raw = maybeMessage;
    }
  }

  raw = raw.trim();
  structured ??= parseStructuredError(raw);
  if (structured?.code === "TASK_FAILED" && structured.message) {
    const classified = classifyError(structured.message);
    if (classified) return i18n.t(classified);
  }
  if (structured?.code) {
    const key = ERROR_CODE_MESSAGES[structured.code];
    if (key) return i18n.t(key);
  }
  if (structured?.message) {
    raw = structured.message;
  }

  if (!raw) {
    return i18n.t(fallback);
  }

  const classified = classifyError(raw);
  return classified ? i18n.t(classified) : raw;
}

export function reportError(error: unknown, context: string): void {
  // Keep console logging centralized so we can switch to external telemetry later.
  console.error(`[voxtrans] ${context}`, error);
}
