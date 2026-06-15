# Step2 VAD-Assisted Segmentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace word-gap-based pause heuristics in Step2 with fireredvad speech segments, so sentence boundaries align with acoustic speech/pause structure instead of guessed gap thresholds.

**Architecture:** VAD speech segments (already computed during ASR chunk-splitting) flow from `voxtrans-core` through `AsrAlignOutput → TranscribeResponse → Step1AsrArtifact → Step2 request`. A new `vad_align` module provides a single `crosses_speech_segment` query (with 100ms tolerance) used by both `semantic.rs` (hard split) and `subtitle_layout.rs` (rank 5). `HARD_SPLIT_GAP_MS` and `PAUSE_BONUS_GAP_MS` are deleted.

**Tech Stack:** Rust, voxtrans-core, fireredvad, serde, tauri pipeline framework.

**Spec:** `docs/specs/2026-06-15-step2-vad-assisted-segmentation-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| `voxtrans-core/src/vad.rs` (modify) | Return normalized speech segments alongside chunk segments |
| `voxtrans-core/src/lib.rs` (modify) | Carry speech segments in `PreparedAudioSegments` |
| `src-tauri/src/services/transcribe/asr_align.rs` (modify) | Carry speech segments in `AsrAlignOutput` |
| `src-tauri/src/services/transcribe.rs` (modify) | Carry speech segments in `TranscribeResponse` |
| `src-tauri/src/commands/workspace/pipeline_steps/recognition.rs` (modify) | Persist speech segments in `Step1AsrArtifact`; pass to Step2 |
| `src-tauri/src/commands/transcription.rs` (modify) | Add field to `BuildSourceSentencesCommandRequest` |
| `src-tauri/src/commands/workspace/execution_flow.rs` (modify) | Pass Step1 VAD → Step2 request |
| `src-tauri/src/services/transcription/sentence_boundary/types.rs` (modify) | Add field to `SentenceBoundaryRequest` |
| `src-tauri/src/services/transcription/sentence_boundary/mod.rs` (modify) | Pass VAD segments into semantic + subtitle_layout |
| `src-tauri/src/services/transcription/sentence_boundary/vad_align.rs` (create) | Tolerance query: does a cut point fall in VAD silence? |
| `src-tauri/src/services/transcription/sentence_boundary/semantic.rs` (modify) | Replace gap hard-split with VAD cross-segment |
| `src-tauri/src/services/transcription/sentence_boundary/subtitle_layout.rs` (modify) | Replace rank-5 gap with VAD cross-segment |

---

### Task 1: `vad_align` tolerance query module (TDD)

This is the single algorithmic primitive both call sites use. Build and test it first, in isolation, before touching any data flow.

**Files:**
- Create: `src-tauri/src/services/transcription/sentence_boundary/vad_align.rs`
- Modify: `src-tauri/src/services/transcription/sentence_boundary/mod.rs:7` (add `mod vad_align;`)

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/services/transcription/sentence_boundary/vad_align.rs`:

```rust
//! Tolerance query: does a cut point between two words fall inside a VAD
//! silence gap (i.e. the two words are in different speech segments)?
//!
//! VAD segment boundaries and forced-aligner word timestamps never align
//! exactly — VAD lags up to ~200ms (frame accumulation), aligner extends
//! word edges into silence by tens of ms. We absorb both with a tolerance
//! window applied to each silence gap before testing the cut point.

/// Tolerance (seconds) added to each end of a VAD silence gap when testing
/// whether a cut point falls inside it. 100ms covers VAD frame lag (10ms
/// steps × min_silence_frame accumulation) plus aligner jitter, without
/// being so large that adjacent words get misjudged as crossing a gap.
pub(super) const CUT_POINT_TOLERANCE_SEC: f64 = 0.100;

/// Sorted, merged speech segments `[(start_sec, end_sec)]`. Built once from
/// fireredvad output and reused for every word-pair query.
#[derive(Debug, Clone)]
pub(super) struct SpeechSegmentIndex {
    segments: Vec<(f64, f64)>,
}

impl SpeechSegmentIndex {
    pub(super) fn new(mut segments: Vec<(f64, f64)>) -> Self {
        segments.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        Self { segments }
    }

    /// Returns true when the cut point between `left_end_sec` and
    /// `right_start_sec` falls inside a VAD silence gap — i.e. the two
    /// words belong to different speech segments. Uses the midpoint of the
    /// word-pair gap as the test point (symmetric against aligner drift in
    /// either direction), with `CUT_POINT_TOLERANCE_SEC` of slack on each
    /// edge of the silence.
    ///
    /// Empty segment list => always false (no VAD data, caller degrades).
    pub(super) fn crosses_silence(
        &self,
        left_end_sec: f64,
        right_start_sec: f64,
    ) -> bool {
        if self.segments.len() < 2 {
            return false;
        }
        let cut = (left_end_sec + right_start_sec) / 2.0;
        for window in self.segments.windows(2) {
            let silence_start = window[0].1;
            let silence_end = window[1].0;
            if cut >= silence_start - CUT_POINT_TOLERANCE_SEC
                && cut <= silence_end + CUT_POINT_TOLERANCE_SEC
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> SpeechSegmentIndex {
        // Three speech segments with two silence gaps: [2.0,2.0]-[3.0,3.0]
        // gap is 1.0s wide; [5.0,5.0]-[6.0,6.0] gap is 1.0s wide.
        SpeechSegmentIndex::new(vec![
            (0.0, 2.0),
            (3.0, 5.0),
            (6.0, 8.0),
        ])
    }

    #[test]
    fn cut_at_silence_midpoint_crosses() {
        let i = idx();
        // Words span the [2.0, 3.0] gap; midpoint = 2.5.
        assert!(i.crosses_silence(2.2, 2.8));
    }

    #[test]
    fn cut_inside_a_speech_segment_does_not_cross() {
        let i = idx();
        // Both words inside [3.0, 5.0].
        assert!(!i.crosses_silence(3.5, 4.0));
    }

    #[test]
    fn cut_within_tolerance_of_silence_edge_crosses() {
        let i = idx();
        // Aligner extended left word end into the silence by 80ms; the
        // midpoint (2.05) sits 50ms inside the [2.0,3.0] gap start, well
        // within tolerance.
        assert!(i.crosses_silence(2.18, 1.92));
    }

    #[test]
    fn cut_beyond_tolerance_does_not_cross() {
        let i = idx();
        // Midpoint 1.85 is 150ms before the gap start (2.0); outside the
        // 100ms tolerance window on that side.
        assert!(!i.crosses_silence(1.9, 1.8));
    }

    #[test]
    fn empty_segments_never_cross() {
        let i = SpeechSegmentIndex::new(vec![]);
        assert!(!i.crosses_silence(1.0, 5.0));
    }

    #[test]
    fn single_segment_never_cross() {
        let i = SpeechSegmentIndex::new(vec![(0.0, 10.0)]);
        assert!(!i.crosses_silence(3.0, 4.0));
    }

    #[test]
    fn second_gap_also_detected() {
        let i = idx();
        // Words span the [5.0, 6.0] gap.
        assert!(i.crosses_silence(5.1, 5.9));
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/services/transcription/sentence_boundary/mod.rs`, add `mod vad_align;` to the existing module list (after `mod text;` on line 12).

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri\Cargo.toml --features cuda --lib vad_align`
Expected: 7 passed; 0 failed.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/transcription/sentence_boundary/vad_align.rs src-tauri/src/services/transcription/sentence_boundary/mod.rs
git commit -m "feat(step2): add vad_align tolerance query module"
```

---

### Task 2: Plumb speech segments out of `voxtrans-core`

Thread fireredvad's normalized speech segments from `build_segments_from_vad` through `PreparedAudioSegments`. This is the source of truth — everything downstream reads from here.

**Files:**
- Modify: `voxtrans-core/src/vad.rs`
- Modify: `voxtrans-core/src/lib.rs`

- [ ] **Step 1: Change `build_segments_from_vad` return type to include speech segments**

In `voxtrans-core/src/vad.rs`, change the signature and all three return sites. The function currently returns `(Vec<AudioSegment>, f64)`; change to `(Vec<AudioSegment>, f64, Vec<(f64, f64)>)` where the third element is the normalized speech ranges (already computed as `speech_ranges` at line 45).

Change the signature at line 19:

```rust
pub(crate) fn build_segments_from_vad(
    audio_path: &Path,
    total_duration_sec: f64,
    chunk_target_seconds: f64,
) -> Result<(Vec<AudioSegment>, f64, Vec<(f64, f64)>), Box<dyn std::error::Error>> {
```

In the early-return branch (short audio, ~line 34-42), return the single full-span segment as speech segments:

```rust
    if effective_total_duration <= chunk_target_seconds {
        return Ok((
            vec![AudioSegment {
                index: 0,
                start_sec: 0.0,
                end_sec: effective_total_duration,
            }],
            vad_elapsed_sec,
            vec![(0.0, effective_total_duration)],
        ));
    }
```

At the final `Ok(...)` (~line 82), add `speech_ranges.clone()` as the third element:

```rust
    Ok((segments, vad_elapsed_sec, speech_ranges))
```

- [ ] **Step 2: Carry speech segments in `PreparedAudioSegments`**

In `voxtrans-core/src/lib.rs`, add a field to `PreparedAudioSegments` (line 23):

```rust
#[derive(Debug, Clone)]
pub struct PreparedAudioSegments {
    pub mono_samples: Vec<f32>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub vad_speech_segments: Vec<(f64, f64)>,
    pub segment_summaries: Vec<SegmentSummary>,
}
```

Update `prepare_audio_segments_for_asr` (~line 36) to capture and pass the third return value:

```rust
    let (segments, vad_elapsed_sec, vad_speech_segments) = build_segments_from_vad(
        &prepared_audio.vad_wav.path,
        audio_duration_sec,
        chunk_target_seconds,
    )?;
```

And in the final `Ok(...)` (~line 51):

```rust
    Ok(PreparedAudioSegments {
        mono_samples: prepared_audio.mono_samples,
        audio_duration_sec,
        vad_elapsed_sec,
        vad_speech_segments,
        segment_summaries,
    })
```

- [ ] **Step 3: Compile-check voxtrans-core**

Run: `cargo check -p voxtrans-core`
Expected: compiles with no errors (callers of `build_segments_from_vad` and `PreparedAudioSegments` are within this crate and already updated).

- [ ] **Step 4: Commit**

```bash
git add voxtrans-core/src/vad.rs voxtrans-core/src/lib.rs
git commit -m "feat(core): expose normalized VAD speech segments from prepare_audio_segments_for_asr"
```

---

### Task 3: Plumb speech segments through `AsrAlignOutput` and `TranscribeResponse`

**Files:**
- Modify: `src-tauri/src/services/transcribe/asr_align.rs`
- Modify: `src-tauri/src/services/transcribe.rs`

- [ ] **Step 1: Add field to `AsrAlignOutput`**

In `src-tauri/src/services/transcribe/asr_align.rs`, add to the struct (line 27):

```rust
pub(super) struct AsrAlignOutput {
    pub(super) words: Vec<WordToken>,
    pub(super) text: String,
    pub(super) aligned_text: String,
    pub(super) segment_summaries: Vec<voxtrans_core::SegmentSummary>,
    pub(super) audio_duration_sec: f64,
    pub(super) vad_elapsed_sec: f64,
    pub(super) vad_speech_segments: Vec<(f64, f64)>,
    pub(super) transcribe_elapsed_sec: f64,
    pub(super) timing: AsrAlignTiming,
    pub(super) execution_provider: String,
    pub(super) new_asr_results: Vec<(usize, String)>,
}
```

In the final `Ok(AsrAlignOutput { ... })` (~line 219), read from `prepared` and add:

```rust
    Ok(AsrAlignOutput {
        words,
        text: transcript_text,
        aligned_text,
        segment_summaries: prepared.segment_summaries,
        audio_duration_sec: prepared.audio_duration_sec,
        vad_elapsed_sec: prepared.vad_elapsed_sec,
        vad_speech_segments: prepared.vad_speech_segments,
        transcribe_elapsed_sec: timing.total_elapsed_sec,
        timing,
        execution_provider: device.label,
        new_asr_results: segment_transcripts.new_results,
    })
```

- [ ] **Step 2: Add field to `TranscribeResponse` and populate**

In `src-tauri/src/services/transcribe.rs`, add to `TranscribeResponse` (line 41):

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeResponse {
    pub words: Vec<WordTokenDto>,
    pub text: String,
    pub aligned_text: String,
    pub segment_total: usize,
    pub segment_durations_sec: Vec<f64>,
    pub audio_duration_sec: f64,
    pub vad_elapsed_sec: f64,
    pub vad_speech_segments: Vec<(f64, f64)>,
    pub transcribe_elapsed_sec: f64,
    pub timing_sec: TranscribeTimingSecDto,
    pub rtf_x: f64,
    pub rtf_breakdown_x: TranscribeRtfBreakdownDto,
    pub execution_provider: String,
    pub new_asr_segments: Vec<(usize, String)>,
}
```

In the `response = TranscribeResponse { ... }` block (~line 167), add:

```rust
        vad_speech_segments: output.vad_speech_segments,
```

- [ ] **Step 3: Compile-check the service layer**

Run: `cargo check --manifest-path src-tauri\Cargo.toml --features cuda`
Expected: errors only at `Step1AsrArtifact` / Step1 pipeline construction (not yet updated) — that's the next task. If errors appear elsewhere, fix them.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/transcribe/asr_align.rs src-tauri/src/services/transcribe.rs
git commit -m "feat(transcribe): carry VAD speech segments through AsrAlignOutput and TranscribeResponse"
```

---

### Task 4: Persist speech segments in Step1 artifact and pass to Step2

**Files:**
- Modify: `src-tauri/src/commands/workspace/pipeline_steps/recognition.rs`
- Modify: `src-tauri/src/commands/transcription.rs`
- Modify: `src-tauri/src/commands/workspace/execution_flow.rs`

- [ ] **Step 1: Add field to `Step1AsrArtifact`**

In `src-tauri/src/commands/workspace/pipeline_steps/recognition.rs`, add a serde-defaulted field to `Step1AsrArtifact` (line 13) — the `default` ensures old artifacts deserialize cleanly as an empty Vec:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::commands::workspace) struct Step1AsrArtifact {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    #[serde(default)]
    pub(in crate::commands::workspace) text: String,
    #[serde(default)]
    pub(in crate::commands::workspace) aligned_text: String,
    pub(in crate::commands::workspace) words:
        Vec<crate::commands::transcription::WordTokenCommandDto>,
    #[serde(default)]
    pub(in crate::commands::workspace) vad_speech_segments: Vec<(f64, f64)>,
}
```

- [ ] **Step 2: Populate the field in Step1's `run`**

In `recognition.rs`, find where `Step1AsrArtifact` is constructed (~line 180). The current code builds it from a `TranscribeResponse`-shaped value. Add `vad_speech_segments` from that response.

```rust
        Ok(Step1AsrArtifact {
            task_id: self.task_id.clone(),
            media_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            text: transcribe_response.text,
            aligned_text: transcribe_response.aligned_text,
            words,
            vad_speech_segments: transcribe_response.vad_speech_segments,
        })
```

- [ ] **Step 3: Add field to `BuildSourceSentencesCommandRequest` and `SentenceBoundaryRequest`**

In `src-tauri/src/commands/transcription.rs`, find `BuildSourceSentencesCommandRequest` (this struct is defined in `commands/transcription_types.rs` or inline — locate it via the `Step2SegmentsPipelineStep::run` usage at `recognition.rs:222`). Add a field:

```rust
    #[serde(default)]
    pub vad_speech_segments: Vec<(f64, f64)>,
```

In `src-tauri/src/services/transcription/sentence_boundary/types.rs`, add to `SentenceBoundaryRequest` (line 7):

```rust
#[derive(Debug, Clone)]
pub struct SentenceBoundaryRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub subtitle_length_preset: String,
    pub use_subtitle_layout_split: bool,
    pub words: Vec<WordTokenDto>,
    pub vad_speech_segments: Vec<(f64, f64)>,
}
```

- [ ] **Step 4: Wire the field through Step2's pipeline step**

In `recognition.rs`, `Step2SegmentsPipelineStep` (line 192) needs a new field, and its `run` (line 221) must pass it into the request:

Add field to struct:

```rust
pub(in crate::commands::workspace) struct Step2SegmentsPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) subtitle_length_preset: String,
    pub(in crate::commands::workspace) use_subtitle_layout_split: bool,
    pub(in crate::commands::workspace) words:
        Vec<crate::commands::transcription::WordTokenCommandDto>,
    pub(in crate::commands::workspace) vad_speech_segments: Vec<(f64, f64)>,
}
```

In `run` (~line 222), add to the request:

```rust
        let step2_request = crate::commands::transcription::BuildSourceSentencesCommandRequest {
            task_id: self.task_id.clone(),
            audio_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            subtitle_length_preset: self.subtitle_length_preset.clone(),
            use_subtitle_layout_split: self.use_subtitle_layout_split,
            words: self.words.clone(),
            vad_speech_segments: self.vad_speech_segments.clone(),
        };
```

- [ ] **Step 5: Pass Step1 VAD → Step2 in `execution_flow.rs`**

In `src-tauri/src/commands/workspace/execution_flow.rs`, the `Step2SegmentsPipelineStep` is constructed (~line 102). Add the VAD field from the Step1 artifact (which is available as `step1_exec.output` earlier in the function):

```rust
    let step2_exec = execute_workspace_step(
        &Step2SegmentsPipelineStep {
            task_id: task_id.to_string(),
            media_path: record.item.path.clone(),
            source_lang: source_lang.clone(),
            subtitle_length_preset: runtime.subtitle_length_preset.clone(),
            use_subtitle_layout_split: true,
            words: step2_words,
            vad_speech_segments: step1_exec.output.vad_speech_segments.clone(),
        },
        &step_context,
        store,
    )
    .await?;
```

- [ ] **Step 6: Wire `SentenceBoundaryRequest` in `commands/transcription.rs`**

In `build_source_sentences_with_progress` (`src-tauri/src/commands/transcription.rs:17`), the `SentenceBoundaryRequest` is constructed (~line 23). Add the VAD field:

```rust
    let step2 = crate::services::transcription::build_source_sentences_from_words_with_progress(
        crate::services::transcription::SentenceBoundaryRequest {
            task_id: request.task_id,
            media_path: request.audio_path,
            source_lang: request.source_lang,
            subtitle_length_preset: request.subtitle_length_preset,
            use_subtitle_layout_split: request.use_subtitle_layout_split,
            words: request.words.into_iter().map(to_service_word).collect(),
            vad_speech_segments: request.vad_speech_segments,
        },
        on_progress,
    )
    .await?;
```

- [ ] **Step 7: Compile-check**

Run: `cargo check --manifest-path src-tauri\Cargo.toml --features cuda`
Expected: compiles with no errors. The `build_source_sentences_from_words_with_progress` signature already takes `SentenceBoundaryRequest`, so adding a field is internal. If errors appear at `SentenceBoundaryRequest` construction in tests, add `vad_speech_segments: Vec::new()` to those test sites.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands/workspace/pipeline_steps/recognition.rs src-tauri/src/commands/transcription.rs src-tauri/src/commands/workspace/execution_flow.rs src-tauri/src/services/transcription/sentence_boundary/types.rs
git commit -m "feat(step2): plumb VAD speech segments from Step1 artifact into Step2 request"
```

---

### Task 5: Use VAD in `semantic.rs` hard-split (replaces `HARD_SPLIT_GAP_MS`)

**Files:**
- Modify: `src-tauri/src/services/transcription/sentence_boundary/semantic.rs`
- Modify: `src-tauri/src/services/transcription/sentence_boundary/mod.rs`

- [ ] **Step 1: Update `build_source_sentences_from_words_with_progress` to build a `SpeechSegmentIndex` and pass it down**

In `src-tauri/src/services/transcription/sentence_boundary/mod.rs` (~line 32), after `normalized_words` is built (~line 42), construct the index from the request:

```rust
    let vad_index = vad_align::SpeechSegmentIndex::new(request.vad_speech_segments.clone());
```

Pass `&vad_index` into `build_split_points_from_hard_boundaries` (used at ~line 52):

```rust
    let hard_split_points = build_split_points_from_hard_boundaries(&normalized_words, &vad_index);
```

Also update the `#[cfg(test)]` helper at ~line 114 that calls `build_deterministic_split_points` — it must accept and forward the index (or pass an empty one for tests that don't care about VAD).

- [ ] **Step 2: Replace gap-based HardPause with VAD cross-segment**

In `src-tauri/src/services/transcription/sentence_boundary/semantic.rs`:

Delete the `use super::HARD_SPLIT_GAP_MS;` import. The `use super::timing::gap_ms;` import is also no longer needed in this file (it was only used for the HardPause gap check).

Change `build_split_points_from_hard_boundaries` signature and the `build_high_priority_split_points` internals:

```rust
use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::should_split_after_terminal_token;

use super::types::SplitReason;
use super::vad_align::SpeechSegmentIndex;

pub(super) fn build_split_points_from_hard_boundaries(
    words: &[WordTokenDto],
    vad_index: &SpeechSegmentIndex,
) -> Vec<(usize, SplitReason)> {
    build_high_priority_split_points(words, vad_index)
}

#[cfg(test)]
pub(super) fn build_deterministic_split_points(
    words: &[WordTokenDto],
    vad_index: &SpeechSegmentIndex,
) -> Vec<(usize, SplitReason)> {
    build_high_priority_split_points(words, vad_index)
}

fn build_high_priority_split_points(
    words: &[WordTokenDto],
    vad_index: &SpeechSegmentIndex,
) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    for index in 0..words.len() {
        let next_word = words.get(index + 1).map(|word| word.word.as_str());
        let high_priority_reason =
            if should_split_after_terminal_token(&words[index].word, next_word) {
                Some(SplitReason::TerminalPunctuation)
            } else if index + 1 < words.len()
                && vad_index.crosses_silence(words[index].end, words[index + 1].start)
            {
                Some(SplitReason::HardPause)
            } else {
                None
            };

        if let Some(reason) = high_priority_reason {
            push_split_point(&mut out, index, reason);
        }
    }
    out
}
```

- [ ] **Step 3: Delete `HARD_SPLIT_GAP_MS`**

In `src-tauri/src/services/transcription/sentence_boundary/mod.rs`, delete line 30:

```rust
const HARD_SPLIT_GAP_MS: u64 = 2_000;
```

Grep for any remaining `HARD_SPLIT_GAP_MS` references and remove them (the only other user is `assembly.rs::build_micro_chunks` which sets `hard_split_before`/`hard_split_after` booleans — see Task 7).

- [ ] **Step 4: Fix test helpers**

In `src-tauri/src/services/transcription/sentence_boundary/mod.rs`, the `#[cfg(test)] fn build_deterministic_sentence_spans` (~line 108) and `build_deterministic_split_points` (~line 114) now need a `vad_index` parameter. Update their signatures and all call sites in `tests.rs` to pass `&SpeechSegmentIndex::new(vec![])` (empty = no VAD in those tests).

- [ ] **Step 5: Compile-check + run Step2 tests**

Run: `cargo test --manifest-path src-tauri\Cargo.toml --features cuda --lib sentence_boundary`
Expected: all existing tests pass (they use empty VAD index, so behavior is unchanged — only the gap hard-split path is gone, which those tests didn't rely on because they used punctuation).

Run: `cargo check --manifest-path src-tauri\Cargo.toml --features cuda`
Expected: compiles clean (assembly.rs will have a dangling `HARD_SPLIT_GAP_MS` reference — fixed in Task 7).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/transcription/sentence_boundary/semantic.rs src-tauri/src/services/transcription/sentence_boundary/mod.rs src-tauri/src/services/transcription/sentence_boundary/tests.rs
git commit -m "feat(step2): replace HARD_SPLIT_GAP_MS with VAD cross-segment in semantic hard-split"
```

---

### Task 6: Use VAD in `subtitle_layout.rs` rank 5 (replaces `PAUSE_BONUS_GAP_MS`)

**Files:**
- Modify: `src-tauri/src/services/transcription/sentence_boundary/subtitle_layout.rs`
- Modify: `src-tauri/src/services/transcription/sentence_boundary/mod.rs`

- [ ] **Step 1: Thread `SpeechSegmentIndex` into `build_subtitle_layout_split_points`**

In `subtitle_layout.rs`, change `build_subtitle_layout_split_points` signature (~line 31) to accept `&SpeechSegmentIndex`, and thread it through `greedy_split_span` → `find_best_greedy_cut` → `boundary_rank`. All four functions gain a `vad_index: &SpeechSegmentIndex` parameter.

```rust
use super::vad_align::SpeechSegmentIndex;

pub(super) fn build_subtitle_layout_split_points(
    words: &[WordTokenDto],
    semantic_spans: &[(usize, usize)],
    source_lang: &str,
    subtitle_length_preset: &str,
    vad_index: &SpeechSegmentIndex,
) -> Vec<(usize, SplitReason)> {
    // ... body unchanged except passing vad_index into greedy_split_span
}
```

- [ ] **Step 2: Replace rank-5 gap check with VAD cross-segment**

In `boundary_rank` (~line 212), change signature to accept `vad_index: &SpeechSegmentIndex`, and replace the rank-5 block:

Delete at top of file:

```rust
const PAUSE_BONUS_GAP_MS: u64 = 350;
```

Replace the rank-5 branch (~line 244):

```rust
    // Rank 5: cut point falls in a VAD silence (cross speech segment).
    if vad_index.crosses_silence(left.end, right.start) {
        return 5;
    }
```

Also delete the now-unused `use super::timing::gap_ms;` import at the top of `subtitle_layout.rs` (verify `gap_ms` has no other callers in this file before deleting).

- [ ] **Step 3: Update the call site in `mod.rs`**

In `mod.rs` (~line 57), pass `&vad_index`:

```rust
            build_subtitle_layout_split_points(
                &normalized_words,
                &semantic_spans,
                &request.source_lang,
                &request.subtitle_length_preset,
                &vad_index,
            ),
```

- [ ] **Step 4: Compile-check**

Run: `cargo check --manifest-path src-tauri\Cargo.toml --features cuda`
Expected: compiles clean. The `gap_ms` function in `timing.rs` may now be unused — leave it for now (assembly.rs still uses it).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/transcription/sentence_boundary/subtitle_layout.rs src-tauri/src/services/transcription/sentence_boundary/mod.rs
git commit -m "feat(step2): replace PAUSE_BONUS_GAP_MS with VAD cross-segment in boundary_rank"
```

---

### Task 7: Update `assembly.rs` micro-chunk hard-split flags

`build_micro_chunks` in `assembly.rs` currently sets `hard_split_before`/`hard_split_after` booleans using `HARD_SPLIT_GAP_MS`. These flags feed into `BoundaryDecision` (reported to the UI). Replace with VAD cross-segment.

**Files:**
- Modify: `src-tauri/src/services/transcription/sentence_boundary/assembly.rs`
- Modify: `src-tauri/src/services/transcription/sentence_boundary/mod.rs`

- [ ] **Step 1: Pass `vad_index` into `build_micro_chunks`**

In `mod.rs` (~line 47), update the call:

```rust
    let micro_chunks = build_micro_chunks(&normalized_words, &vad_index);
```

- [ ] **Step 2: Replace gap check in `build_micro_chunks`**

In `assembly.rs`, change `build_micro_chunks` signature (~line 26) to accept `vad_index: &SpeechSegmentIndex`. Keep `gap_before_ms`/`gap_after_ms` as-is (they're informational UI fields), but switch the `hard_split_before`/`hard_split_after` booleans from the gap-threshold to VAD cross-segment:

```rust
use super::vad_align::SpeechSegmentIndex;

pub(super) fn build_micro_chunks(
    words: &[WordTokenDto],
    vad_index: &SpeechSegmentIndex,
) -> Vec<MicroChunk> {
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let gap_before_ms = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| gap_ms(prev.end, word.start))
                .unwrap_or(0);
            let gap_after_ms = words
                .get(index + 1)
                .map(|next| gap_ms(word.end, next.start))
                .unwrap_or(0);
            let hard_split_before = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| vad_index.crosses_silence(prev.end, word.start))
                .unwrap_or(false);
            let hard_split_after = words
                .get(index + 1)
                .map(|next| vad_index.crosses_silence(word.end, next.start))
                .unwrap_or(false);
            MicroChunk {
                chunk_id: index + 1,
                start_ms: seconds_to_ms(word.start),
                end_ms: seconds_to_ms(word.end.max(word.start)),
                text: word.word.clone(),
                word_start: index,
                word_end: index,
                gap_before_ms,
                gap_after_ms,
                hard_split_before,
                hard_split_after,
            }
        })
        .collect()
}
```

Delete the `use super::HARD_SPLIT_GAP_MS;` import at the top of `assembly.rs`.

- [ ] **Step 3: Compile + run full sentence_boundary test suite**

Run: `cargo check --manifest-path src-tauri\Cargo.toml --features cuda`
Expected: compiles clean. No remaining `HARD_SPLIT_GAP_MS` references anywhere.

Run: `cargo test --manifest-path src-tauri\Cargo.toml --features cuda --lib sentence_boundary`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/transcription/sentence_boundary/assembly.rs src-tauri/src/services/transcription/sentence_boundary/mod.rs
git commit -m "feat(step2): use VAD cross-segment for micro-chunk hard-split flags"
```

---

### Task 8: Ablation test — punctuation-stripped segmentation with/without VAD

This is the core proof that VAD adds positive value. It directly implements the verification from the spec: strip punctuation from words, compare segmentation quality with and without VAD.

**Files:**
- Create test cases in: `src-tauri/src/services/transcription/sentence_boundary/tests.rs`

- [ ] **Step 1: Write the ablation test**

Add to `src-tauri/src/services/transcription/sentence_boundary/tests.rs`:

```rust
#[test]
fn vad_sustains_segmentation_when_punctuation_stripped() {
    // Words from a coherent sentence with natural pauses. Punctuation
    // version has a period after "world"; stripped version has none.
    // VAD segments place a silence gap between "world" and "Again".
    use crate::services::transcribe::WordTokenDto;

    fn word(start: f64, end: f64, text: &str) -> WordTokenDto {
        WordTokenDto { start, end, word: text.to_string() }
    }

    let words_with_punct = vec![
        word(0.6, 1.0, "Hello"),
        word(1.1, 1.5, "world."),
        // silence gap 1.5 -> 4.2 (VAD segments split here)
        word(4.2, 4.8, "Again"),
        word(4.9, 5.3, "today"),
        word(5.4, 5.8, "is"),
        word(5.9, 6.5, "great."),
    ];
    let words_stripped: Vec<WordTokenDto> = words_with_punct
        .iter()
        .map(|w| WordTokenDto {
            start: w.start,
            end: w.end,
            word: w.word.trim_end_matches(['.', ',', '!', '?']).to_string(),
        })
        .collect();

    let vad_segments = vec![
        (0.0, 1.6),   // "Hello world"
        (4.1, 6.6),   // "Again today is great"
    ];

    // With punctuation + VAD: splits at "world." (terminal punct).
    let idx_vad = SpeechSegmentIndex::new(vad_segments.clone());
    let splits_punct = build_split_points_from_hard_boundaries(&words_with_punct, &idx_vad);
    assert!(splits_punct.iter().any(|(i, _)| *i == 1), "punct should split after 'world'");

    // Stripped, NO VAD: no terminal punct, no long gap in word timestamps
    // (gap 1.5->4.2 is large but we're testing the VAD path; without VAD
    // index the hard-split has no signal). Expect NO hard split.
    let idx_empty = SpeechSegmentIndex::new(vec![]);
    let splits_no_vad = build_split_points_from_hard_boundaries(&words_stripped, &idx_empty);
    assert!(splits_no_vad.is_empty(), "without VAD or punct, no hard split");

    // Stripped, WITH VAD: the silence gap is detected, splits after "world".
    let splits_vad = build_split_points_from_hard_boundaries(&words_stripped, &idx_vad);
    assert!(
        splits_vad.iter().any(|(i, r)| *i == 1 && *r == SplitReason::HardPause),
        "VAD should split after 'world' even without punctuation"
    );
}
```

Add the necessary imports at the top of `tests.rs`:

```rust
use super::vad_align::SpeechSegmentIndex;
use super::semantic::build_split_points_from_hard_boundaries;
use super::types::SplitReason;
```

- [ ] **Step 2: Run the test**

Run: `cargo test --manifest-path src-tauri\Cargo.toml --features cuda --lib vad_sustains_segmentation`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/transcription/sentence_boundary/tests.rs
git commit -m "test(step2): ablation test proving VAD sustains segmentation without punctuation"
```

---

### Task 9: Full compile, test sweep, and cleanup

- [ ] **Step 1: Full workspace compile**

Run: `cargo check --manifest-path src-tauri\Cargo.toml --features cuda`
Expected: zero errors, zero warnings.

- [ ] **Step 2: Full test sweep**

Run: `cargo test --manifest-path src-tauri\Cargo.toml --features cuda --lib`
Expected: all tests pass.

- [ ] **Step 3: Grep for orphaned constants**

Run: `findstr /s /i "HARD_SPLIT_GAP_MS PAUSE_BONUS_GAP_MS" src-tauri\src\*.rs`
Expected: no matches (both deleted).

- [ ] **Step 4: Commit any final cleanup**

```bash
git add -A
git commit -m "chore(step2): final cleanup after VAD integration"
```

---

## Notes for the implementer

- **Backward compatibility:** `Step1AsrArtifact.vad_speech_segments` uses `#[serde(default)]`, so existing checkpoints load as empty Vec. Step2 degrades cleanly to punctuation + length budget. No migration needed.
- **To activate VAD on existing tasks:** re-run Step1 (the artifact gains the VAD field). Step2 then auto-uses it.
- **`gap_ms` in `timing.rs`:** still used by `assembly.rs` for informational `gap_before_ms`/`gap_after_ms` fields reported to the UI. Do NOT delete `timing.rs` or `gap_ms`.
- **The `gap_ms` import in `semantic.rs`:** was only used for `HARD_SPLIT_GAP_MS` comparison. After Task 5, remove it from `semantic.rs` if the compiler flags it as unused.
