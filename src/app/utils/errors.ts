import { invoke } from "@tauri-apps/api/core";
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

/**
 * Reporting is best-effort and must never change control flow: never await
 * IPC, never surface a second error while reporting the first, never touch
 * React state. The text is capped here (the backend caps again) so a multi-MB
 * stack cannot cross the IPC boundary.
 */
const REPORT_MESSAGE_MAX_CHARS = 2000;
const REPORT_CONTEXT_MAX_CHARS = 200;
const TRUNCATION_MARKER = "…";

function truncateForReport(value: string, maxChars: number): string {
  // Fast path: UTF-16 units are never fewer than code points, so this cannot
  // under-count. It avoids materializing an array for the common case — and in
  // a crash path a multi-MB stack is exactly what arrives here.
  if (value.length <= maxChars) return value;
  const characters = Array.from(value);
  if (characters.length <= maxChars) return value;
  return `${characters.slice(0, Math.max(0, maxChars - 1)).join("")}${TRUNCATION_MARKER}`;
}

/**
 * Flatten an unknown thrown value into a bounded string. `Error` keeps its
 * name/message/stack, strings pass through, everything else falls back to
 * JSON. `JSON.stringify` can throw (bigint, cycles, throwing `toJSON`) and
 * `String()` can throw on hostile objects, so both paths are guarded.
 */
function stringifyErrorForReport(error: unknown): string {
  if (typeof error === "string") {
    return truncateForReport(error, REPORT_MESSAGE_MAX_CHARS);
  }
  if (error instanceof Error) {
    const parts = [error.name, error.message, error.stack].filter(
      (part): part is string => typeof part === "string" && part.length > 0,
    );
    return truncateForReport(parts.join("\n"), REPORT_MESSAGE_MAX_CHARS);
  }

  let text: string;
  try {
    const json = JSON.stringify(error);
    text = json ?? String(error);
  } catch {
    try {
      text = String(error);
    } catch {
      text = "unserializable error";
    }
  }
  return truncateForReport(text, REPORT_MESSAGE_MAX_CHARS);
}

export function reportError(error: unknown, context: string): void {
  // Keep console logging centralized so we can switch to external telemetry later.
  console.error(`[voxtrans] ${context}`, error);

  // Fire-and-forget into `<output root>/logs/app.log`. In non-Tauri hosts
  // (tests, plain browser) `invoke` rejects and the catch drops it.
  void invoke("report_frontend_error", {
    context: truncateForReport(context, REPORT_CONTEXT_MAX_CHARS),
    message: stringifyErrorForReport(error),
  }).catch(() => {});
}
