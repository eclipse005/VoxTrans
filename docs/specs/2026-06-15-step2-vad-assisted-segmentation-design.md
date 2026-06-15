# Step2 接入 VAD 说话片段辅助断句

## 目标

把 fireredvad 的原始说话片段（speech segments）引入 Step2 断句，用声学层的直接信号取代当前基于词时间戳 gap 的间接停顿判据，提升断句在以下场景的质量：

- ASR 漏词（咳嗽、犹豫"嗯啊"、背景噪音）导致词 gap 看不见真实静音
- 无标点的连贯语流（教学/演讲）——标点缺失时靠声学结构兜底
- 说话人停顿但 ASR 把静音归给了相邻词——gap 被压缩

## 核心设计

### 判定逻辑统一

Step2 里凡是要判断"word[i] 和 word[i+1] 之间有没有停顿"的地方，统一查一个问题：

> word[i] 和 word[i+1] 是否落在不同的 VAD 说话片段里？

- 跨说话片段 = 有停顿（声学层判定）
- 同片段内 = 无停顿

**词 gap 时长判据从 Step2 彻底消失**——不再用 `gap_ms(...) >= 阈值` 做任何判断。词时间戳只用于时间映射（word→所属说话片段），不做时长判断。

### 两处改造点

| 位置 | 当前 | 改造后 |
|------|------|--------|
| `semantic.rs` 硬切点 | `gap_ms >= 2000ms` → `HardPause` | 跨说话片段 → `HardPause` |
| `subtitle_layout.rs` rank 5 | `gap_ms >= 350ms` → rank 5 | 切点落在 VAD 静音（跨片段）→ rank 5 |

两处用同一个 VAD 信号、同一个"跨片段"判定，只是触发不同的级别。`HARD_SPLIT_GAP_MS` 和 `PAUSE_BONUS_GAP_MS` 常量退役。

## 数据流改造

VAD 说话片段当前在 `voxtrans-core` 的 ASR 准备阶段算出，只用于 chunk 切分后被丢弃。改造后沿 pipeline 传到 Step2。

```
voxtrans_core::prepare_audio_segments_for_asr
  └─ build_segments_from_vad
      └─ vad.timestamps  ← 原始说话片段 [(f32, f32)]
          ├─ 用于 chunk 切分（不变）
          └─ 新增：传出
              ▼
AsrAlignOutput.vad_speech_segments  ← 新增字段
              ▼
TranscribeResponse.vad_speech_segments  ← 新增字段
              ▼
Step1AsrArtifact.vad_speech_segments  ← 新增字段（持久化到 task_artifacts）
              ▼
SentenceBoundaryRequest.vad_speech_segments  ← 新增字段
              ▼
Step2 内部：
  1. 构建"切点→是否跨片段"查询表
  2. semantic.rs 查表判硬切
  3. subtitle_layout.rs boundary_rank 查表判 rank
```

### VAD 片段的数据结构

传出去的是 `normalize_ranges` 处理后的说话片段（已排序、合并重叠、钳制到总时长），而非原始 `vad.timestamps`——避免脏数据（重叠、越界）流入 Step2。转 f64 以匹配词时间戳精度：

```rust
// 在 PreparedAudioSegments / AsrAlignOutput / Step1AsrArtifact /
// SentenceBoundaryRequest 里统一加这个字段：
pub vad_speech_segments: Vec<(f64, f64)>,  // [(start_sec, end_sec)]
```

`build_segments_from_vad` 返回值从 `(Vec<AudioSegment>, f64)` 改为 `(Vec<AudioSegment>, f64, Vec<(f64, f64)>)`——第三项是 normalized 后的说话片段。两个 early-return 路径（短音频、无 split）也要返回对应片段。

### 容错：VAD 缺失时降级

VAD 说话片段字段为可选（`Vec` 为空 = 无 VAD 数据）。当为空时（旧 artifact、或 VAD 失败），Step2 退化到纯标点 + 连词 + 字幕长度预算，不崩。这保证了已有 checkpoint 的向后兼容。

## 时间对齐与容差

VAD 片段边界和 forced aligner 词时间戳不可能完全一致（两者误差来源不同），判定逻辑必须带容差。

### 误差来源

- **VAD**：帧级（10ms/帧）判"有没有人声"，边界受 `min_silence_frame`（200ms）影响，片段边界可能滞后真实静音起点最多 ~200ms
- **Forced aligner**：词级对齐，会把静音归给相邻词（word.end 延伸进静音），典型抖动 ±几十毫秒

### 判定算法

在 `boundary_rank(words, i)` 或硬切判定时，判断切点 `t`（取 `words[i].end` 和 `words[i+1].start` 的中点）是否落在 VAD 静音区间内：

```
对每对相邻说话片段 [seg_k.end, seg_{k+1}.start]（即静音区间）：
  查询区间 = [seg_k.end - TOLERANCE, seg_{k+1}.start + TOLERANCE]
  若 t ∈ 查询区间 → 切点落在静音（跨片段）
```

- **`TOLERANCE = 100ms`**（VAD 帧 step 的 10 倍裕度，吸收 VAD 边界滞后 + aligner 抖动，又不至于把相邻词误判为跨片段）

### 切点位置取值

切点用 **`t = (words[i].end + words[i+1].start) / 2`**（两词时间戳的中点），而非 `words[i].end`。原因：aligner 可能把静音归给前一个词（end 延伸）或后一个词（start 提前），取中点对这两种偏差都更对称。

## boundary_rank 改造

当前 rank 体系（`subtitle_layout.rs:212`）：

| rank | 含义 | 改造 |
|------|------|------|
| 1 | 句末标点 `. ! ? 。` | 不变 |
| 2 | 软子句标点 `; : ，` | 不变 |
| 3 | 逗号 `,` | 不变 |
| 4 | 连词前 `and/but/so...` | 不变 |
| 5 | ~~词 gap ≥ 350ms~~ | **改为：切点落在 VAD 静音（跨片段，带容差）** |
| 6 | 普通词界 | 不变 |
| 9 | 禁切（括号/数字内部） | 不变 |

rank 5 从"词 gap ≥ 350ms"改成"VAD 跨片段"。标点（rank 1-3）和连词（rank 4）保持优先，VAD 是补充信号——有标点的地方标点说了算，没标点的地方 VAD 兜底。

## semantic.rs 硬切点改造

当前：

```rust
// semantic.rs
const HARD_SPLIT_GAP_MS: u64 = 2_000;

fn build_high_priority_split_points(words) {
    for index in 0..words.len() {
        let reason = if should_split_after_terminal_token(...) {
            TerminalPunctuation
        } else if gap_ms(words[i].end, words[i+1].start) >= HARD_SPLIT_GAP_MS {
            HardPause  // ← 这个分支换成 VAD
        };
    }
}
```

改造后：

```rust
fn build_high_priority_split_points(words, vad_speech_segments) {
    for index in 0..words.len() {
        let reason = if should_split_after_terminal_token(...) {
            TerminalPunctuation
        } else if crosses_vad_segment(words, index, vad_speech_segments) {
            HardPause  // ← VAD 跨片段判定，取代 gap >= 2000ms
        };
    }
}
```

`crosses_vad_segment` = word[index] 和 word[index+1] 落在不同说话片段（带 100ms 容差）。`HARD_SPLIT_GAP_MS` 常量删除。

## 改动文件清单

| 文件 | 改动 |
|------|------|
| `voxtrans-core/src/lib.rs` | `PreparedAudioSegments` 加 `vad_speech_segments` 字段；从 `build_segments_from_vad` 传出原始 timestamps |
| `voxtrans-core/src/vad.rs` | `build_segments_from_vad` 返回值增加原始 `vad.timestamps`（f64） |
| `src-tauri/src/services/transcribe/asr_align.rs` | `AsrAlignOutput` 加 `vad_speech_segments`；从 `prepared` 取值 |
| `src-tauri/src/services/transcribe.rs` | `TranscribeResponse` 加字段并填充 |
| `src-tauri/src/commands/workspace/pipeline_steps/recognition.rs` | `Step1AsrArtifact` 加字段（`#[serde(default)]` 保证向后兼容）；Step1 从 response 取值填入 |
| `src-tauri/src/commands/transcription.rs` | `SentenceBoundaryRequest` + `BuildSourceSentencesCommandRequest` 加字段 |
| `src-tauri/src/services/transcription/sentence_boundary/types.rs` | `SentenceBoundaryRequest` 加 `vad_speech_segments` |
| `src-tauri/src/services/transcription/sentence_boundary/mod.rs` | 传 VAD 片段给 semantic / subtitle_layout |
| `src-tauri/src/services/transcription/sentence_boundary/semantic.rs` | 删 `HARD_SPLIT_GAP_MS`；硬切判定换成 VAD 跨片段 |
| `src-tauri/src/services/transcription/sentence_boundary/subtitle_layout.rs` | rank 5 从 gap ≥ 350ms 换成 VAD 跨片段；删 `PAUSE_BONUS_GAP_MS` |
| `src-tauri/src/commands/workspace/translation_flow.rs` | Step1 artifact → Step2 request 时传递 VAD 片段 |

新增一个内部模块或在 `sentence_boundary/` 下加 `vad_align.rs`：封装"切点→是否跨片段"的查询逻辑（含容差），供 semantic 和 subtitle_layout 复用。

## 向后兼容

- `Step1AsrArtifact.vad_speech_segments` 标 `#[serde(default)]`：旧 artifact 反序列化时为空 Vec，Step2 降级到纯标点 + 长度预算
- 现有 checkpoint（step_01_asr）无需迁移：重跑 Step2 时若 VAD 字段缺失，自动降级
- 要让已有任务用上 VAD，需重跑 Step1（重新产出带 VAD 字段的 artifact）——但这不是必须的，旧任务继续按原逻辑工作

## 验证方案

### 消融实验（核心验证）

手动去除 words 的标点符号，对比"无 VAD"和"有 VAD"的断句效果：

1. **基线**：带标点 + 有 VAD → 最佳断句（参照）
2. **去标点 + 无 VAD**：预期断句质量明显下降（只剩长度预算 + 连词 + rank 6 词界）
3. **去标点 + 有 VAD**：预期接近基线（VAD 声学结构撑住断句）

合格标准：
- 短说话片段（<15 秒）：断句边界与 VAD 片段边界高度吻合
- 长片段内部（>15 秒）：靠长度预算切，可接受（VAD 无可用切点）

### 单元测试

- `crosses_vad_segment` 的容差判定：切点在静音正中、边缘、容差外、片段内部各一例
- `semantic.rs` 硬切点：跨片段产生 HardPause，同片段不产生
- `boundary_rank`：VAD 跨片段给 rank 5，标点仍优先于 VAD
- VAD 缺失（空 Vec）时降级：不崩，标点 + 长度预算正常工作

### 端到端

用 `output/` 下已有的两个任务（Trump-Zelensky、Orderblock）重跑 Step2，对比断句质量。需先重跑 Step1 产出带 VAD 字段的 artifact。

## 不做的事（YAGNI）

- 不改 VAD 模型本身（fireredvad 的 `min_silence_frame` 等配置不动）
- 不做"动态 VAD 阈值"（经分析对字幕断句无正向收益）
- 不统一前后端类型来源（generated bindings 的孤立文件问题）
- 不改 forced aligner 的词时间戳产出逻辑
