const ERROR_PATTERNS: Array<{ pattern: RegExp; message: string }> = [
  { pattern: /task not found/i, message: "任务不存在，请刷新任务列表" },
  { pattern: /task is processing or queued/i, message: "任务正在处理中，请稍后再试" },
  { pattern: /workspace store lock poisoned/i, message: "内部状态错误，请重启应用" },
  { pattern: /invalid request/i, message: "请求参数无效" },
  { pattern: /serialization error/i, message: "数据解析失败" },
  { pattern: /io error/i, message: "文件读写失败，请检查磁盘空间" },
  { pattern: /failed to create http client/i, message: "网络配置错误，请检查 API 设置" },
  { pattern: /llm call failed/i, message: "AI 调用失败，请检查网络连接和 API 配置" },
  { pattern: /timeout|timed out/i, message: "请求超时，请检查网络连接" },
  { pattern: /cancelled|取消|已取消/i, message: "操作已取消" },
  { pattern: /api key is required|translateApiKey/i, message: "翻译 API Key 未设置，请在设置中配置" },
  { pattern: /base url is required|translateBaseUrl/i, message: "翻译 API 地址未设置，请在设置中配置" },
  { pattern: /model is required|translateModel/i, message: "翻译模型未设置，请在设置中配置" },
];

const ERROR_CODE_MESSAGES: Record<string, string> = {
  TASK_NOT_FOUND: "任务不存在，请刷新任务列表",
  TASK_BUSY: "任务正在处理中，请稍后再试",
  WORKSPACE_LOCK_POISONED: "内部状态错误，请重启应用",
  INVALID_REQUEST: "请求参数无效",
  TASK_FAILED: "任务执行失败，请查看日志",
  IO_ERROR: "文件读写失败，请检查磁盘空间",
  SERIALIZATION_ERROR: "数据解析失败",
};

function classifyError(raw: string): string | null {
  const lower = raw.toLowerCase();
  for (const { pattern, message } of ERROR_PATTERNS) {
    if (pattern.test(lower)) {
      return message;
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
  fallback = "操作失败，请稍后重试",
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
    if (classified) return classified;
  }
  if (structured?.code) {
    const coded = ERROR_CODE_MESSAGES[structured.code];
    if (coded) return coded;
  }
  if (structured?.message) {
    raw = structured.message;
  }

  if (!raw) {
    return fallback;
  }

  const classified = classifyError(raw);
  return classified ?? raw;
}

export function reportError(error: unknown, context: string): void {
  // Keep console logging centralized so we can switch to external telemetry later.
  console.error(`[voxtrans] ${context}`, error);
}
