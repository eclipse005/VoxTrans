import { invoke } from "@tauri-apps/api/core";
import type { SubtitleSegment, WordToken } from "../../features/media/types";
import type { HotwordCorrection } from "../types";

type HotwordTerm = {
  name: string;
  meaning: string | null;
};

type LlmToolFunction = {
  name: string;
  description?: string;
  parameters: Record<string, unknown>;
};

type LlmTool = {
  type: "function";
  function: LlmToolFunction;
};

type LlmToolCall = {
  id: string;
  type: string;
  function: {
    name: string;
    arguments: string;
  };
};

type LlmToolResult = {
  toolCallId: string;
  content: string;
};

type LlmMessageInput = {
  role: string;
  content?: string;
  toolCalls?: LlmToolCall[];
};

type LlmInteractRequest = {
  apiKey: string;
  model: string;
  baseUrl?: string | null;
  systemPrompt?: string;
  prompt?: string;
  messages?: LlmMessageInput[];
  mode?: "chat" | "tool";
  tools?: LlmTool[];
  toolResults?: LlmToolResult[];
  toolChoice?: "auto";
  timeoutSecs?: number;
};

type LlmInteractResponse = {
  status: "completed" | "requires_tool";
  message?: string;
  toolCalls: LlmToolCall[];
};

type HotwordLlmConfig = {
  apiKey: string;
  apiBase: string;
  apiModel: string;
};

export type TimedHotwordSegment = SubtitleSegment & {
  words: WordToken[];
};

type HotwordCorrectionArgs = {
  segments: TimedHotwordSegment[];
  config: HotwordCorrection;
  llm: HotwordLlmConfig;
  invokeLlm: (request: LlmInteractRequest) => Promise<LlmInteractResponse>;
};

export type HotwordCorrectionResult = {
  segments: TimedHotwordSegment[];
  words: WordToken[];
  changedCount: number;
  summary: string;
  replacementStats: Array<{
    oldText: string;
    newText: string;
    count: number;
  }>;
};

type BuildHotwordCorrectionPromptsResponse = {
  systemPrompt: string;
  initialTask: string;
  tools: LlmTool[];
};

type CorrectionRecord = {
  segmentIdx: number;
  startIdx: number;
  endIdx: number;
  oldText: string;
  newText: string;
};

const DEFAULT_WINDOW_SIZE = 80;
const FIRST_PASS_MAX_AGENT_ROUNDS = 20;
const SECOND_PASS_MAX_AGENT_ROUNDS = 10;
const NO_TOOL_RETRY = 2;
const ACTION_MAX_ROUNDS = 12;
const NO_IMPROVE_PATIENCE = 2;
const FOCUS_RESCAN_PADDING = 10;

type AgentRunResult = {
  status: "finished" | "stopped" | "max_rounds";
  summary: string;
  stopReason: string;
};

type AgentRuntimeState = {
  actionRound: number;
  noImproveStreak: number;
  changes: CorrectionRecord[];
  changedIndexes: Set<number>;
  finishedSummary: string;
};

export function shouldRunHotwordCorrection(config: HotwordCorrection): boolean {
  if (!config.enabled) return false;
  const active = config.groups.find((group) => group.id === config.activeGroupId);
  return Boolean(active && active.keyterms.some((term) => term.trim().length > 0));
}

export async function correctSegmentsWithHotwords(args: HotwordCorrectionArgs): Promise<HotwordCorrectionResult> {
  const { segments, config, llm, invokeLlm } = args;
  const active = config.groups.find((group) => group.id === config.activeGroupId) ?? config.groups[0];
  const terms = parseTerms(active?.keyterms ?? []);
  if (!terms.length) {
    return {
      segments,
      words: flattenWords(segments),
      changedCount: 0,
      summary: "active group has no terms",
      replacementStats: [],
    };
  }

  const working = segments.map((segment) => ({
    ...segment,
    words: segment.words.map((word) => ({ ...word })),
  }));
  const originalTexts = working.map((segment) => segment.sourceText);
  const state: AgentRuntimeState = {
    actionRound: 0,
    noImproveStreak: 0,
    changes: [],
    changedIndexes: new Set<number>(),
    finishedSummary: "",
  };

  const countMatchesForText = (target: string): number => {
    if (!target) return 0;
    let total = 0;
    for (const segment of working) {
      total += findSurfaceMatches(segment.sourceText, target).length;
    }
    return total;
  };

  const executeToolCall = (call: LlmToolCall): { raw: string; parsed: Record<string, unknown> | null } => {
    const rawArgs = call.function.arguments || "{}";
    let argsObj: Record<string, unknown> = {};
    try {
      argsObj = JSON.parse(rawArgs) as Record<string, unknown>;
    } catch {
      const response = { status: "error", message: "arguments must be valid JSON" };
      return { raw: JSON.stringify(response), parsed: response };
    }

    if (call.function.name === "read_sentences") {
      const startRaw = typeof argsObj.start_idx === "number" ? argsObj.start_idx : 0;
      const endRaw = typeof argsObj.end_idx === "number" ? argsObj.end_idx : startRaw + DEFAULT_WINDOW_SIZE;
      const total = working.length;
      const start = Math.max(0, Math.min(total, Math.floor(startRaw)));
      const end = Math.max(start, Math.min(total, Math.floor(endRaw)));
      const lines = working.slice(start, end).map((segment, idx) => {
        const absolute = start + idx;
        return `第${absolute + 1}句 [${(segment.startMs / 1000).toFixed(1)}s-${(segment.endMs / 1000).toFixed(1)}s]: ${segment.sourceText}`;
      });
      const response = {
        status: "ok",
        start_idx: start,
        end_idx: end,
        total,
        message: lines.join("\n"),
      };
      return { raw: JSON.stringify(response), parsed: response };
    }

    if (call.function.name === "batch_replace") {
      if (state.actionRound >= ACTION_MAX_ROUNDS) {
        const stopped = {
          status: "stopped",
          indexes: [],
          changes_count: 0,
          errors: [],
          message: `达到最大修改轮次 ${ACTION_MAX_ROUNDS}，停止继续替换`,
          stop_reason: "max_rounds",
          round: state.actionRound,
        };
        return { raw: JSON.stringify(stopped), parsed: stopped };
      }
      state.actionRound += 1;

      const replacements = Array.isArray(argsObj.replacements) ? argsObj.replacements : [];
      let totalChanges = 0;
      const replacementTerms = new Set<string>();
      for (const replacement of replacements) {
        if (!replacement || typeof replacement !== "object") continue;
        const oldText = String((replacement as { old_text?: unknown }).old_text ?? "").trim();
        const newText = String((replacement as { new_text?: unknown }).new_text ?? "").trim();
        if (!oldText || !newText || oldText === newText) continue;
        replacementTerms.add(oldText);
      }
      const beforeMetric = Array.from(replacementTerms.values()).reduce((acc, term) => acc + countMatchesForText(term), 0);

      for (const replacement of replacements) {
        if (!replacement || typeof replacement !== "object") continue;
        const oldText = String((replacement as { old_text?: unknown }).old_text ?? "").trim();
        const newText = String((replacement as { new_text?: unknown }).new_text ?? "").trim();
        if (!oldText || !newText || oldText === newText) continue;

        for (let idx = 0; idx < working.length; idx += 1) {
          const segment = working[idx];
          const result = replaceInText(segment.sourceText, oldText, newText);
          if (result.matches.length <= 0) continue;
          segment.sourceText = result.text;
          totalChanges += result.matches.length;
          state.changedIndexes.add(idx);

          for (const match of result.matches) {
            state.changes.push({
              segmentIdx: idx,
              startIdx: match.start,
              endIdx: match.end,
              oldText: match.matchedText,
              newText,
            });
          }
        }
      }

      const afterMetric = Array.from(replacementTerms.values()).reduce((acc, term) => acc + countMatchesForText(term), 0);

      let status: "ok" | "stopped" = "ok";
      let stopReason = "";
      if (afterMetric < beforeMetric) {
        state.noImproveStreak = 0;
      } else {
        state.noImproveStreak += 1;
        if (state.noImproveStreak >= NO_IMPROVE_PATIENCE) {
          status = "stopped";
          stopReason = "no_improvement";
        }
      }

      const response = {
        status,
        changes_count: totalChanges,
        indexes: Array.from(state.changedIndexes.values()).sort((a, b) => a - b),
        metrics: {
          before: beforeMetric,
          after: afterMetric,
          delta: afterMetric - beforeMetric,
          no_improve_streak: state.noImproveStreak,
          round: state.actionRound,
        },
        stop_reason: stopReason,
      };
      return { raw: JSON.stringify(response), parsed: response };
    }

    if (call.function.name === "finish") {
      state.finishedSummary = String(argsObj.summary ?? "").trim();
      const response = {
        status: "finished",
        summary: state.finishedSummary,
      };
      return { raw: JSON.stringify(response), parsed: response };
    }

    const response = { status: "error", message: `unknown tool: ${call.function.name}` };
    return { raw: JSON.stringify(response), parsed: response };
  };

  const promptBundle = await invoke<BuildHotwordCorrectionPromptsResponse>("build_hotword_correction_prompts", {
    request: { terms, total: working.length },
  });
  const runAgentSession = async (prompt: string, maxRounds: number): Promise<AgentRunResult> => {
    const { systemPrompt } = promptBundle;
    const messages: LlmMessageInput[] = [];
    let pendingToolResults: LlmToolResult[] | undefined;
    let noToolStreak = 0;

    for (let round = 0; round < maxRounds; round += 1) {
      const response = await invokeLlm({
        apiKey: llm.apiKey,
        model: llm.apiModel,
        baseUrl: llm.apiBase || null,
        systemPrompt,
        prompt: messages.length === 0 ? prompt : undefined,
        messages: messages.length > 0 ? messages : undefined,
        mode: "tool",
        tools: promptBundle.tools,
        toolResults: pendingToolResults,
        toolChoice: "auto",
        timeoutSecs: 120,
      });
      pendingToolResults = undefined;

      if (response.status !== "requires_tool" || !response.toolCalls.length) {
        const content = response.message?.trim() ?? "";
        noToolStreak += 1;
        if (noToolStreak <= NO_TOOL_RETRY) {
          messages.push({
            role: "assistant",
            content,
          });
          messages.push({
            role: "user",
            content: "请不要输出解释文字，必须继续调用工具完成任务；若已完成请调用 finish。",
          });
          continue;
        }
        return {
          status: "stopped",
          summary: content,
          stopReason: "no_tool_call",
        };
      }

      noToolStreak = 0;
      messages.push({
        role: "assistant",
        content: response.message ?? "",
        toolCalls: response.toolCalls,
      });

      const toolResults: LlmToolResult[] = [];
      for (const call of response.toolCalls) {
        const executed = executeToolCall(call);
        toolResults.push({
          toolCallId: call.id,
          content: executed.raw,
        });

        const status = String(executed.parsed?.status ?? "").toLowerCase();
        if (call.function.name === "finish" || status === "finished") {
          return {
            status: "finished",
            summary: String(executed.parsed?.summary ?? state.finishedSummary ?? ""),
            stopReason: "",
          };
        }
        if (status === "stopped") {
          return {
            status: "stopped",
            summary: String(executed.parsed?.message ?? ""),
            stopReason: String(executed.parsed?.stop_reason ?? "stopped"),
          };
        }
      }
      pendingToolResults = toolResults;
    }

    return {
      status: "max_rounds",
      summary: "",
      stopReason: "max_rounds",
    };
  };

  const firstRunResult = await runAgentSession(promptBundle.initialTask, FIRST_PASS_MAX_AGENT_ROUNDS);
  const firstRoundChanges = state.changes.length;
  let finalRunResult = firstRunResult;
  let secondRoundChanges = 0;

  if (firstRoundChanges > 0) {
    const focusRanges = buildFocusRescanRanges(
      Array.from(state.changedIndexes.values()).sort((a, b) => a - b),
      working.length,
      FOCUS_RESCAN_PADDING,
    );
    if (focusRanges.length > 0) {
      const focusTask = buildFocusRescanTask(terms, focusRanges, working.length);
      finalRunResult = await runAgentSession(focusTask, SECOND_PASS_MAX_AGENT_ROUNDS);
      secondRoundChanges = state.changes.length - firstRoundChanges;
    }
  }

  rebuildWordsFromCorrections(working, originalTexts, state.changes);

  const changedCount = state.changes.length;
  const replacementStats = summarizeReplacementStats(state.changes);
  const summary = state.finishedSummary.trim()
    || (changedCount > 0
      ? secondRoundChanges > 0
        ? `已修改 ${changedCount} 处（第1轮 ${firstRoundChanges}，第2轮 ${secondRoundChanges}）`
        : `已修改 ${changedCount} 处`
      : finalRunResult.status === "stopped"
        ? `提前停止：${finalRunResult.stopReason}`
        : "未发现需要矫正的项");

  return {
    segments: working,
    words: flattenWords(working),
    changedCount,
    summary,
    replacementStats,
  };
}

function summarizeReplacementStats(
  changes: CorrectionRecord[],
): Array<{ oldText: string; newText: string; count: number }> {
  const map = new Map<string, { oldText: string; newText: string; count: number }>();
  for (const change of changes) {
    const oldText = change.oldText.trim();
    const newText = change.newText.trim();
    if (!oldText || !newText) continue;
    const key = `${oldText}\u0000${newText}`;
    const current = map.get(key);
    if (current) {
      current.count += 1;
    } else {
      map.set(key, { oldText, newText, count: 1 });
    }
  }
  return Array.from(map.values()).sort((a, b) => b.count - a.count);
}

function buildFocusRescanRanges(changedIndexes: number[], total: number, padding: number): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  for (const idx of changedIndexes) {
    const start = Math.max(0, idx - padding);
    const end = Math.min(total, idx + padding + 1);
    ranges.push([start, end]);
  }
  return mergeRanges(ranges);
}

function mergeRanges(ranges: Array<[number, number]>): Array<[number, number]> {
  if (!ranges.length) return [];
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  const merged: Array<[number, number]> = [sorted[0]];
  for (let i = 1; i < sorted.length; i += 1) {
    const [start, end] = sorted[i];
    const last = merged[merged.length - 1];
    if (start <= last[1]) {
      last[1] = Math.max(last[1], end);
    } else {
      merged.push([start, end]);
    }
  }
  return merged;
}

function formatRangesBrief(ranges: Array<[number, number]>, maxShow = 20): string {
  if (!ranges.length) return "[]";
  const brief = ranges.slice(0, maxShow).map(([start, end]) => `[${start}-${Math.max(start, end - 1)}]`);
  if (ranges.length > maxShow) brief.push("...");
  return brief.join(" ");
}

function buildFocusRescanTask(terms: HotwordTerm[], focusRanges: Array<[number, number]>, totalSentences: number): string {
  const termNames = terms.map((t) => t.name).join(", ");
  return `请对这些重点窗口做第二轮复扫，继续检查是否还有遗漏的术语识别错误：${termNames}\n\n重点复扫窗口：${formatRangesBrief(focusRanges)}\n请优先检查这些窗口，不要重新从头浏览全部 ${totalSentences} 句。`;
}

function parseTerms(rawTerms: string[]): HotwordTerm[] {
  const out: HotwordTerm[] = [];
  const seen = new Set<string>();
  for (const raw of rawTerms) {
    const value = raw.trim();
    if (!value) continue;
    const pair = splitTerm(value);
    const name = pair.name.trim();
    if (!name) continue;
    const key = name.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ name, meaning: pair.meaning });
  }
  return out;
}

function splitTerm(raw: string): { name: string; meaning: string | null } {
  const separators = [" : ", ": ", "：", ":"];
  for (const separator of separators) {
    const pos = raw.indexOf(separator);
    if (pos <= 0) continue;
    const left = raw.slice(0, pos).trim();
    const right = raw.slice(pos + separator.length).trim();
    if (!left || !right) continue;
    return { name: left, meaning: right };
  }
  return { name: raw, meaning: null };
}

function replaceInText(text: string, oldText: string, newText: string): {
  text: string;
  matches: Array<{ start: number; end: number; matchedText: string }>;
} {
  const matches = findSurfaceMatches(text, oldText);
  if (!matches.length) return { text, matches: [] };

  let next = text;
  for (let i = matches.length - 1; i >= 0; i -= 1) {
    const match = matches[i];
    next = `${next.slice(0, match.start)}${newText}${next.slice(match.end)}`;
  }
  return { text: next, matches };
}

function findSurfaceMatches(text: string, source: string): Array<{ start: number; end: number; matchedText: string }> {
  const raw: Array<{ start: number; end: number; matchedText: string }> = [];
  for (const variant of buildSurfaceVariants(source)) {
    if (!variant) continue;
    let from = 0;
    while (from <= text.length - variant.length) {
      const index = text.indexOf(variant, from);
      if (index < 0) break;
      const end = index + variant.length;
      if (isWordBoundary(text, index, end)) {
        raw.push({ start: index, end, matchedText: text.slice(index, end) });
      }
      from = index + 1;
    }
  }

  raw.sort((a, b) => (a.start - b.start) || ((b.end - b.start) - (a.end - a.start)));
  const deduped: Array<{ start: number; end: number; matchedText: string }> = [];
  let occupiedEnd = -1;
  for (const item of raw) {
    if (item.start < occupiedEnd) continue;
    deduped.push(item);
    occupiedEnd = item.end;
  }
  return deduped;
}

function buildSurfaceVariants(text: string): string[] {
  const base = text.trim();
  if (!base) return [];
  if (!/[\s\-_]/.test(base)) return [base];
  const tokens = base.split(/[\s\-_]+/).map((t) => t.trim()).filter(Boolean);
  if (tokens.length < 2) return [base];
  const variants = new Set<string>([
    base,
    tokens.join(" "),
    tokens.join("-"),
    tokens.join(""),
  ]);
  return Array.from(variants.values()).sort((a, b) => b.length - a.length);
}

function isWordBoundary(text: string, start: number, end: number): boolean {
  if (start > 0 && isLetter(text[start - 1])) return false;
  if (end < text.length && isLetter(text[end])) return false;
  return true;
}

function isLetter(char: string): boolean {
  return /\p{L}/u.test(char);
}

function rebuildWordsFromCorrections(
  segments: TimedHotwordSegment[],
  originalTexts: string[],
  corrections: CorrectionRecord[],
): void {
  for (let i = 0; i < segments.length; i += 1) {
    const seg = segments[i];
    const segCorrections = corrections
      .filter((item) => item.segmentIdx === i)
      .sort((a, b) => b.startIdx - a.startIdx);
    if (!segCorrections.length || !seg.words.length) continue;

    const chunkMap = buildChunkMap(originalTexts[i], seg.words);
    if (!chunkMap.length) continue;

    const finalWords: WordToken[] = [];
    let skipUntil = -1;

    for (const item of chunkMap) {
      if (item.startIdx < skipUntil) {
        continue;
      }

      const correction = segCorrections.find((corr) => !(item.endIdx <= corr.startIdx || item.startIdx >= corr.endIdx));
      if (!correction) {
        finalWords.push(item.word);
        continue;
      }

      const affected = chunkMap.filter((entry) => !(entry.endIdx <= correction.startIdx || entry.startIdx >= correction.endIdx));
      if (!affected.length) {
        finalWords.push(item.word);
        continue;
      }

      const fixedStart = affected[0].word.start;
      const fixedEnd = affected[affected.length - 1].word.end;
      finalWords.push(...splitTextIntoWordsWithTiming(correction.newText, fixedStart, fixedEnd));
      skipUntil = correction.endIdx;
    }

    if (finalWords.length > 0) {
      seg.words = finalWords;
      seg.startMs = Math.max(0, Math.round(finalWords[0].start * 1000));
      seg.endMs = Math.max(seg.startMs, Math.round(finalWords[finalWords.length - 1].end * 1000));
    }
  }
}

function buildChunkMap(text: string, words: WordToken[]): Array<{ word: WordToken; startIdx: number; endIdx: number }> {
  const map: Array<{ word: WordToken; startIdx: number; endIdx: number }> = [];
  let cursor = 0;

  for (const word of words) {
    const w = word.word ?? "";
    if (!w) continue;
    let start = text.indexOf(w, cursor);
    if (start < 0) {
      start = cursor;
    }
    const end = Math.min(text.length, start + w.length);
    map.push({ word, startIdx: start, endIdx: end });
    cursor = end;
  }

  return map;
}

function splitTextIntoWordsWithTiming(text: string, start: number, end: number): WordToken[] {
  const parts = text.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return [];
  const duration = Math.max(0, end - start);
  const chunk = parts.length > 0 ? duration / parts.length : 0;

  let cursor = start;
  return parts.map((part, idx) => {
    const wordStart = cursor;
    const wordEnd = idx === parts.length - 1 ? end : cursor + chunk;
    cursor = wordEnd;
    return {
      word: part,
      start: wordStart,
      end: wordEnd,
    };
  });
}

function flattenWords(segments: TimedHotwordSegment[]): WordToken[] {
  return segments.flatMap((seg) => seg.words.map((word) => ({ ...word })));
}










