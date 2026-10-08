# 翻译层契约：整句翻译 + 槽位回填（two-stage protocol）

这份文档描述 `services::translation` 的对外契约和内部不变量。改动这一层之前先读它；
改动之后如果契约变了，先改这份文档再改代码。

## 1. 名词

| 名词 | 定义 | 代码位置 |
|---|---|---|
| **slot（显示槽）** | 一条上屏字幕（SoT 的一行），全片唯一的全局 id = 1-based segment id | `types::BatchWindow::unit_slots` |
| **unit（翻译单元）** | 一个完整语义句：相邻且 `sentence_id` 相同的 segment 组成一个 unit；`sentence_id == 0` 时每条字幕自成一个 unit | `batches::build_translation_units` |
| **window（批次窗口）** | 一次 LLM 调用发送的 unit 集合。大小 = 调用方的 `batch_size`；未给（0）时用 `DEFAULT_BATCH_SIZE`，上限 `MAX_BATCH_SIZE`。桌面端、命令默认值与示例统一传 `DEFAULT_BATCH_SIZE`（8） | `batches::build_batch_windows` |
| **ladder（降级阶梯）** | 窗口失败后的切分：>4 个 unit 切成每片 4，>2 切成每片 2，2 切成每片 1；1 个 unit 的窗口进入 lenient 模式。默认窗口的路径即 8→4→2→1 | `batches::split_chunk_size`、`mod::translate_window_with_split` |
| **lenient floor** | 阶梯最底层：不再重试，能救则救（槽位完整就保留，只有整句就按比例回填） | `responses::validate_sentence_slot_response_with_context` |

## 2. 一次调用的流程

```
segments (sentence_id, text, tokens)
  │ normalize_segments          空文本丢弃；segment_id = 位置 + 1
  ▼
NormalizedSegment[]
  │ build_batch_windows         按 sentence_id 组成 unit；按 batch_size 切窗口；
  │                             每个 unit 带上它的全局 slot id + 槽源文本；
  │                             前 3 / 后 2 个 unit 作为上下文；术语 ≤16 条
  ▼
BatchWindow
  │ build_prompt                currentUnits[{id,text,slots[{id,text}]}]
  ▼
LLM 返回 {"units":[{"id","translation","slots":[{"id","text"}]}]}
  │ validate_..._with_context   接受门（见 §3）
  ▼
ValidatedUnit[]                 Slots(texts) 或 TranslationOnly(整句)
  │ repair_slot_boundaries      确定性边界修复（见 §4）
  │ proportional_split          TranslationOnly 时按槽源字符数加权分配
  ▼
segment_translations: global slot id → 上屏文本
```

失败路径：任一接受门不通过 → `LlmSemanticValidationError::retryable` → 客户端重试 →
仍失败则 `translate_window_with_split` 按阶梯切分当前窗口、逐半重试 →
单 unit 窗口用 `lenient = true` 兜底。**任何一层都不会把坏字幕写进 SoT。**

## 3. 接受门（按顺序）

| # | 检查 | strict | lenient（阶梯底层） |
|---|---|---|---|
| 1 | unit id 齐全、无重复、translation 非空 | 报错重试 | 同左 |
| 2 | 该 unit 的每个 slot id 都出现且非空 | 报错重试 | 降级为 `TranslationOnly`（整句按比例回填） |
| 3 | `norm_core(槽位拼接) == norm_core(整句译文)` | 报错重试 | soft accept：保留槽位（内容仍是模型的整句译文） |
| 4 | 源文有内容的 slot 不得只渲染标点（`norm_core(source) != "" ⇒ norm_core(slot) != ""`） | 报错重试 | 豁免 |
| 5 | 源语言脚本泄漏（按 source_lang 的 distinctive script，术语 target 已剥离） | 报错重试 | 同左 |
| 6 | 相邻 unit 译文重复（模型把两句并成一句再补 id 的特征） | 报错重试 | 同左 |

`norm_core` = 小写 + 全角折叠 + 只保留 Unicode alphanumeric（空白与标点忽略），
定义在 `slots.rs`，是第 3、4 条的共同判据。

## 4. 确定性修复规则（`slots::repair_slot_boundaries`）

四类"检测—吸附"，只移动切点、不改写文字：

| 规则 | 触发 | 动作 |
|---|---|---|
| ① 行首禁则 | 槽位以非开括号标点开头 | 把该标点并回前一个槽 |
| ② 空格语言劈词 | 相邻两字符都是字母数字且至少一个是 ASCII | 吸附到最近的空格 |
| ③ 中文词内切分 | zh 目标且切点落在 jieba 词典词内部 | 吸附到最近的词边 |
| ④ 纯标点槽 | 某槽整槽只有标点 | 把前一槽的末段词块拉下来（CJK 目标恒修；空格语言保留邻字符守卫） |

**接受门**：修复后 `norm_core(拼接)` 必须不变、且不得产生空槽；否则整组放弃，保留原文。

## 5. 不变量（改代码必须保持）

1. **文本只来自 ASR token**：显示文本是 token 的投影；任何一层都不许改写、增删、调序词。
2. **unit 分组只由 `sentence_id` 决定**：相邻 + 非零 id 相同 ⇒ 同一 unit；0 ⇒ 每条自成 unit。
3. **slot id 全局唯一且稳定**：切分窗口（阶梯降级）只改本地序号，全局 slot id 与归属不变。
4. **内容守恒**：发布到 SoT 的槽位拼接在 `norm_core` 下等于整句译文（lenient 的
   `TranslationOnly` 回填同样满足，因为它是同一段文本的比例切分）。
5. **无空行**：接受门 + 修复门保证不产生空槽；beautify 阶段也不得清空一行。
6. **`semantic_sentence_id` 是持久字段**：`subtitle_segments` 表有列（schema + 迁移），
   JSON SoT 由这些行重建。丢掉它 = 句子级翻译静默退化为逐条翻译。

## 6. 改动地图

| 想改什么 | 改哪里 |
|---|---|
| 批大小、上下文行数、术语上限 | `mod.rs` 顶部常量 |
| prompt 措辞 / 输出 JSON 形状 | `services/prompts/translation.rs`（不发给服务商 schema，形状只在本地接受门检查） |
| 接受门判据 | `responses.rs` |
| 确定性修复规则 | `slots.rs` |
| 降级阶梯 | `batches.rs::split_chunk_size / split_window` + `mod.rs::translate_window_with_split` |
| unit 分组规则 | `batches.rs::build_translation_units` |
| 端到端回归样例 | `testdata/ep6_slot_units.json` + `tests.rs` 的 fixture 测试 |
