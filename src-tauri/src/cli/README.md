# voxtrans-cli：无 GUI 的分阶段管线入口

这份文档描述 `voxtrans::cli`（二进制 `voxtrans-cli`，源码 `src-tauri/src/cli/`）的对外契约。
改动这一层之前先读它；改动之后如果契约变了，先改这份文档再改代码。

## 1. 为什么存在

桌面应用把四个阶段绑成一次"整条跑"：ASR + 强制对齐 → 句子切分 + beautify → 术语 → 翻译。
而四段里最贵的是第一段：只想改翻译措辞或换术语表，在 GUI 里也要重跑一次 ASR。

| 入口 | 能驱动的最小单位 | 复跑代价 | 机器可读结果 |
|---|---|---|---|
| 桌面应用 | 整条任务 | 每次都得重跑 ASR | 任务表 / UI 日志，面向人 |
| `examples/replay_*.rs`（已删除，见 git 历史） | 手写到哪步算哪步 | 不用重跑 ASR，但设置和路径写死 | 无统一报告 |
| **`voxtrans-cli`** | 一个 verb 一个阶段 | 从落盘 artifact 继续 | 每次运行一份 `run.json` |

`examples/` 下曾经有 `replay_pipeline.rs`、`replay_translation_ab.rs`、`replay_terminology.rs`、
`terminology_fast_demo.rs` 四个手搓脚本，正是被这个 CLI 取代的：四个脚本各自重复拼装设置、把
`C:\Users\...` 的输出目录和临时 settings JSON 写死在代码里，再和 GUI 的步进逻辑各演化一份。
它们在 CLI 落地时一并删除（要翻旧账看 git 历史）；今后不要再往 `examples/` 里加一次性脚本，
新能力加一个 verb。

整个 CLI 只有一条硬规则：**它是服务层的调用方，不是第二个实现**。

- 配置来自桌面应用同一个 SQLite settings 行，经 `runtime_settings::runtime_from_saved` 解析，
  所以"什么被冻结、什么必填、术语组怎么展开"不会在 GUI / CLI 之间漂移。
- 中间 artifact 的构造用的就是 app 的那几个转换函数（`artifacts.rs` 顶部注释列出了来源）。
- `pipeline` 只是四个 stage 的顺序组合 + 计时，自己不含管线逻辑（见 `pipeline.rs` 的模块注释）。

## 2. 五个 verb

所有 verb 共享同一组公共旗标（`--db`、`--out`、`--task`、`--source`/`--target`、模型 / API 覆盖、术语、
字幕预设、`--json`/`--quiet`……）。与该 verb 无关的旗标会被解析但不产生效果。

| verb | 位置参数 | 执行的阶段 | 产物 |
|---|---|---|---|
| `transcribe` | `<media>` 音视频 | ASR + 强制对齐 | `words.json` |
| `segment` | `<words.json>` | 句切分 + beautify | `source.json`、`src.srt` |
| `terminology` | `<srt\|source.json\|words.json>` | 术语 agent（可 `--briefing` 注入） | `briefing.json` |
| `translate` | `<srt\|source.json\|words.json>` | （可选）术语 + 翻译层 | `trans.json` + 4 个 SRT 变体 |
| `pipeline` | `<media>` | 上面四个按顺序跑 | 以上全部 |

### 输入 → 输出 → 报告统计

| verb | 输入 | 输出 artifact | `run.json` 中该 stage 的 `detail` 键 |
|---|---|---|---|
| transcribe | media | words.json | words, asrModel, alignModel, audioDurationSec, rtfX, provider, cachedAsrSegments, cachedAlignSegments |
| segment | words.json | source.json, src.srt | cues, sentences, microChunks, boundaries, beautified, preset |
| terminology | srt / source.json / words.json（+ `--briefing`） | briefing.json | cues, windows, terms, styleGuideChars, endReason, skipped |
| translate | 同上 | trans.json, src.srt, trans.srt, src_trans.srt, trans_src.srt | segments, batches, batchSize, glossaryTerms, userTerms, agentTerms, elapsedSec |
| translate `--dry-run` | 同上 | prompts.json | units, windows, glossaryTerms（stage 名 `plan`） || pipeline | media | 以上全部（术语被注入或跳过时没有 briefing.json） | 每个 stage 与单独跑同键，可直接对比 |

注：

- translate 既没给 `--briefing` 也没给 `--no-terminology` 时，会先跑一遍术语 agent，报告里多一条
  `terminology` stage；其 detail 与 `translate` 内联版本完全一致（只有 windows / terms /
  styleGuideChars / endReason / skipped 五个键；`terminology` verb 单独跑时还会多一个 `cues`）。
  **`--dry-run` 例外**：dry run 保证零计费，所以默认跳过 agent（想在计划里看到真实术语表就显式
  `--briefing` 注入），跳过的决定会写进 `run.json` 的 `warnings[]`。
- `--no-cache` 关掉的是整个 store，不是只有 unit 断点缓存：terminology 的 merged-briefing 缓存和
  LLM token 记账都挂在同一个 store 上，所以这种运行既不读旧结果也不写 token（A/B 要的是干净对照）。
- translate / terminology 以 `words.json` 为输入时，会在内存里先跑 segment，并把 `source.json`/`src.srt`
  落到 `--out`——写文件的是 segment stage 自己，但报告按 translate / terminology 记账。
- `src.srt` 会被写两次（segment 一次，translate 的四个 SRT 变体里也含它）；`report.artifact` 按路径去重。

### artifact 文件表

| 文件（在 `--out` 下） | 内容 | 谁写 | 谁读 |
|---|---|---|---|
| `words.json` | 对齐后的 token、VAD 语音段、ASR 统计与模型名 | transcribe | segment；translate / terminology 可直接当输入 |
| `source.json` | 句级 SoT：`segments[]`（含 semanticSentenceId + tokens）+ 统计 | segment | terminology / translate |
| `src.srt` | 源文字幕（beautify 之后的边界） | segment / translate | 人、外部工具 |
| `briefing.json` | `glossary[]` + `style_guide` + `end_reason` / `skipped`（snake_case；`load_briefing` 也接受 camelCase 的轻量形式） | terminology | `translate --briefing` |
| `trans.json` | 每条 segment 的译文 + 统计 | translate | 下游 / 外部工具 |
| `prompts.json` | dry-run 的计划窗口与完整 prompt，不发网络 | translate `--dry-run` | 人、模型 |
| `run.json` | 运行报告（见 §4） | 每个 verb | 脚本、CI |

谁产出谁落盘：verb / pipeline 只负责在报告里登记路径，不重复写别的阶段的文件。这样单跑一个
verb 和 `pipeline` 整跑得到的是同一批 artifact。

## 3. 配置来源

优先级（高的覆盖低的）：**命令行旗标 > settings DB > 内置默认**。旗标只作用于本次进程，不落盘。

| 来源 | 何时 | 内容 | 写库 |
|---|---|---|---|
| 桌面应用 DB | `--db auto`（默认）且默认路径存在 | 模型、provider、API key / base / model、并发、术语组、预设、beautify、models dir | 读；只有 `--task` 才写 |
| 显式 DB | `--db <path>` | 同上 | 同上 |
| 内置默认 | `--db none`，或 `auto` 但 DB 不存在 | `domain::preferences_normalize::default_settings()` | 不写 |
| 旗标 | 任意 verb | 逐字段覆盖本次运行 | 不写 |

`--db` 取值：

| 值 | 解析 |
|---|---|
| `auto`（默认） | `%APPDATA%\com.voxtrans.desktop\voxtrans.db`（macOS `~/Library/Application Support/...`，Linux `$XDG_DATA_HOME/...`）。文件不存在视为内置默认，不报错 |
| `none` | 不打开任何 DB，全部走内置默认 |
| 其他 | 当作 DB 路径，必须已存在，否则报错 |

`--task` 的写库规则：

| 运行方式 | tasks 行 | unit 断点缓存 | token |
|---|---|---|---|
| 无 `--task` | 不建 | 不读不写 | 不记 |
| `--task <ID>` | 建（已存在则复用，不覆盖 GUI 的行） | 读 / 写 | 写 `task_usage` |
| `--task <ID> --no-cache` | 建 | 不读不写（A/B 防缓存代答） | 不记（store 被关，unit 缓存与 token 记账一起停） |

`--task` 要求 DB 可用（`--db none --task X` 会直接报错）。反过来，没有 `--task` 的运行必须保持
零副作用：这样才能做模型对比、CI 回归而不污染用户任务列表。**不要**把 `--task` 做成默认值——
那会让一次实验性运行出现在 GUI 的任务列表里，还得手动清理。

`--task` 注册的行只满足外键与断点缓存的需要，**不发布 SoT**：`subtitle_segments_json` 与预览
保持为空，`transcribe_status` 固定为 `done`。想拿字幕就看 `--out` 里的 artifact；GUI 里那条任务
只是一条占位记录（`media_kind = cli`，与视频/音频/字幕任务可区分）。

LLM 凭据检查：`terminology`（未注入 `--briefing` 时）、`translate`、`pipeline` 在进入阶段前检查
`translateApiKey` / `translateBaseUrl` / `translateModel` 三项，缺一项立刻失败；`transcribe` / `segment`
不需要 LLM。`terminology --briefing` 不调模型，也不要求凭据。

语言默认：`--source` 缺省 `en`，`--target` 缺省 `zh-CN`（与 app 新建任务的种子一致）。

常用旗标分组：

| 覆盖项 | 旗标 |
|---|---|
| ASR / 对齐 | `--asr-model`、`--align-model`、`--provider`、`--chunk-seconds`、`--models-dir`、`--vocal-separation` / `--no-vocal-separation` |
| LLM | `--api-key`、`--base-url`、`--model`、`--concurrency`、`--batch-size` |
| 术语 | `--terms <file>`、`--terms-group <id>` |
| 样式 / 字幕 | `--style-guide`、`--preset`、`--beautify` / `--no-beautify` |
| 运行 | `--out`、`--task`、`--no-cache`、`--json`、`--quiet` |

## 4. 输出契约

| 通道 | 内容 | 备注 |
|---|---|---|
| stdout | 一次运行的结果：默认是一屏人类摘要；`--json` 时是 `run.json` 全文 | 脚本只解析 stdout |
| stderr | 进度 `[phase] done/total detail` 与诊断 | `--quiet` 关进度，warning / error 仍然输出；同一行（phase+done/total+detail）只打一次——翻译的流式回调会在一个批次里发很多帧，只有计数器变化才算进展 |
| 退出码 | `0` 成功；`1` 运行失败；`2` 用法错误（clap，含缺位置参数） | |

`<out>/run.json` 与 `--json` 打印的是同一份文档，失败也会写（除非 context 都没建起来）。
序列化一律 camelCase，顶层 `schema` 目前是 `voxtrans.cli/1`。

| 字段 | 含义 |
|---|---|
| `schema` | 报告 schema id；形状有消费者可见变化时递增 |
| `verb` / `taskId` / `mediaPath` / `sourceLang` / `targetLang` | 本次运行的身份 |
| `settingsSource` | `db:<path>` 或 `defaults` |
| `taskRegistered` | 是否走了 `--task` 写库路径 |
| `outDir` | `--out` 解析后的绝对路径 |
| `ok` / `elapsedSec` / `error` | 结果、总耗时、失败原因（失败时才有 `error`） |
| `stages[]` | 每个已执行阶段：`name`、`elapsedSec`、`source`（`computed` / `cache` / `skipped`）、`detail`（§2 的统计键）。`transcribe` 在全部单元都来自断点表时记 `cache`（真的没跑推理，`cachedAsrSegments` 等于 `segmentTotal`）；其余情况记 `computed` |
| `artifacts[]` | 本次产出的 artifact 绝对路径，去重 |
| `llm` | 本次 `llm.log` 聚合的 token：`{ calls, promptTokens, completionTokens, totalTokens, byPhase: { <phase>: {…} } }`；没有 LLM 调用时整个字段缺省 |
| `warnings[]` | 运行警告（缓存保存失败等） |

失败语义是 fail-fast：任一阶段返回 `Err`，后面的阶段不再运行；已完成阶段保留计时，失败阶段不写进
`stages[]`，`ok=false`、`error` 为错误文本，`run.json` 照常落盘。脚本应同时看退出码和 `ok`。

LLM 日志与 token 聚合：

- LLM 客户端把每次调用写成一条 `llm.call` 事件，落在 `<out>` 的任务目录里：默认
  `<out>/<media 主干名>_<taskId>/artifacts/logs/llm.log`（media 已在 `<out>` 下时即其所在目录；
  与 app 的任务目录布局一致；`main.log` 同目录）。
- `run.json` 的 `llm` 是 `report.finish` 读本次 `llm.log` 聚合出来的，按事件里的 `phase`
  （翻译批次为 `step4_translate_batch`，术语 agent 为 `terminology`）分桶到 `byPhase`。只有 `status=ok`
  的调用计数；`totalTokens` 缺失时回退为 prompt + completion。
- 为什么不让 verb 自己记 token：一次翻译会拆成多个窗口、失败重试还会再调，只有 LLM 客户端看得到全量；
  从日志回读能覆盖所有阶段，又不用把计数器穿过整条管线。

## 5. 常用命令

```bash
# 只跑一次 ASR（不需要 LLM 配置），后续翻译全部复用这一份 words.json
voxtrans-cli transcribe media.mp4 --out work --db none

# 无头迭代翻译层：改旗标反复跑，ASR 不再执行
voxtrans-cli translate work/words.json --out work/run-a \
  --db none --api-key sk-... --base-url https://api.example.com/v1 --model gpt-x \
  --source en --target zh-CN --preset standard --beautify --json > work/run-a/stdout.json

# A/B 术语：A 只用用户词表；B 先跑 agent 拿 briefing，再显式注入（翻译可复现、agent 不重复计费）
voxtrans-cli translate work/source.json --out work/ab-a --no-terminology --terms glossary.json \
  --api-key sk-... --base-url ... --model ...
voxtrans-cli terminology work/source.json --out work/ab-b --api-key sk-... --base-url ... --model ...
voxtrans-cli translate work/source.json --out work/ab-b \
  --briefing work/ab-b/briefing.json --api-key sk-... --base-url ... --model ...

# 看真实会发出去的提示词（不联网），报告里 plan stage 给出 units/windows/glossaryTerms
voxtrans-cli translate work/source.json --out work/dry --dry-run --json   # agent 被跳过，零计费
# 想在计划里带真实术语表：先跑 terminology（会花钱），再注入
voxtrans-cli translate work/source.json --out work/dry2 --dry-run --briefing work/ab-b/briefing.json --json
jq '.stages[] | select(.name=="plan")' work/dry/run.json   # prompts.json 是完整提示词

# 端到端；挂进 GUI 任务表并启用断点缓存则加 --task，A/B 时加 --no-cache
voxtrans-cli pipeline media.mp4 --out work/e2e --db auto --json | jq '.stages, .artifacts'
voxtrans-cli pipeline media.mp4 --out work/e2e --task ep12-cli --no-cache
```

## 6. 新增一个 verb

| 步骤 | 文件 | 做什么 |
|---|---|---|
| 1 | `cli/mod.rs` | `Command` enum 加 variant，`dispatch` 加分支 |
| 2 | 自己的模块（或并入现有 `*_stage.rs`） | `#[derive(Args)]` 参数：`#[command(flatten)] common: CommonArgs` + 自己的位置参数；`run_<verb>` 与 `pub(crate) <name>_stage` |
| 3 | `cli/artifacts.rs` | 新 artifact 的文件名常量 + load / save 函数（复用已有类型时不必加） |
| 4 | 同模块 | `CliContext::new(ContextRequest { verb, common, input_path, require_llm })` → `RunReport::new` → `StageTimer` → stage → `report.artifact(...)` → `report.finish` |
| 5 | 这份 README | verb 表、artifact 表、detail 键各加一行 |

规矩：

- stage 函数自己写自己的 artifact，verb / pipeline 只登记路径，不重复写文件。
- stage 名与 detail 键和同类 verb 对齐，pipeline 报告才能和单独跑对比。
- 不引入 `AppHandle`，不加第二个配置文件；配置只走 `CliContext` / `runtime_from_saved`。
- 报告统计只放标量（`summarize_detail` 只渲染标量，嵌套对象在人类摘要里会消失，但仍留在 JSON 里）。
