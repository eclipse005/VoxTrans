# Hotwords Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add dedicated ASR hotword correction between Step1 ASR and Step2 sentence segmentation, including Chinese pinyin/first-letter recall and non-Chinese fuzzy alias recall.

**Architecture:** Store hotwords beside terminology in saved settings, but keep runtime correction in a new backend Step1.5 artifact. The hotword service normalizes entries, recalls candidates locally, optionally asks LLM to judge candidates, applies strict token-range replacements, and passes corrected word tokens to Step2 without changing Step2 output shape.

**Tech Stack:** Rust/Tauri backend, React/TypeScript frontend, serde JSON artifacts, `pinyin` crate for Chinese pinyin, existing OpenAI-compatible LLM request helpers in `src-tauri/src/services/llm.rs`.

---

## File Structure

- Create `src-tauri/src/services/hotwords.rs`: core types, normalization, pinyin generation, fuzzy aliases, recall, decisions, corrections, tests.
- Modify `src-tauri/src/services/mod.rs`: export `hotwords`.
- Modify `src-tauri/Cargo.toml`: add `pinyin`.
- Modify `src-tauri/src/services/preferences.rs`: add `HotwordTerm`, `HotwordGroup`, saved settings fields, normalization.
- Modify `src-tauri/src/commands/preferences.rs`: mirror hotword command DTOs and service mapping.
- Modify `src-tauri/src/commands/workspace.rs`: snapshot hotwords, runtime hotword entries, Step1.5 pipeline step, Step2 words source.
- Modify `src/features/media/types.ts`: add `HotwordTerm`, `HotwordGroup`, `enableHotwords`, `hotwordGroups`.
- Create `src/app/utils/hotwords.ts`: group/term creation, parsing, normalization.
- Create `src/app/components/HotwordsModal.tsx`: dedicated UI beside terminology.
- Modify `src/app/App.tsx`, `src/app/components/Navbar.tsx`, `src/app/components/SettingsModal.tsx`, `src/app/state/appReducer.ts`, `src/app/hooks/useAppPersistence.ts`, `src/app/hooks/useSettingsController.ts`, `src/app/hooks/queue/useQueueRunner.ts`, `src/app/hooks/queue/useQueueScheduler.ts`: wire settings and modal.
- Modify CSS in `src/app/styles/components/subtitle-settings.css` only if existing modal classes need hotword-specific aliases.

---

### Task 1: Add Hotword Settings Types And Normalization

**Files:**
- Modify: `src-tauri/src/services/preferences.rs`
- Modify: `src-tauri/src/commands/preferences.rs`
- Modify: `src/features/media/types.ts`

- [ ] **Step 1: Add backend saved settings tests**

Append tests to `src-tauri/src/services/preferences.rs` under a new `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::{
        HotwordGroup, HotwordTerm, SavedSettings, SubtitleRenderStyle, normalize_saved_settings,
    };

    fn settings_with_hotwords(groups: Vec<HotwordGroup>) -> SavedSettings {
        SavedSettings {
            provider: "cpu".to_string(),
            chunk_target_seconds: 180,
            subtitle_max_words_per_segment: 20,
            subtitle_length_reference: 28,
            asr_model: crate::services::model::DEFAULT_ASR_MODEL.to_string(),
            demucs_model: "htdemucs_ft".to_string(),
            enable_vocal_separation: false,
            translate_api_key: String::new(),
            translate_base_url: "https://api.openai.com/v1".to_string(),
            translate_model: "gpt-4.1-mini".to_string(),
            llm_concurrency: 4,
            terminology_groups: Vec::new(),
            enable_terminology: true,
            hotword_groups: groups,
            enable_hotwords: true,
            enable_subtitle_beautify: true,
            auto_burn_hard_subtitle: false,
            subtitle_burn_mode: "bilingualSourceFirst".to_string(),
            subtitle_render_style: SubtitleRenderStyle::default(),
        }
    }

    #[test]
    fn normalize_hotwords_trims_aliases_and_drops_empty_words() {
        let normalized = normalize_saved_settings(settings_with_hotwords(vec![HotwordGroup {
            id: " g1 ".to_string(),
            name: " AI ".to_string(),
            terms: vec![
                HotwordTerm {
                    id: " h1 ".to_string(),
                    word: " Claude Code ".to_string(),
                    aliases: vec![" cloud code ".to_string(), "cloud code".to_string()],
                    lang: "non_zh".to_string(),
                    note: " product ".to_string(),
                },
                HotwordTerm {
                    id: " h2 ".to_string(),
                    word: " ".to_string(),
                    aliases: vec!["x".to_string()],
                    lang: "zh".to_string(),
                    note: String::new(),
                },
            ],
        }]));

        assert!(normalized.enable_hotwords);
        assert_eq!(normalized.hotword_groups.len(), 1);
        assert_eq!(normalized.hotword_groups[0].name, "AI");
        assert_eq!(normalized.hotword_groups[0].terms.len(), 1);
        assert_eq!(normalized.hotword_groups[0].terms[0].word, "Claude Code");
        assert_eq!(normalized.hotword_groups[0].terms[0].aliases, vec!["cloud code"]);
        assert_eq!(normalized.hotword_groups[0].terms[0].lang, "non_zh");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p voxtrans normalize_hotwords_trims_aliases_and_drops_empty_words`

Expected: FAIL because `HotwordGroup`, `HotwordTerm`, `hotword_groups`, and `normalize_saved_settings` test visibility do not exist yet.

- [ ] **Step 3: Implement backend settings model**

In `src-tauri/src/services/preferences.rs`, add structs after `TerminologyGroup`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HotwordTerm {
    pub id: String,
    pub word: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_hotword_lang")]
    pub lang: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HotwordGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub terms: Vec<HotwordTerm>,
}
```

Add to `SavedSettings`:

```rust
#[serde(default)]
pub hotword_groups: Vec<HotwordGroup>,
#[serde(default = "default_true")]
pub enable_hotwords: bool,
```

Set defaults in `default_settings()`:

```rust
hotword_groups: normalize_hotword_groups(Vec::new()),
enable_hotwords: true,
```

Set normalized values in `normalize_saved_settings()`:

```rust
hotword_groups: normalize_hotword_groups(settings.hotword_groups),
enable_hotwords: settings.enable_hotwords,
```

Add helper functions near terminology normalization:

```rust
fn default_hotword_lang() -> String {
    "auto".to_string()
}

fn normalize_hotword_lang(value: &str) -> String {
    match value.trim() {
        "zh" => "zh".to_string(),
        "non_zh" => "non_zh".to_string(),
        _ => "auto".to_string(),
    }
}

fn normalize_hotword_groups(groups: Vec<HotwordGroup>) -> Vec<HotwordGroup> {
    let mut seen_group_ids = HashSet::new();
    let mut normalized = Vec::new();
    for (group_idx, group) in groups.into_iter().enumerate() {
        let mut group_id = group.id.trim().to_string();
        if group_id.is_empty() || !seen_group_ids.insert(group_id.clone()) {
            group_id = make_entity_id("hotword-group", group_idx);
            seen_group_ids.insert(group_id.clone());
        }
        let name = if group.name.trim().is_empty() {
            "默认".to_string()
        } else {
            group.name.trim().to_string()
        };
        normalized.push(HotwordGroup {
            id: group_id,
            name,
            terms: normalize_hotword_terms(group.terms, group_idx),
        });
    }
    if normalized.is_empty() {
        return vec![HotwordGroup {
            id: make_entity_id("hotword-group", 0),
            name: "默认".to_string(),
            terms: Vec::new(),
        }];
    }
    normalized
}

fn normalize_hotword_terms(terms: Vec<HotwordTerm>, group_idx: usize) -> Vec<HotwordTerm> {
    let mut normalized = Vec::new();
    let mut seen_term_ids = HashSet::new();
    for (term_idx, term) in terms.into_iter().enumerate() {
        let word = term.word.trim();
        if word.is_empty() {
            continue;
        }
        let mut term_id = term.id.trim().to_string();
        if term_id.is_empty() || !seen_term_ids.insert(term_id.clone()) {
            let seq = group_idx.saturating_mul(10_000).saturating_add(term_idx);
            term_id = make_entity_id("hotword", seq);
            seen_term_ids.insert(term_id.clone());
        }
        let mut seen_aliases = HashSet::<String>::new();
        let aliases = term
            .aliases
            .into_iter()
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty())
            .filter(|alias| seen_aliases.insert(alias.to_lowercase()))
            .collect::<Vec<_>>();
        normalized.push(HotwordTerm {
            id: term_id,
            word: word.to_string(),
            aliases,
            lang: normalize_hotword_lang(&term.lang),
            note: term.note.trim().to_string(),
        });
    }
    normalized
}
```

Make `normalize_saved_settings` visible to tests by leaving it private; child test modules can call it through `super::normalize_saved_settings`.

- [ ] **Step 4: Mirror settings in command DTOs**

In `src-tauri/src/commands/preferences.rs`, add command structs after `TerminologyGroupCommand`:

```rust
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HotwordTermCommand {
    pub id: String,
    pub word: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HotwordGroupCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub terms: Vec<HotwordTermCommand>,
}
```

Add to `SavedSettingsCommand`:

```rust
#[serde(default)]
pub hotword_groups: Vec<HotwordGroupCommand>,
#[serde(default = "default_true")]
pub enable_hotwords: bool,
```

In `to_service_settings`, map:

```rust
hotword_groups: settings
    .hotword_groups
    .into_iter()
    .map(|group| crate::services::preferences::HotwordGroup {
        id: group.id,
        name: group.name,
        terms: group
            .terms
            .into_iter()
            .map(|term| crate::services::preferences::HotwordTerm {
                id: term.id,
                word: term.word,
                aliases: term.aliases,
                lang: term.lang,
                note: term.note,
            })
            .collect(),
    })
    .collect(),
enable_hotwords: settings.enable_hotwords,
```

In `from_service_settings`, map:

```rust
hotword_groups: settings
    .hotword_groups
    .into_iter()
    .map(|group| HotwordGroupCommand {
        id: group.id,
        name: group.name,
        terms: group
            .terms
            .into_iter()
            .map(|term| HotwordTermCommand {
                id: term.id,
                word: term.word,
                aliases: term.aliases,
                lang: term.lang,
                note: term.note,
            })
            .collect(),
    })
    .collect(),
enable_hotwords: settings.enable_hotwords,
```

- [ ] **Step 5: Add frontend settings types**

In `src/features/media/types.ts`, add:

```ts
export type HotwordLang = "auto" | "zh" | "non_zh";

export type HotwordTerm = {
  id: string;
  word: string;
  aliases: string[];
  lang: HotwordLang;
  note: string;
};

export type HotwordGroup = {
  id: string;
  name: string;
  terms: HotwordTerm[];
};
```

Add to `SavedSettings`:

```ts
hotwordGroups: HotwordGroup[];
enableHotwords: boolean;
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p voxtrans normalize_hotwords_trims_aliases_and_drops_empty_words`

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```powershell
git add src-tauri/src/services/preferences.rs src-tauri/src/commands/preferences.rs src/features/media/types.ts
git commit -m "feat: add hotword settings model"
```

---

### Task 2: Build Hotword Core Service With Local Recall

**Files:**
- Create: `src-tauri/src/services/hotwords.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add dependency**

In `src-tauri/Cargo.toml`, add:

```toml
pinyin = "0.10"
```

- [ ] **Step 2: Write failing core tests**

Create `src-tauri/src/services/hotwords.rs` with tests first:

```rust
use serde::{Deserialize, Serialize};

use crate::services::transcribe::WordTokenDto;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HotwordLang {
    Auto,
    Zh,
    NonZh,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordEntry {
    pub word: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub note: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(index: usize, text: &str) -> WordTokenDto {
        WordTokenDto {
            start: index as f64,
            end: index as f64 + 0.4,
            word: text.to_string(),
        }
    }

    #[test]
    fn chinese_pinyin_recalls_homophone_without_alias() {
        let hotwords = vec![HotwordEntry {
            word: "浩叔".to_string(),
            aliases: Vec::new(),
            lang: "zh".to_string(),
            note: String::new(),
        }];
        let words = vec![w(0, "今天"), w(1, "浩书"), w(2, "讲"), w(3, "Cursor")];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "浩书");
        assert_eq!(candidates[0].target, "浩叔");
        assert_eq!(candidates[0].source_kind, "pinyin");
    }

    #[test]
    fn chinese_first_letters_recalls_short_ascii_abbreviation() {
        let hotwords = vec![HotwordEntry {
            word: "浩叔".to_string(),
            aliases: Vec::new(),
            lang: "zh".to_string(),
            note: String::new(),
        }];
        let words = vec![w(0, "hs"), w(1, "今天"), w(2, "更新")];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "hs");
        assert_eq!(candidates[0].source_kind, "first_letters");
    }

    #[test]
    fn chinese_alias_recalls_direct_match() {
        let hotwords = vec![HotwordEntry {
            word: "浩叔".to_string(),
            aliases: vec!["皓叔".to_string()],
            lang: "zh".to_string(),
            note: String::new(),
        }];
        let words = vec![w(0, "这"), w(1, "皓叔"), w(2, "说")];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_kind, "alias");
    }

    #[test]
    fn non_chinese_generated_alias_recalls_when_alias_is_missing() {
        let hotwords = vec![HotwordEntry {
            word: "Claude Code".to_string(),
            aliases: Vec::new(),
            lang: "non_zh".to_string(),
            note: String::new(),
        }];
        let words = vec![w(0, "I"), w(1, "use"), w(2, "cloud"), w(3, "code")];

        let candidates = recall_hotword_candidates(&words, &hotwords);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_text, "cloud code");
        assert_eq!(candidates[0].target, "Claude Code");
        assert_eq!(candidates[0].source_kind, "generated_alias");
    }

    #[test]
    fn accepted_multi_token_correction_merges_timing() {
        let words = vec![w(0, "use"), w(1, "cloud"), w(2, "code"), w(3, "now")];
        let candidates = vec![HotwordCandidate {
            id: "c1".to_string(),
            start_index: 1,
            end_index: 2,
            source_text: "cloud code".to_string(),
            target: "Claude Code".to_string(),
            source_kind: "generated_alias".to_string(),
            context: "use cloud code now".to_string(),
        }];
        let decisions = vec![HotwordDecision {
            candidate_id: "c1".to_string(),
            replace: true,
            target: "Claude Code".to_string(),
            reason: "product name".to_string(),
            error: String::new(),
        }];

        let corrected = apply_hotword_corrections(&words, &candidates, &decisions);

        assert_eq!(corrected.len(), 3);
        assert_eq!(corrected[1].word, "Claude Code");
        assert_eq!(corrected[1].start, 1.0);
        assert_eq!(corrected[1].end, 2.4);
        assert_eq!(corrected[2].word, "now");
    }
}
```

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p voxtrans hotwords`

Expected: FAIL with missing functions/types `recall_hotword_candidates`, `HotwordCandidate`, `HotwordDecision`, and `apply_hotword_corrections`.

- [ ] **Step 4: Implement core types and local recall**

Add below `HotwordEntry`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedHotword {
    pub word: String,
    pub aliases: Vec<String>,
    pub generated_aliases: Vec<String>,
    pub pinyin: String,
    pub first_letters: String,
    pub lang: HotwordLang,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordCandidate {
    pub id: String,
    pub start_index: usize,
    pub end_index: usize,
    pub source_text: String,
    pub target: String,
    pub source_kind: String,
    pub context: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordDecision {
    pub candidate_id: String,
    pub replace: bool,
    pub target: String,
    pub reason: String,
    pub error: String,
}

pub fn recall_hotword_candidates(
    words: &[WordTokenDto],
    hotwords: &[HotwordEntry],
) -> Vec<HotwordCandidate> {
    let normalized = normalize_hotwords(hotwords);
    let mut candidates = Vec::new();
    for hotword in normalized {
        candidates.extend(recall_non_chinese(words, &hotword));
        candidates.extend(recall_chinese_alias(words, &hotword));
        candidates.extend(recall_chinese_homophone(words, &hotword));
    }
    dedupe_candidates(candidates)
}
```

Implement helpers:

```rust
fn normalize_hotwords(entries: &[HotwordEntry]) -> Vec<NormalizedHotword> {
    entries
        .iter()
        .filter_map(|entry| {
            let word = entry.word.trim();
            if word.is_empty() {
                return None;
            }
            let lang = normalize_lang(&entry.lang, word);
            let aliases = dedupe_strings(entry.aliases.iter().map(String::as_str));
            let generated_aliases = if matches!(lang, HotwordLang::NonZh) {
                generate_non_chinese_aliases(word, &aliases)
            } else {
                Vec::new()
            };
            let pinyin = chinese_pinyin(word);
            let first_letters = pinyin_first_letters(&pinyin);
            Some(NormalizedHotword {
                word: word.to_string(),
                aliases,
                generated_aliases,
                pinyin,
                first_letters,
                lang,
            })
        })
        .collect()
}

fn normalize_lang(raw: &str, word: &str) -> HotwordLang {
    match raw.trim() {
        "zh" => HotwordLang::Zh,
        "non_zh" => HotwordLang::NonZh,
        _ if word.chars().any(is_cjk) => HotwordLang::Zh,
        _ => HotwordLang::NonZh,
    }
}

fn dedupe_strings<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn generate_non_chinese_aliases(word: &str, user_aliases: &[String]) -> Vec<String> {
    let mut seeds = Vec::<String>::new();
    let lower = word.to_lowercase();
    seeds.push(lower.replace("claude", "cloud"));
    seeds.push(lower.replace("claude", "clod"));
    seeds.push(lower.replace("code", "cod"));
    seeds.push(lower.replace("cursor", "科舍"));
    dedupe_strings(
        seeds
            .iter()
            .map(String::as_str)
            .chain(user_aliases.iter().map(String::as_str)),
    )
    .into_iter()
    .filter(|alias| !alias.eq_ignore_ascii_case(word))
    .collect()
}

fn recall_non_chinese(words: &[WordTokenDto], hotword: &NormalizedHotword) -> Vec<HotwordCandidate> {
    if !matches!(hotword.lang, HotwordLang::NonZh) {
        return Vec::new();
    }
    let patterns = hotword
        .aliases
        .iter()
        .map(|value| (value.as_str(), "alias"))
        .chain(hotword.generated_aliases.iter().map(|value| (value.as_str(), "generated_alias")))
        .chain(std::iter::once((hotword.word.as_str(), "word")))
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for (pattern, source_kind) in patterns {
        let pattern_tokens = ascii_tokens(pattern);
        if pattern_tokens.is_empty() || pattern_tokens.len() > words.len() {
            continue;
        }
        for start in 0..=words.len() - pattern_tokens.len() {
            let end = start + pattern_tokens.len() - 1;
            let window = words[start..=end]
                .iter()
                .map(|word| normalize_ascii_token(&word.word))
                .collect::<Vec<_>>();
            if window == pattern_tokens {
                out.push(candidate(start, end, words, &hotword.word, source_kind));
            }
        }
    }
    out
}

fn recall_chinese_alias(words: &[WordTokenDto], hotword: &NormalizedHotword) -> Vec<HotwordCandidate> {
    if !matches!(hotword.lang, HotwordLang::Zh) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for alias in &hotword.aliases {
        for (start, end, text) in find_text_windows(words, alias.chars().count().max(1)) {
            if text == *alias {
                out.push(candidate_with_text(start, end, &text, words, &hotword.word, "alias"));
            }
        }
    }
    out
}

fn recall_chinese_homophone(words: &[WordTokenDto], hotword: &NormalizedHotword) -> Vec<HotwordCandidate> {
    if !matches!(hotword.lang, HotwordLang::Zh) || hotword.pinyin.is_empty() {
        return Vec::new();
    }
    let target_len = hotword.word.chars().count().max(1);
    let mut out = Vec::new();
    for (start, end, text) in find_text_windows(words, target_len) {
        if text == hotword.word {
            continue;
        }
        let pinyin = chinese_pinyin(&text);
        if !pinyin.is_empty() && pinyin == hotword.pinyin {
            out.push(candidate_with_text(start, end, &text, words, &hotword.word, "pinyin"));
            continue;
        }
        if text.is_ascii() && text.eq_ignore_ascii_case(&hotword.first_letters) {
            out.push(candidate_with_text(start, end, &text, words, &hotword.word, "first_letters"));
        }
    }
    out
}
```

Add pinyin and utility helpers:

```rust
fn chinese_pinyin(text: &str) -> String {
    use pinyin::ToPinyin;
    text.to_pinyin()
        .filter_map(|item| item.map(|p| p.plain().to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn pinyin_first_letters(pinyin: &str) -> String {
    pinyin
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .collect::<String>()
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

fn ascii_tokens(text: &str) -> Vec<String> {
    text.split_whitespace().map(normalize_ascii_token).filter(|v| !v.is_empty()).collect()
}

fn normalize_ascii_token(text: &str) -> String {
    text.trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

fn join_words(words: &[WordTokenDto]) -> String {
    let mut out = String::new();
    for word in words {
        if !out.is_empty() && word.word.is_ascii() {
            out.push(' ');
        }
        out.push_str(word.word.trim());
    }
    out
}

fn find_text_windows(words: &[WordTokenDto], char_len: usize) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    for start in 0..words.len() {
        let mut text = String::new();
        for end in start..words.len() {
            text.push_str(words[end].word.trim());
            if text.chars().count() == char_len {
                out.push((start, end, text.clone()));
            }
            if text.chars().count() >= char_len {
                break;
            }
        }
    }
    out
}

fn candidate(
    start: usize,
    end: usize,
    words: &[WordTokenDto],
    target: &str,
    source_kind: &str,
) -> HotwordCandidate {
    let source_text = join_words(&words[start..=end]);
    candidate_with_text(start, end, &source_text, words, target, source_kind)
}

fn candidate_with_text(
    start: usize,
    end: usize,
    source_text: &str,
    words: &[WordTokenDto],
    target: &str,
    source_kind: &str,
) -> HotwordCandidate {
    HotwordCandidate {
        id: format!("hotword-{start}-{end}-{}", target.to_lowercase()),
        start_index: start,
        end_index: end,
        source_text: source_text.to_string(),
        target: target.to_string(),
        source_kind: source_kind.to_string(),
        context: build_context(words, start, end),
    }
}

fn build_context(words: &[WordTokenDto], start: usize, end: usize) -> String {
    let left = start.saturating_sub(8);
    let right = (end + 8).min(words.len().saturating_sub(1));
    join_words(&words[left..=right])
}

fn dedupe_candidates(candidates: Vec<HotwordCandidate>) -> Vec<HotwordCandidate> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::<(usize, usize, String)>::new();
    for candidate in candidates {
        let key = (
            candidate.start_index,
            candidate.end_index,
            candidate.target.to_lowercase(),
        );
        if seen.insert(key) {
            out.push(candidate);
        }
    }
    out
}
```

Add correction application:

```rust
pub fn apply_hotword_corrections(
    words: &[WordTokenDto],
    candidates: &[HotwordCandidate],
    decisions: &[HotwordDecision],
) -> Vec<WordTokenDto> {
    let mut accepted = decisions
        .iter()
        .filter(|decision| decision.replace && decision.error.trim().is_empty())
        .filter_map(|decision| {
            candidates
                .iter()
                .find(|candidate| candidate.id == decision.candidate_id)
                .map(|candidate| (candidate, decision))
        })
        .collect::<Vec<_>>();
    accepted.sort_by_key(|(candidate, _)| (candidate.start_index, std::cmp::Reverse(candidate.end_index)));

    let mut occupied = vec![false; words.len()];
    let mut accepted_non_overlapping = Vec::new();
    for (candidate, decision) in accepted {
        if candidate.start_index >= words.len() || candidate.end_index >= words.len() || candidate.start_index > candidate.end_index {
            continue;
        }
        if occupied[candidate.start_index..=candidate.end_index].iter().any(|value| *value) {
            continue;
        }
        for item in &mut occupied[candidate.start_index..=candidate.end_index] {
            *item = true;
        }
        accepted_non_overlapping.push((candidate, decision));
    }

    let mut out = Vec::new();
    let mut index = 0usize;
    while index < words.len() {
        if let Some((candidate, decision)) = accepted_non_overlapping
            .iter()
            .find(|(candidate, _)| candidate.start_index == index)
        {
            out.push(WordTokenDto {
                start: words[candidate.start_index].start,
                end: words[candidate.end_index].end,
                word: decision.target.trim().to_string(),
            });
            index = candidate.end_index + 1;
            continue;
        }
        out.push(words[index].clone());
        index += 1;
    }
    out
}
```

- [ ] **Step 5: Export service module**

In `src-tauri/src/services/mod.rs`, add:

```rust
pub mod hotwords;
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p voxtrans hotwords`

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```powershell
git add src-tauri/Cargo.toml src-tauri/src/services/mod.rs src-tauri/src/services/hotwords.rs
git commit -m "feat: add hotword recall core"
```

---

### Task 3: Add Step1.5 Hotword Build Service And LLM Decision

**Files:**
- Modify: `src-tauri/src/services/hotwords.rs`

- [ ] **Step 1: Add service-level tests**

Append tests in `src-tauri/src/services/hotwords.rs`:

```rust
#[test]
fn disabled_hotwords_pass_through_words() {
    let words = vec![w(0, "cloud"), w(1, "code")];
    let response = build_hotword_correction(BuildHotwordCorrectionRequest {
        task_id: "task".to_string(),
        media_path: "demo.mp4".to_string(),
        source_lang: "en".to_string(),
        words: words.clone(),
        hotwords: Vec::new(),
        enabled: false,
        translate_api_key: String::new(),
        translate_base_url: String::new(),
        translate_model: String::new(),
    });

    assert!(!response.enabled);
    assert_eq!(response.words, words);
    assert!(response.candidates.is_empty());
}

#[test]
fn llm_unavailable_records_skipped_decision_and_keeps_words() {
    let words = vec![w(0, "cloud"), w(1, "code")];
    let response = build_hotword_correction(BuildHotwordCorrectionRequest {
        task_id: "task".to_string(),
        media_path: "demo.mp4".to_string(),
        source_lang: "en".to_string(),
        words: words.clone(),
        hotwords: vec![HotwordEntry {
            word: "Claude Code".to_string(),
            aliases: Vec::new(),
            lang: "non_zh".to_string(),
            note: String::new(),
        }],
        enabled: true,
        translate_api_key: String::new(),
        translate_base_url: String::new(),
        translate_model: String::new(),
    });

    assert!(response.enabled);
    assert_eq!(response.candidates.len(), 1);
    assert_eq!(response.decisions.len(), 1);
    assert_eq!(response.decisions[0].error, "llm_unavailable");
    assert_eq!(response.words, words);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p voxtrans disabled_hotwords_pass_through_words llm_unavailable_records_skipped_decision_and_keeps_words`

Expected: FAIL because request/response types and `build_hotword_correction` do not exist.

- [ ] **Step 3: Add request/response/correction types**

Add to `src-tauri/src/services/hotwords.rs`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHotwordCorrectionRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub words: Vec<WordTokenDto>,
    pub hotwords: Vec<HotwordEntry>,
    pub enabled: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordCorrection {
    pub candidate_id: String,
    pub source_text: String,
    pub target: String,
    pub start_index: usize,
    pub end_index: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHotwordCorrectionResponse {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub enabled: bool,
    pub hotwords: Vec<NormalizedHotword>,
    pub candidates: Vec<HotwordCandidate>,
    pub decisions: Vec<HotwordDecision>,
    pub corrections: Vec<HotwordCorrection>,
    pub words: Vec<WordTokenDto>,
}
```

- [ ] **Step 4: Implement synchronous safe baseline**

Add:

```rust
pub fn build_hotword_correction(
    request: BuildHotwordCorrectionRequest,
) -> BuildHotwordCorrectionResponse {
    let normalized = normalize_hotwords(&request.hotwords);
    if !request.enabled || normalized.is_empty() || request.words.is_empty() {
        return BuildHotwordCorrectionResponse {
            task_id: request.task_id,
            media_path: request.media_path,
            source_lang: request.source_lang,
            enabled: false,
            hotwords: normalized,
            candidates: Vec::new(),
            decisions: Vec::new(),
            corrections: Vec::new(),
            words: request.words,
        };
    }

    let candidates = recall_hotword_candidates(&request.words, &request.hotwords);
    if candidates.is_empty() {
        return BuildHotwordCorrectionResponse {
            task_id: request.task_id,
            media_path: request.media_path,
            source_lang: request.source_lang,
            enabled: true,
            hotwords: normalized,
            candidates,
            decisions: Vec::new(),
            corrections: Vec::new(),
            words: request.words,
        };
    }

    let llm_available = !request.translate_api_key.trim().is_empty()
        && !request.translate_base_url.trim().is_empty()
        && !request.translate_model.trim().is_empty();
    let decisions = if llm_available {
        candidates
            .iter()
            .map(|candidate| HotwordDecision {
                candidate_id: candidate.id.clone(),
                replace: false,
                target: candidate.target.clone(),
                reason: "llm_decision_not_connected_yet".to_string(),
                error: "llm_decision_not_connected_yet".to_string(),
            })
            .collect::<Vec<_>>()
    } else {
        candidates
            .iter()
            .map(|candidate| HotwordDecision {
                candidate_id: candidate.id.clone(),
                replace: false,
                target: candidate.target.clone(),
                reason: String::new(),
                error: "llm_unavailable".to_string(),
            })
            .collect::<Vec<_>>()
    };
    let words = apply_hotword_corrections(&request.words, &candidates, &decisions);
    let corrections = decisions
        .iter()
        .filter(|decision| decision.replace)
        .filter_map(|decision| {
            candidates.iter().find(|candidate| candidate.id == decision.candidate_id).map(|candidate| HotwordCorrection {
                candidate_id: candidate.id.clone(),
                source_text: candidate.source_text.clone(),
                target: decision.target.clone(),
                start_index: candidate.start_index,
                end_index: candidate.end_index,
            })
        })
        .collect::<Vec<_>>();

    BuildHotwordCorrectionResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        enabled: true,
        hotwords: normalized,
        candidates,
        decisions,
        corrections,
        words,
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p voxtrans disabled_hotwords_pass_through_words llm_unavailable_records_skipped_decision_and_keeps_words`

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add src-tauri/src/services/hotwords.rs
git commit -m "feat: add hotword correction artifact model"
```

---

### Task 4: Wire Step1.5 Into Workspace Pipeline

**Files:**
- Modify: `src-tauri/src/commands/workspace.rs`

- [ ] **Step 1: Add workspace hotword DTOs and runtime fields**

In `SettingsSnapshotInput`, add:

```rust
#[serde(default)]
hotword_groups: Option<Vec<SettingsSnapshotHotwordGroup>>,
#[serde(default)]
enable_hotwords: Option<bool>,
```

Add structs after terminology snapshot structs:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotHotwordGroup {
    #[serde(default)]
    terms: Vec<SettingsSnapshotHotwordTerm>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshotHotwordTerm {
    #[serde(default)]
    word: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    note: String,
}
```

Add to `PipelineRuntimeSettings`:

```rust
hotword_entries: Vec<crate::services::hotwords::HotwordEntry>,
enable_hotwords: bool,
```

- [ ] **Step 2: Add artifact const and pipeline step**

Add const:

```rust
const STEP_01_5_HOTWORDS_FILE: &str = "step_01_5_hotwords.json";
```

Add struct after `Step1AsrPipelineStep`:

```rust
#[derive(Debug, Clone)]
struct Step15HotwordsPipelineStep {
    task_id: String,
    media_path: String,
    source_lang: String,
    words: Vec<crate::commands::transcription::WordTokenCommandDto>,
    hotwords: Vec<crate::services::hotwords::HotwordEntry>,
    enabled: bool,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
}
```

Implement `PipelineStep`:

```rust
#[async_trait]
impl PipelineStep for Step15HotwordsPipelineStep {
    type Output = crate::services::hotwords::BuildHotwordCorrectionResponse;

    fn name(&self) -> &'static str {
        "step_01_5_hotwords"
    }

    fn artifact_file(&self) -> &'static str {
        STEP_01_5_HOTWORDS_FILE
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::ValidateThenSkip
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty() || output.media_path.trim().is_empty() || output.words.is_empty() {
            return Err("invalid step1.5 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        Ok(crate::services::hotwords::build_hotword_correction(
            crate::services::hotwords::BuildHotwordCorrectionRequest {
                task_id: self.task_id.clone(),
                media_path: self.media_path.clone(),
                source_lang: self.source_lang.clone(),
                words: self
                    .words
                    .iter()
                    .map(|word| crate::services::transcribe::WordTokenDto {
                        start: word.start,
                        end: word.end,
                        word: word.word.clone(),
                    })
                    .collect(),
                hotwords: self.hotwords.clone(),
                enabled: self.enabled,
                translate_api_key: self.translate_api_key.clone(),
                translate_base_url: self.translate_base_url.clone(),
                translate_model: self.translate_model.clone(),
            },
        ))
    }
}
```

- [ ] **Step 3: Resolve hotwords in runtime settings**

In `resolve_runtime_settings`, compute:

```rust
let enable_hotwords = snapshot_parsed
    .enable_hotwords
    .unwrap_or(saved.enable_hotwords);
let hotword_entries = if enable_hotwords {
    let snapshot_entries = snapshot_parsed
        .hotword_groups
        .unwrap_or_default()
        .into_iter()
        .flat_map(|group| group.terms.into_iter())
        .map(|term| crate::services::hotwords::HotwordEntry {
            word: term.word.trim().to_string(),
            aliases: term.aliases.into_iter().map(|alias| alias.trim().to_string()).collect(),
            lang: term.lang.trim().to_string(),
            note: term.note.trim().to_string(),
        })
        .collect::<Vec<_>>();
    snapshot_entries
        .into_iter()
        .chain(saved_hotword_entries(&saved).into_iter())
        .filter(|entry| !entry.word.trim().is_empty())
        .collect::<Vec<_>>()
} else {
    Vec::new()
};
```

Add fields to `PipelineRuntimeSettings` construction:

```rust
hotword_entries,
enable_hotwords,
```

Update `fallback_saved_settings()` with:

```rust
hotword_groups: Vec::new(),
enable_hotwords: true,
```

Add helper near `saved_terminology_entries`:

```rust
fn saved_hotword_entries(
    saved: &crate::services::preferences::SavedSettings,
) -> Vec<crate::services::hotwords::HotwordEntry> {
    saved
        .hotword_groups
        .iter()
        .flat_map(|group| group.terms.iter())
        .map(|term| crate::services::hotwords::HotwordEntry {
            word: term.word.clone(),
            aliases: term.aliases.clone(),
            lang: term.lang.clone(),
            note: term.note.clone(),
        })
        .collect()
}
```

- [ ] **Step 4: Insert Step1.5 before Step2**

In `execute_single_task`, after Step1 executes and before `Step2SegmentsPipelineStep`, add:

```rust
let step15_output = execute_step(
    &step_ctx,
    &Step15HotwordsPipelineStep {
        task_id: task_id.to_string(),
        media_path: record.item.path.clone(),
        source_lang: source_lang.to_string(),
        words: step1_output.words.clone(),
        hotwords: runtime.hotword_entries.clone(),
        enabled: runtime.enable_hotwords,
        translate_api_key: runtime.translate_api_key.clone(),
        translate_base_url: runtime.translate_base_url.clone(),
        translate_model: runtime.translate_model.clone(),
    },
)
.await?;
let step2_words = step15_output
    .words
    .iter()
    .map(|word| crate::commands::transcription::WordTokenCommandDto {
        start: word.start,
        end: word.end,
        word: word.word.clone(),
    })
    .collect::<Vec<_>>();
```

Change the Step2 construction from `words: step1_output.words.clone()` to:

```rust
words: step2_words,
```

- [ ] **Step 5: Add migration name**

In `migrate_target_artifact_name`, add:

```rust
"step_01_5_hotwords.json" => Some(STEP_01_5_HOTWORDS_FILE),
```

- [ ] **Step 6: Run backend checks**

Run: `cargo check -p voxtrans`

Expected: PASS with no warnings.

- [ ] **Step 7: Commit**

Run:

```powershell
git add src-tauri/src/commands/workspace.rs
git commit -m "feat: run hotwords before segmentation"
```

---

### Task 5: Add Frontend Hotword UI And Settings Snapshot

**Files:**
- Create: `src/app/utils/hotwords.ts`
- Create: `src/app/components/HotwordsModal.tsx`
- Modify: `src/app/App.tsx`
- Modify: `src/app/components/Navbar.tsx`
- Modify: `src/app/components/SettingsModal.tsx`
- Modify: `src/app/state/appReducer.ts`
- Modify: `src/app/hooks/useAppPersistence.ts`
- Modify: `src/app/hooks/useSettingsController.ts`
- Modify: `src/app/hooks/queue/useQueueRunner.ts`
- Modify: `src/app/hooks/queue/useQueueScheduler.ts`
- Modify: `src/app/styles/components/subtitle-settings.css`

- [ ] **Step 1: Add frontend utility**

Create `src/app/utils/hotwords.ts`:

```ts
import type { HotwordGroup, HotwordLang, HotwordTerm } from "../../features/media/types";

export const DEFAULT_HOTWORD_GROUP_NAME = "默认";

export function createHotwordGroup(name?: string): HotwordGroup {
  return {
    id: makeId("hotword-group"),
    name: (name ?? DEFAULT_HOTWORD_GROUP_NAME).trim() || DEFAULT_HOTWORD_GROUP_NAME,
    terms: [],
  };
}

export function createHotwordTerm(word: string, aliases: string[], lang: HotwordLang = "auto", note = ""): HotwordTerm {
  return {
    id: makeId("hotword"),
    word: word.trim(),
    aliases: aliases.map((alias) => alias.trim()).filter(Boolean),
    lang,
    note: note.trim(),
  };
}

export function parseInlineHotwordInput(input: string): { terms: HotwordTerm[]; skipped: number } {
  const terms: HotwordTerm[] = [];
  let skipped = 0;
  for (const rawLine of input.split(/\r?\n/).flatMap((line) => line.split(";"))) {
    const line = rawLine.trim();
    if (!line) continue;
    const [wordRaw, aliasesRaw = ""] = line.split("=");
    const word = (wordRaw ?? "").trim();
    const aliases = aliasesRaw.split(",").map((alias) => alias.trim()).filter(Boolean);
    if (!word) {
      skipped += 1;
      continue;
    }
    terms.push(createHotwordTerm(word, aliases));
  }
  return { terms, skipped };
}

export function normalizeHotwordGroups(groups: HotwordGroup[]): HotwordGroup[] {
  if (groups.length > 0) return groups;
  return [createHotwordGroup(DEFAULT_HOTWORD_GROUP_NAME)];
}

function makeId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}
```

- [ ] **Step 2: Add modal**

Create `src/app/components/HotwordsModal.tsx` by copying the structure of `TerminologyModal.tsx` and changing:

```tsx
import type { HotwordGroup, HotwordLang } from "../../features/media/types";
import { createHotwordGroup, normalizeHotwordGroups, parseInlineHotwordInput } from "../utils/hotwords";
```

Use title text `热词管理`, input placeholder `热词=错词1,错词2；如 Claude Code=cloud code,clod code`, and chip text:

```tsx
{term.word}
{term.aliases.length > 0 ? ` = ${term.aliases.join(", ")}` : ""}
{term.lang !== "auto" ? ` [${term.lang === "zh" ? "中文" : "非中文"}]` : ""}
```

Add a `<select>` beside the input for `HotwordLang` with options:

```tsx
<option value="auto">自动</option>
<option value="zh">中文</option>
<option value="non_zh">非中文</option>
```

When adding parsed terms, override parsed `lang` with the selected dropdown value.

- [ ] **Step 3: Wire App and Navbar**

In `src/app/components/Navbar.tsx`, add prop:

```ts
onOpenHotwords: () => void;
```

Render a button next to terminology:

```tsx
<button className="nav-button" onClick={onOpenHotwords}>
  <BookIcon />
  <span>热词</span>
</button>
```

In `src/app/App.tsx`, import `HotwordsModal`, add state `showHotwordsModal`, pass `onOpenHotwords={() => setShowHotwordsModal(true)}`, and render:

```tsx
<HotwordsModal
  visible={showHotwordsModal}
  groups={draftHotwordGroups}
  onClose={() => setShowHotwordsModal(false)}
  onChange={(value) => dispatch({ type: "set_draft", payload: { draftHotwordGroups: value } })}
  onSave={async (groups) => {
    await saveHotwordGroups(groups);
    setShowHotwordsModal(false);
  }}
/>
```

- [ ] **Step 4: Wire reducer and settings controller**

In `src/app/state/appReducer.ts`, import `createHotwordGroup`, add defaults:

```ts
hotwordGroups: [createHotwordGroup()],
enableHotwords: true,
```

Add draft state:

```ts
draftHotwordGroups: SavedSettings["hotwordGroups"];
draftEnableHotwords: boolean;
```

Include the draft keys in the `set_draft` payload union and `initialState`.

In `src/app/hooks/useSettingsController.ts`, mirror terminology:

```ts
draftHotwordGroups: SavedSettings["hotwordGroups"];
draftEnableHotwords: boolean;
saveHotwordGroups: async (groups: SavedSettings["hotwordGroups"]) => {
  const normalizedGroups = normalizeHotwordGroups(groups);
  const nextSettings = Object.assign({}, settings, {
    hotwordGroups: normalizedGroups,
  });
  await saveSettings(nextSettings);
  dispatch({ type: "set_settings", payload: nextSettings });
  dispatch({ type: "set_draft", payload: { draftHotwordGroups: normalizedGroups } });
};
```

Use `normalizeHotwordGroups(groups)` before saving.

- [ ] **Step 5: Wire persistence and settings modal**

In `src/app/hooks/useAppPersistence.ts`, normalize loaded hotwords:

```ts
const hotwordGroupsRaw = Array.isArray(res.settings.hotwordGroups) ? res.settings.hotwordGroups : [];
const hotwordGroups = normalizeHotwordGroups(hotwordGroupsRaw);
const enableHotwords = Boolean(res.settings.enableHotwords ?? true);
```

Include `hotwordGroups`, `enableHotwords`, `draftHotwordGroups`, and `draftEnableHotwords` in dispatched state.

In `src/app/components/SettingsModal.tsx`, add props and render a toggle near terminology:

```tsx
<label className="setting-toggle" htmlFor="enable-hotwords">
  <input
    id="enable-hotwords"
    type="checkbox"
    checked={draftEnableHotwords}
    onChange={(e) => onDraftEnableHotwordsChange(e.target.checked)}
  />
  <span>启用热词修正</span>
</label>
```

- [ ] **Step 6: Wire queue snapshots**

In both `src/app/hooks/queue/useQueueRunner.ts` and `src/app/hooks/queue/useQueueScheduler.ts`, add to `buildSettingsSnapshot`:

```ts
hotwordGroups: settings.hotwordGroups,
enableHotwords: settings.enableHotwords,
```

- [ ] **Step 7: Run frontend build**

Run: `npm run build`

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```powershell
git add src/features/media/types.ts src/app/utils/hotwords.ts src/app/components/HotwordsModal.tsx src/app/App.tsx src/app/components/Navbar.tsx src/app/components/SettingsModal.tsx src/app/state/appReducer.ts src/app/hooks/useAppPersistence.ts src/app/hooks/useSettingsController.ts src/app/hooks/queue/useQueueRunner.ts src/app/hooks/queue/useQueueScheduler.ts src/app/styles/components/subtitle-settings.css
git commit -m "feat: add hotword settings UI"
```

---

### Task 6: Connect Real LLM Hotword Decisions

**Files:**
- Modify: `src-tauri/src/services/hotwords.rs`
- Read for existing client pattern: `src-tauri/src/services/llm/client.rs`
- Read for config/context/request ids: `src-tauri/src/services/llm/port.rs`

- [ ] **Step 1: Add strict JSON parser test**

Add test:

```rust
#[test]
fn parse_hotword_decision_accepts_strict_json() {
    let decision = parse_hotword_decision_json(
        "c1",
        "Claude Code",
        r#"{"replace":true,"target":"Claude Code","reason":"product name"}"#,
    );

    assert!(decision.replace);
    assert_eq!(decision.candidate_id, "c1");
    assert_eq!(decision.target, "Claude Code");
    assert_eq!(decision.reason, "product name");
    assert!(decision.error.is_empty());
}

#[test]
fn parse_hotword_decision_rejects_invalid_json() {
    let decision = parse_hotword_decision_json("c1", "Claude Code", "yes");

    assert!(!decision.replace);
    assert_eq!(decision.error, "invalid_json");
}
```

- [ ] **Step 2: Implement parser**

Add:

```rust
#[derive(Debug, Deserialize)]
struct RawHotwordDecision {
    replace: bool,
    #[serde(default)]
    target: String,
    #[serde(default)]
    reason: String,
}

fn parse_hotword_decision_json(candidate_id: &str, fallback_target: &str, raw: &str) -> HotwordDecision {
    match serde_json::from_str::<RawHotwordDecision>(raw.trim()) {
        Ok(parsed) => HotwordDecision {
            candidate_id: candidate_id.to_string(),
            replace: parsed.replace,
            target: if parsed.target.trim().is_empty() {
                fallback_target.to_string()
            } else {
                parsed.target.trim().to_string()
            },
            reason: parsed.reason.trim().to_string(),
            error: String::new(),
        },
        Err(_) => HotwordDecision {
            candidate_id: candidate_id.to_string(),
            replace: false,
            target: fallback_target.to_string(),
            reason: String::new(),
            error: "invalid_json".to_string(),
        },
    }
}
```

- [ ] **Step 3: Replace placeholder decisions with LLM calls**

Use `OpenAiCompatLlmClient::call_json_validated` from `src-tauri/src/services/llm/client.rs` with `LlmConfig`, `LlmCallContext`, and `next_llm_request_id` from `src-tauri/src/services/llm/port.rs`. Add an async variant:

```rust
pub async fn build_hotword_correction_async(
    request: BuildHotwordCorrectionRequest,
) -> BuildHotwordCorrectionResponse
```

Keep `build_hotword_correction` as the no-network deterministic baseline used by unit tests. The async variant should:

- Run normalization and recall exactly like `build_hotword_correction`.
- If no LLM settings, return `llm_unavailable` decisions.
- For each candidate, send a prompt that says:

```rust
fn build_hotword_decision_prompt(candidate: &HotwordCandidate) -> String {
    format!(
        concat!(
            "You are judging whether one ASR phrase should be replaced by a configured hotword.\n",
            "Only decide this candidate. Do not edit grammar, punctuation, style, or surrounding text.\n",
            "Return only JSON: {{\"replace\": boolean, \"target\": string, \"reason\": string}}\n\n",
            "Context: {context}\n",
            "Candidate source text: {source_text}\n",
            "Target hotword: {target}\n",
            "Recall type: {source_kind}\n"
        ),
        context = candidate.context,
        source_text = candidate.source_text,
        target = candidate.target,
        source_kind = candidate.source_kind,
    )
}
```

- Parse with `parse_hotword_decision_json`.
- Apply corrections with `apply_hotword_corrections`.

- [ ] **Step 4: Update workspace Step1.5 to call async variant**

In `Step15HotwordsPipelineStep::run`, replace the synchronous call with this complete async call:

```rust
crate::services::hotwords::build_hotword_correction_async(
    crate::services::hotwords::BuildHotwordCorrectionRequest {
        task_id: self.task_id.clone(),
        media_path: self.media_path.clone(),
        source_lang: self.source_lang.clone(),
        words: self
            .words
            .iter()
            .map(|word| crate::services::transcribe::WordTokenDto {
                start: word.start,
                end: word.end,
                word: word.word.clone(),
            })
            .collect(),
        hotwords: self.hotwords.clone(),
        enabled: self.enabled,
        translate_api_key: self.translate_api_key.clone(),
        translate_base_url: self.translate_base_url.clone(),
        translate_model: self.translate_model.clone(),
    },
)
.await
```

- [ ] **Step 5: Run tests/checks**

Run:

```powershell
cargo test -p voxtrans hotwords
cargo check -p voxtrans
```

Expected: PASS with no warnings.

- [ ] **Step 6: Commit**

Run:

```powershell
git add src-tauri/src/services/hotwords.rs src-tauri/src/commands/workspace.rs
git commit -m "feat: judge hotword candidates with llm"
```

---

### Task 7: Final Verification

**Files:**
- No planned edits unless checks expose failures.

- [ ] **Step 1: Run Rust tests**

Run: `cargo test -p voxtrans`

Expected: PASS.

- [ ] **Step 2: Run Rust check**

Run: `cargo check -p voxtrans`

Expected: PASS with no warnings.

- [ ] **Step 3: Run frontend build**

Run: `npm run build`

Expected: PASS.

- [ ] **Step 4: Check git diff**

Run: `git diff --check`

Expected: no output.

- [ ] **Step 5: Final commit if verification required edits**

If Step 1-4 required fixes, commit the fixes:

```powershell
git add src src-tauri docs
git commit -m "fix: finalize hotword pipeline"
```

If no fixes were needed, do not create an empty commit.
