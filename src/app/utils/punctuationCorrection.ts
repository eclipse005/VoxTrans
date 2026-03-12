import { invoke } from "@tauri-apps/api/core";
import type { WordToken } from "../../features/media/types";

type LlmToolCall = {
  id: string;
  type: string;
  function: {
    name: string;
    arguments: string;
  };
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
  timeoutSecs?: number;
  maxRetries?: number;
};

type LlmInteractResponse = {
  status: "completed" | "requires_tool";
  message?: string;
  toolCalls: LlmToolCall[];
};

type LlmConfig = {
  apiKey: string;
  apiBase: string;
  apiModel: string;
};

type TemporarySentence = {
  startWord: number;
  endWordExclusive: number;
  text: string;
};

type PunctuationRestoreArgs = {
  words: WordToken[];
  llm: LlmConfig;
  invokeLlm: (request: LlmInteractRequest) => Promise<LlmInteractResponse>;
};

type BuildPunctuationRestorePromptResponse = {
  systemPrompt: string;
  userPrompt: string;
};

export type PunctuationRestoreResult = {
  words: WordToken[];
  sentenceTotal: number;
  suspiciousCount: number;
  restoredCount: number;
  acceptedCount: number;
  rejectedCount: number;
  changedExamples: Array<{
    sentenceIndex: number;
    originalText: string;
    restoredText: string;
  }>;
};

const SENTENCE_END_RE = /[.!?。！？][)"'\]]*$/;
const ABBREVIATIONS = new Set([
  "mr.",
  "mrs.",
  "ms.",
  "dr.",
  "prof.",
  "sr.",
  "jr.",
  "st.",
  "vs.",
  "etc.",
  "e.g.",
  "i.e.",
  "u.s.",
  "u.k.",
]);

export async function restorePunctuationOnWords(
  args: PunctuationRestoreArgs,
): Promise<PunctuationRestoreResult> {
  const { words, llm, invokeLlm } = args;
  if (!words.length) {
    return {
      words,
      sentenceTotal: 0,
      suspiciousCount: 0,
      restoredCount: 0,
      acceptedCount: 0,
      rejectedCount: 0,
      changedExamples: [],
    };
  }

  const working = words.map((word) => ({ ...word }));
  const sentences = splitTemporarySentences(working);
  const suspicious = sentences.filter((sentence) => isSuspiciousSentence(sentence.text));
  let restoredCount = 0;
  let acceptedCount = 0;
  let rejectedCount = 0;
  const changedExamples: Array<{
    sentenceIndex: number;
    originalText: string;
    restoredText: string;
  }> = [];

  for (const sentence of suspicious) {
    const sentenceIndex = sentences.findIndex((item) => item.startWord === sentence.startWord && item.endWordExclusive === sentence.endWordExclusive);
    const prompt = await invoke<BuildPunctuationRestorePromptResponse>("build_punctuation_restore_prompt", {
      request: { text: sentence.text },
    });
    const response = await invokeLlm({
      apiKey: llm.apiKey,
      model: llm.apiModel,
      baseUrl: llm.apiBase || null,
      systemPrompt: prompt.systemPrompt,
      prompt: prompt.userPrompt,
      mode: "chat",
      timeoutSecs: 120,
      maxRetries: 2,
    });
    const rawText = response.message ?? "";
    const parsed = parseJsonText(rawText);
    const restoredText = typeof parsed?.text === "string" ? parsed.text.trim() : "";
    if (!restoredText) {
      rejectedCount += 1;
      continue;
    }

    restoredCount += 1;
    const originalTokens = working
      .slice(sentence.startWord, sentence.endWordExclusive)
      .map((word) => word.word ?? "");
    const projected = projectRestoredTextToWordTokens(restoredText, originalTokens.length);
    if (!projected) {
      rejectedCount += 1;
      continue;
    }

    if (!sameLexicalTokens(originalTokens, projected)) {
      rejectedCount += 1;
      continue;
    }

    for (let i = sentence.startWord, j = 0; i < sentence.endWordExclusive; i += 1, j += 1) {
      working[i].word = projected[j];
    }
    acceptedCount += 1;
    if (changedExamples.length < 8) {
      changedExamples.push({
        sentenceIndex: sentenceIndex >= 0 ? sentenceIndex : 0,
        originalText: sentence.text,
        restoredText: joinWordTexts(projected),
      });
    }
  }

  return {
    words: working,
    sentenceTotal: sentences.length,
    suspiciousCount: suspicious.length,
    restoredCount,
    acceptedCount,
    rejectedCount,
    changedExamples,
  };
}

function splitTemporarySentences(words: WordToken[]): TemporarySentence[] {
  const sentences: TemporarySentence[] = [];
  let start = 0;

  for (let i = 0; i < words.length; i += 1) {
    const token = (words[i].word ?? "").trim();
    if (!token) continue;
    if (!isSentenceEndToken(token)) continue;
    if (isAbbreviationToken(token)) continue;

    sentences.push({
      startWord: start,
      endWordExclusive: i + 1,
      text: joinWordTexts(words.slice(start, i + 1).map((w) => w.word ?? "")),
    });
    start = i + 1;
  }

  if (start < words.length) {
    sentences.push({
      startWord: start,
      endWordExclusive: words.length,
      text: joinWordTexts(words.slice(start).map((w) => w.word ?? "")),
    });
  }

  return sentences.filter((sentence) => sentence.text.trim().length > 0);
}

function isSentenceEndToken(token: string): boolean {
  return SENTENCE_END_RE.test(token);
}

function isAbbreviationToken(token: string): boolean {
  const normalized = token
    .toLowerCase()
    .replace(/^[("'[]+/, "")
    .replace(/[)"'\]]+$/, "");
  if (ABBREVIATIONS.has(normalized)) return true;
  return /^[a-z]\.$/.test(normalized);
}

function isSuspiciousSentence(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return false;
  if (shouldSkipSentence(trimmed)) return false;
  // With pre-splitting by end punctuation, the practical suspicious signal
  // is: this temporary sentence still has no sentence-ending token.
  return !SENTENCE_END_RE.test(trimmed);
}

function shouldSkipSentence(text: string): boolean {
  // hard_skip #1: too short snippets usually do not need LLM punctuation pass.
  if (wordCount(text) <= 2) return true;
  // hard_skip #2: pure number/time-ish phrase.
  if (/^[\d\s:./-]+$/.test(text)) return true;
  // hard_skip #3: url/path-like text.
  if (looksLikeUrlOrPath(text)) return true;
  // hard_skip #4: code-like or markup-like line.
  if (looksLikeCodeLikeText(text)) return true;
  // hard_skip #5: version-heavy / decimal-heavy fragments.
  if (looksLikeVersionOrDecimalHeavy(text)) return true;
  return false;
}

function wordCount(text: string): number {
  return text.split(/\s+/).filter(Boolean).length;
}

function looksLikeUrlOrPath(text: string): boolean {
  return /(https?:\/\/|www\.|[a-z]:\\|\\|\/)/i.test(text);
}

function looksLikeCodeLikeText(text: string): boolean {
  return /[{}[\]<>`]|=>|::|==|!=|\bfunction\b|\bconst\b|\blet\b/i.test(text);
}

function looksLikeVersionOrDecimalHeavy(text: string): boolean {
  const versionMatches = text.match(/\bv?\d+(?:\.\d+){1,}\b/gi) ?? [];
  if (versionMatches.length >= 2) return true;
  const decimalMatches = text.match(/\b\d+\.\d+\b/g) ?? [];
  return decimalMatches.length >= 3;
}


function parseJsonText(raw: string): Record<string, unknown> | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  try {
    return JSON.parse(trimmed) as Record<string, unknown>;
  } catch {
    const fenced = trimmed.match(/```(?:json)?\s*([\s\S]*?)```/i);
    if (!fenced) return null;
    try {
      return JSON.parse(fenced[1]) as Record<string, unknown>;
    } catch {
      return null;
    }
  }
}

function projectRestoredTextToWordTokens(text: string, expectedCount: number): string[] | null {
  if (expectedCount <= 0) return [];
  const rawTokens = text.split(/\s+/).map((token) => token.trim()).filter(Boolean);
  if (!rawTokens.length) return null;
  const merged = mergeStandalonePunctuation(rawTokens);
  if (merged.length !== expectedCount) return null;
  return merged;
}

function mergeStandalonePunctuation(tokens: string[]): string[] {
  const out: string[] = [];
  for (const token of tokens) {
    if (/^[^\p{L}\p{N}]+$/u.test(token) && out.length > 0) {
      out[out.length - 1] = `${out[out.length - 1]}${token}`;
    } else {
      out.push(token);
    }
  }
  return out;
}

function sameLexicalTokens(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (normalizeLexical(a[i]) !== normalizeLexical(b[i])) return false;
  }
  return true;
}

function normalizeLexical(token: string): string {
  return token
    .toLowerCase()
    .replace(/^[^\p{L}\p{N}]+/gu, "")
    .replace(/[^\p{L}\p{N}]+$/gu, "");
}

function joinWordTexts(words: string[]): string {
  return words.map((word) => word.trim()).filter(Boolean).join(" ");
}
