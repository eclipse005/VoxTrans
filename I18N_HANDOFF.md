# VoxTrans 国际化（i18n）交接文档

> 本文档供接手继续开发的 AI 或人类工程师使用。它记录了 i18n 架构的总体方案、已完成的进度、精确到文件的待办清单，以及关键的设计决策和"坑"。
>
> **当前状态：阶段 0 ✅、阶段 1 ✅、阶段 2 ✅、阶段 3 ✅、阶段 4 ✅（语言切换双语持久化已完成，130/130 测试通过，tsc 清洁，lint 0 错误，198 后端测试通过）。仅剩手测中英双语回归。**

---

## 0. 一句话现状

项目正在从"全中文硬编码"迁移到"react-i18next（前端）+ 后端返回英文/code（前端本地化）"的架构。**阶段 0–2 已完成：后端所有用户可见中文已转英文/code，前端 8 个命名空间的字符串已抽取到 i18n JSON（zh-CN + en 双语），UI 已具备双语基础。剩余工作是阶段 3（语言切换持久化）和阶段 4（时区 bug 等收尾）。**

### 新增的"坑"记录
- 子 agent 报告的翻译若含中文弯引号（`" "`）或字符串内嵌双引号，直接写入 JSON 会破坏 JSON 解析（`JSONDecodeError`）。合并 locale JSON 后**必须**用 `python -c "import json; ..."` 校验全部 16 个文件，并把内部引号转义为 `\"`。
- 子 agent 在编辑大文件（>600 行）时常触及 6000 output token 上限，留下"只加了 `useTranslation` import 没替换字符串"的半成品。应把任务拆成 ≤4 个小文件一组，或接手后做一遍完整盘点（见下方"接手执行顺序"）。
- 并发子 agent 会互相覆盖文件（如 MediaList agent 越界覆盖了已完成的 `errors.ts`）。**只能让 agent 编辑 `.tsx`/`.ts`，严禁其编辑 JSON**；JSON 由主流程统一合并。
- `errors.ts` 里的 `ERROR_PATTERNS` 正则残存中文（`/cancelled|取消|已取消/i`）是**故意的**——用于匹配旧后端残留中文，勿删。

---

## 1. 总体架构方案（已确定，勿改）

### 1.1 前端
- **库**：`react-i18next` + `i18next`（已安装）。翻译 JSON 静态打包进 bundle（桌面应用，离线优先，不懒加载）。
- **组织**：8 个命名空间，`src/i18n/locales/{zh-CN,en}/<ns>.json`：
  - `common`（通用按钮/状态/导航）、`settings`、`tasks`（工作区/队列/上传）、`subtitles`（字幕编辑/导出/术语）、`models`（模型下载）、`errors`（后端错误 code 映射）、`toasts`（hook 里的 toast）、`updater`（更新/相对时间）
- **入口**：`src/i18n/index.ts` 已写好，在 `src/main.tsx` 顶部 `import './i18n'` 触发同步初始化，默认语言 `zh-CN`。
- **API**：组件里 `const { t } = useTranslation(["ns"])` 然后 `t("ns:key.path")` 或 `t("key.path")`（默认 ns 为 common）。
- **插值**：`t("toasts:queue.started", { count: 5 })` → JSON 里 `"started": "开始批量处理，共 {{count}} 个文件"`。`interpolation.escapeValue = false`（React 自动转义）。

### 1.2 后端
- **原则**：后端不再向用户返回任何中文。错误走稳定 `code`（前端用 code 查 `errors:` 字典），进度消息走英文短 code。
- **结构化错误**：
  - `WorkspaceError`（`domain/error.rs`）：**原有**，覆盖 workspace 管线子树，已带 `code()` + 英文 message + 序列化为 `{code, message}` JSON。**无需改动。**
  - `AppError`（`domain/app_error.rs`）：**新建**，覆盖其余命令（system/youtube/updater/model/translate/subtitle/demucs/file/download）。已实现，有 `code()`、`to_command_error()`、`From<AppError> for String`。
  - 两者共用 `CommandErrorPayload`（在 `error.rs` 里已改为 `pub(crate)`）。

### 1.3 语言切换
- **不检测系统语言，默认中文**。用户在设置里手动切换，存入 `SavedSettings.locale`，重启后用保存值。
- 启动流程：`main.tsx` 同步初始化 i18n（zh-CN）→ App 加载偏好后调 `changeAppLanguage(locale)` 切换。
- 设置弹窗加"界面语言"下拉，保存后即时切换（`i18n.changeLanguage` 触发所有用 `useTranslation` 的组件重渲染，无需刷新）。

---

## 2. 关键设计决策（重要！避免返工）

### 决策 1：后端 `TaskStage::label()` 一律返回空字符串
- **原因**：前端 `MediaList.tsx` 的 `getTranscribeProcessingText` 本就有 `stage.label.trim() || resolveStageLabel(stage.code)` 的 fallback 逻辑。让后端 label 为空，前端统一用 `stage.code` 查字典（`resolveStageLabel` 会改成查 i18n）。
- **已完成**：`domain/task/stage.rs` 的 `label()` 已改为返回 `""`。

### 决策 2：后端 emit 进度的 `message` 字段——大部分是死字段
- **调研发现**：三个 emit 事件（`youtube-download-progress`、`model-download-progress`、`task-state-changed`）中：
  - `youtube-download-progress.message`：**前端从不渲染**（`useYoutubeDownloadWorkflow.ts` 忽略它，用自己硬编码的 label）
  - `model-download-progress.message`：**前端存储但从不渲染**（`ModelDownloadCard.tsx` 只用 bytes/phase）
  - `task-state-changed`：`stage.label` **会渲染**（但按决策 1 已置空）；`transcribeError` **存储但从不渲染**
- **结论**：emit message 字段的中文转换是**零用户风险**的，改成英文 code 即可。`transcribeError` 当前不显示，转换优先级低，但为干净起见仍应转。

### 决策 3：`"缓存命中"` detail 用 `step_` 前缀 sentinel
- **原因**：前端 `MediaList.tsx:120` 有 `rawDetail.startsWith("step_") ? "" : rawDetail` 的隐藏约定——以 `step_` 开头的 detail 不显示。
- **已完成**：`translation_flow.rs` 两处 `"缓存命中"` 改为 `"step_cache_hit"`。

### 决策 4：哪些后端中文**绝对不能动**
以下文件的中文是**语言处理业务数据或测试夹具**，不是 UI 文本，转了会破坏功能：
- `services/subtitle_beautify.rs` — 字幕美化测试用例（"你好世界"等）
- `services/subtitle_step5/numbers.rs` — 中文数字解析（'零'/'一'/'万'/'亿'…）
- `services/subtitle_step5/quality.rs`、`watchability.rs`、`watchability_merge.rs` — 中文连接词/停用词表
- `services/transcription/sentence_boundary/language.rs` — 分词连接词表
- `commands/translate.rs` — 测试夹具（"你好世界"/"北约"/"欧盟"）
- `voxtrans-core/` 整个 crate — 纯库，中文全是测试和语言数据
- 所有 `#[cfg(test)]` 块和代码注释里的中文

### 决策 5：翻译 JSON 通过 resources.ts 聚合 import
- 项目 tsconfig 是 `verbatimModuleSyntax` + `erasableSyntaxOnly` 严格模式，且没配 `@/` 路径别名。`src/i18n/resources.ts` 用相对路径 `import xxx from "./locales/.../x.json"` 聚合所有 JSON，再传给 i18next。这是经过验证的可编译写法。

---

## 3. 已完成的进度（精确到文件）

### 阶段 0 — 前端 i18n 基础设施 ✅ 全部完成
| 文件 | 状态 |
|------|------|
| `package.json` / `package-lock.json` | 已加 `i18next` + `react-i18next` 依赖 |
| `src/i18n/index.ts` | **新建**。i18next 初始化，导出 `changeAppLanguage`、`normalizeLocale`、`SUPPORTED_LOCALES`、`DEFAULT_LOCALE`、`AppLocale` 类型 |
| `src/i18n/resources.ts` | **新建**。聚合 16 个 JSON，导出 `resources`、`NAMESPACES`、`SUPPORTED_LOCALES`、`DEFAULT_LOCALE`、`AppLocale` |
| `src/i18n/locales/zh-CN/*.json` (8 个) | **新建**。只有 `common.json` 填了少量 key（nav/button/status/loading），其余 7 个是**空 `{}`**，待阶段 2 填充 |
| `src/i18n/locales/en/*.json` (8 个) | **新建**。同上，`common.json` 有英文翻译，其余空 |
| `src/main.tsx` | 已加 `import './i18n'` |

**验证状态**：`npx tsc -b` ✅、`npx eslint src/i18n src/main.tsx` ✅

### 阶段 1 — 后端中文转换 ✅ 全部完成
| 文件 | 状态 | 说明 |
|------|------|------|
| `domain/app_error.rs` | ✅ 完成 | `AppError` 枚举 + `code()` + `to_command_error()` + 测试 |
| `domain/error.rs` | ✅ 完成 | `CommandErrorPayload` 改为 `pub(crate)` |
| `domain/mod.rs` | ✅ 完成 | 注册 `app_error` 模块 |
| `domain/task/stage.rs` | ✅ 完成 | `label()` 改为返回 `""` |
| `commands/workspace/translation_flow.rs` | ✅ 完成 | 两处 `"缓存命中"` → `"step_cache_hit"` |
| `services/youtube.rs` | ✅ 完成 | 所有中文 Err + emit message 转英文/code |
| `services/file_download.rs` | ✅ 完成 | 全部中文已转（含 HTTP 客户端/写入/重命名等） |
| `services/updater.rs` | ✅ 完成 | 全部中文已转 |
| `services/model/downloader.rs` | ✅ 完成 | emit message 转英文 |
| `commands/translate_connectivity.rs` | ✅ 完成 | LLM 连通性错误转英文 |
| `commands/system.rs` | ✅ 完成 | 系统字体错误转英文 |
| `commands/youtube.rs` | ✅ 完成 | map_err 转英文 |
| `commands/updater.rs` | ✅ 完成 | map_err 转英文 |
| `services/subtitle_render.rs` | ✅ 完成 | 压制/ffmpeg 错误转英文 |
| `services/subtitle_srt.rs` | ✅ 完成 | 导出错误转英文 |
| `services/file.rs` | ✅ 完成 | 导出/解析错误转英文 |
| `services/demucs/` | ✅ 完成 | 音频提取/进程错误转英文 |
| `services/transcribe.rs` | ✅ 完成 | 用户可见错误转英文 |
| `db/store.rs` | ✅ 完成 | SQL sentinel `'TASK_INTERRUPTED'` + recover 错误转英文 |
| `services/preferences_normalize.rs` | ✅ 完成 | 默认组名 `"默认"` → `"Default"` |
| 其余少量中文的文件 | ✅ 完成 | batches/asr_align/terminology_text 等均已转 |

**验证状态**：`cargo check -p voxtrans` ✅。仅受保护内容（`#[cfg(test)]`、语言 endonyms、prompt 模板）保留中文。
| 其余服务/命令文件 | ❌ 未开始 | 见 §4 待办清单 |

**验证状态**：`cargo check -p voxtrans` ✅（当前中断点可编译）

---

## 4. 阶段 1 待办：后端中文转换清单

> 用 `python` 脚本统计过：后端 `.rs` 代码内（排除注释）中文字符共 1068 个，分布在 32 个文件。但其中约一半是**不能动的语言数据/测试**（见决策 4）。下面是**真正需要转换**的文件和位置。

### 4.1 必须转换的文件（用户可见的 Err / emit message）

#### `services/file_download.rs`（剩余，约 8 处）
- 行 85、96：`"请求失败: {}"` × 2
- 行 105：`"下载失败: HTTP {}"`
- 行 122：`"打开临时文件失败: {}"`
- 行 128：`callback.on_message("下载中")`
- 行 132：`callback.on_message("下载已取消")`
- 行 142：`"写入文件失败: {}"`
- 行 169：`"下载不完整: 预期 {} 字节，实际 {} 字节"`
- 行 175：`"重命名文件失败: {}"`
- 行 177：`callback.on_message("下载完成")`
- **注意**：`on_message` 当前唯一的实现（`updater.rs` 的 `Cb`）是 no-op，所以这些 message 暂时不触达前端，但为将来接通做准备仍应英文化。

#### `services/updater.rs`（约 10 处）
- 行 109：`"未知错误"`（fallback）
- 行 116：`"创建 HTTP 客户端失败: {}"`
- 行 124：`"请求 GitHub API 失败: {}"`
- 行 127：`"GitHub API 返回错误: {}"`
- 行 133：`"读取响应体失败: {}"`
- 行 136：`"GitHub API 返回空响应"`
- 行 140：`"解析失败: {}"`
- 行 149：`"未找到当前版本对应的安装包（{BUILD_VARIANT}）"`
- 行 174：`"创建临时目录失败: {}"`
- 行 221：`"更新下载已取消"`
- 行 228：`"启动安装程序失败: {}"`

#### `services/model/downloader.rs`（约 8 处）
- 行 33、82、156、227、239、271、300 等：`"开始下载模型"`、`"下载任务异常: {}"`、`"模型下载中"`、`"模型下载完成"`、`"下载已取消"` 等
- emit `model-download-progress` 的 message 字段（前端不渲染，但需英文化）

#### `commands/translate_connectivity.rs`（4 处）
- 行 69、73、86、88：LLM 连通性测试相关错误，如 `"LLM 连通性测试失败(已启用图片辅助翻译): {}。若模型不支持图片输入,请关闭该开关。"`

#### `commands/system.rs`（1 处）
- 行 68：`"读取系统字体失败: {err}"`

#### `commands/youtube.rs`（约 2 处）
- 命令入口的 `map_err`：`"YouTube 下载任务异常: {err}"`、`"yt-dlp 更新任务异常: {err}"`

#### `commands/updater.rs`（约 2 处）
- 行 60：`"下载任务异常: {}"`、行 82：`"打开链接失败: {}"`

#### `services/subtitle_render.rs`（约 6 处）
- 行 38、43、263、317、325 等：`"当前任务没有可压制的字幕"`、`"压制硬字幕失败"`、`"ffmpeg 执行失败"` 等

#### `services/subtitle_srt.rs`（约 3 处）
- 行 96、127、248：`"导出目录不存在"`、`"当前任务暂无译文，无法导出"` 等

#### `services/file.rs`（约 4 处）
- 行 106、160、168、170：`"导出目录不存在"`、`"字幕片段解析失败"`、`"当前任务没有可导出的字幕片段"` 等

#### `services/demucs/audio_prep.rs`（约 3 处）
- 行 36、44、46：`"提取音频失败"` 变体

#### `services/demucs/process_runner.rs`（约 4 处）
- 行 32、38、56、59：`"启动 demucs 失败"`、`"等待 demucs 失败"` 等

#### `services/demucs.rs`（约 2 处）

#### `services/transcribe.rs`（约 2 处用户可见）

#### `db/store.rs`（1 处，重要）
- **行 493**：SQL 中的中文 `'任务在运行中被中断，请重新开始'`。改为英文 sentinel 如 `'TASK_INTERRUPTED'`，前端识别后本地化。

#### 其他少量（每处 1-3 个）
- `services/translation/batches.rs`、`services/transcribe/asr_align.rs`、`services/terminology_text.rs`、`domain/language.rs`、`services/prompts/terminology.rs`、`services/terminology_responses.rs`、`domain/task/runtime_settings.rs`、`db/conversion.rs`、`services/preferences_normalize.rs`（4 个字）
- ⚠️ **逐个核实**：其中部分可能是错误消息（转），部分可能是 prompt 模板内容或业务字符串（视情况）。`domain/language.rs` 和 `language_registry` 的中文可能是语言**名称数据**（如"中文普通话"），这种是业务数据，见决策 6。

### 4.2 转换方法建议
1. **简单 `Err(String)` / `format!("中文")`**：直接把中文换成英文，保持 `Result<_, String>` 签名不变（最小改动）。英文 message 会到前端，由 `errors.ts` 的 pattern 匹配兜底。
2. **想用结构化 code 的**：把 `Result<_, String>` 改成 `Result<_, AppError>`，用合适的 variant 包装（如 `AppError::Youtube(detail)`）。`From<AppError> for String` 已实现，Tauri 命令层无需改签名也能用。**但这会增加改动面，建议优先用方法 1，只有需要精确 code 时才用方法 2。**
3. **emit message 字段**：改成英文短 code（如 `"downloading"`、`"merging"`、`"done"`），前端后续可映射。

### 4.3 `AppError` 的使用方式
```rust
use crate::domain::app_error::{AppError, AppResult};

// 方式 A：返回 AppResult，Tauri 自动序列化
#[tauri::command]
pub fn list_system_fonts(...) -> AppResult<Vec<String>> {
    fonts.map_err(|e| AppError::SystemFont(e.to_string()))
}

// 方式 B：保持 String 返回，但用 AppError 生成结构化错误
.map_err(|e| AppError::SystemFont(e.to_string()).to_command_error())?
```

---

## 5. 阶段 2 待办：前端字符串抽取 ✅ 基本完成

> **已完成**：所有 8 个命名空间（common/settings/models/errors/subtitles/tasks/toasts/updater）的字符串已抽取到 `src/i18n/locales/{zh-CN,en}/*.json`，双语齐全。`npx tsc -b` ✅、`npx vitest run` 130/130 ✅、16 个 JSON 文件全部可解析。

**新增文件**：`src/test/setup.ts`（`import "../i18n"`）+ `vitest.config.ts` 的 `setupFiles` 注入，让测试中 `t()` 能解析真实翻译。

**已改造的核心文件**（非完整列表）：
- `src/app/utils/errors.ts` — `toUserErrorMessage` 全部走 `i18n.t("errors:...")`；`ERROR_PATTERNS` 正则匹配英文后端 message
- `src/app/components/SettingsModal.tsx`、`MediaList.tsx`、`WorkspaceScreen.tsx`、`UploadPanel.tsx`
- `src/app/components/Navbar.tsx`、`TerminologyModal.tsx`、`LogsModal.tsx`、`UpdateModal.tsx`、`AppErrorBoundary.tsx`
- `src/app/components/subtitle-editor/*`（Toolbar/CueList/Header/Modal）、`SubtitleExportModal.tsx`
- `src/app/hooks/queue/*`、`useYoutubeDownloadWorkflow`、`useModelManager`、`useTaskLogs`、`useWorkspacePersistence`、`useSettingsController`、`useAutoUpdateCheck`、`useQueueWorkflow`、`useYtDlpManager`、`useSubtitleTimeValidation` 等
- `src/features/media/utils.ts` — `statusLabel` 改为返回 `common:status.*` key，调用方 `t(statusLabel(...))`
- `src/app/utils/subtitleWarnings.ts` — 走 `i18n.t("subtitles:warnings.*")`
- `src/app/components/settings/subtitleStylePresets.ts` — preset `label` 改为 `labelKey`，渲染处 `t(preset.labelKey)`

**仍保留中文的地方（故意的，勿改）**：
- `errors.ts` 的 `/cancelled|取消|已取消/i` 正则（匹配旧后端残留）
- `useQueueRunner.ts` 的 `"部分任务失败"` 字面量（测试断言）
- `SubtitleStylePreview.tsx` 的示例文本 `"清晨的雨已经停了。"`（预览 fixture）
- 各文件内的代码注释、`console.error`/`console.warn`（开发者面向，非 UI）
- 所有 `*.test.ts(x)` 文件

### 5.1 核心改造点（历史记录，已落地）

#### 改造 `src/app/utils/errors.ts`（最重要）
现有 `ERROR_CODE_MESSAGES`（中文）和 `ERROR_PATTERNS`（正则匹配）改成查 `errors:` 命名空间：
- `ERROR_CODE_MESSAGES` 的值改为 i18n key 引用，或直接在 `toUserErrorMessage` 里 `t(\`errors:code.${code}\`)`
- `ERROR_PATTERNS` 的正则要改成匹配**英文**后端 message（因为后端已转英文），value 改为 i18n key
- 这是所有后端错误走向用户的**咽喉**，约 60+ 处 `pushToast(toUserErrorMessage(error, "..."))` 调用都依赖它

#### 改造三类字符串模式
1. **内联 JSX**（最多）：`<span>术语</span>` → `<span>{t("common:nav.terminology")}</span>`
2. **`{id, label}` 配置数组**：`label: "短"` → `labelKey: "settings:subtitle.length.short"`，渲染处 `{t(item.labelKey)}`
3. **映射函数**：`statusLabel()` 返回 i18n key 而非中文，调用方 `t(statusLabelKey(status))`

### 5.2 字符串密集文件（按工作量排序）
| 文件 | 中文量 | 说明 |
|------|--------|------|
| `app/components/SettingsModal.tsx` | ~497 字符 | 最大块。含多个 `{id,label}` 配置数组 |
| `app/components/MediaList.tsx` | ~252 | 任务列表。`resolveStageLabel`/`getTranscribeProcessingText` 是重点 |
| `app/utils/errors.ts` | ~226 | 错误目录（见上） |
| `app/hooks/useYoutubeDownloadWorkflow.ts` | ~141 | toast |
| `app/hooks/queue/useQueueScheduler.ts` | ~108 | toast |
| `app/components/TerminologyModal.tsx` | ~124 | |
| `app/components/LogsModal.tsx` | ~119 | |
| `app/components/subtitle-editor/SubtitleEditorToolbar.tsx` | ~106 | |
| `app/components/WorkspaceScreen.tsx` | ~91 | |
| `app/components/UpdateModal.tsx` | ~89 | 含**时区 bug**（硬编码 Asia/Shanghai + zh-CN），见阶段 4 |
| 其余 hooks/components | 各 11-83 | |

### 5.3 工作流程建议
1. **先填 `errors:` 命名空间**（对应后端已转的 code/message），改 `errors.ts`，跑通错误路径的双语。
2. **再按组件逐个抽取**：Navbar → UploadPanel → MediaList → WorkspaceScreen → 各 Modal → 各 hook。
3. **边抽取边填 JSON**：抽取一个字符串就在 zh-CN/en 两个 JSON 里加对应 key。
4. 每改完一个组件，`npx tsc -b` + `npm run dev` 肉眼检查中文版无回归。

### 5.4 注意事项
- `MediaList.tsx` 的 `resolveStageLabel`（行 148-175）有**完整 9 个 stage code 的中文 map**，这是天然的 `errors:stage.<code>` key 来源。
- `features/media/languages.ts` 的 `SOURCE_LANGUAGE_OPTIONS` / `TARGET_LANGUAGE_OPTIONS` 的 `label`（如"中文普通话"、"日本語"）是**语言原文名（endonym）**，按 BCP-47 惯例保留，**不 i18n**（见决策 6）。但其中的 `promptLabel`/`DEFAULT_TARGET_LANGUAGE="zh-CN"` 需评估。
- `features/media/utils.ts` 的 `statusLabel` 改为返回 key。
- `UpdateModal.tsx` 的 `formatDateRelative` 是一堆中文 + 硬编码时区，阶段 4 专门处理。
- `useQueueRunner.test.ts:67` 有断言 `expect(...).toBe("文件读写失败，请检查磁盘空间")`，改 errors.ts 后要更新这个测试。
- `useWorkspacePersistence.ts:88,106` 有硬编码的 `transcribeError` 中文兜底字符串。

---

## 6. 阶段 3 ✅ 语言切换持久化 — 已完成

### 6.1 后端 `Locale` 字段 ✅
1. **`services/preferences_types.rs`** — 新增 `enum Locale { ZhCn, En }`（`#[derive(Default)]`，`ZhCn` 为默认），`SavedSettings` 末尾加 `#[serde(default = "default_locale")] pub locale: Locale`，加 `default_locale()` 辅助函数、`as_str()`/`parse()` 方法、ts-rs 导出测试
2. **`services/preferences_normalize.rs`** — `default_settings()` + `normalize_saved_settings` 加 `locale`
3. **`db/models.rs`** — `SettingsRow` 加 `pub locale: String`
4. **`db/conversion.rs`** — `settings_from_row`/`row_from_settings` 加 `locale`（用 `Locale::parse`/`as_str`）
5. **`db/mod.rs`** `MIGRATION_ALTERS` — 加 `"ALTER TABLE settings ADD COLUMN locale TEXT NOT NULL DEFAULT 'zh-CN'"`
6. **`db/store.rs`** — SELECT + UPSERT（列、占位符、excluded.X、bind）都加 `locale`

**关键决定**：`Locale` 的 serde/TS 表示用**标准 BCP-47 字符串**（`"zh-CN"` / `"en"`），通过 per-variant `#[serde(rename)]` + `#[ts(rename)]` 实现。这让 DB（`as_str`/`parse`）、Tauri IPC（serde）和前端 i18next（`AppLocale = "zh-CN" | "en"`）三个边界值完全一致。不要使用 `rename_all = "camelCase"`（那会产生 `"zhCn"`，与前端资源键不匹配）。生成 `Locale.ts` 类型为 `"zh-CN" | "en"`。

`npm run generate:types`（= `cargo test -p voxtrans ts_export_tests`）已运行，`SavedSettings.ts` 含 `locale: Locale`，`Locale.ts` 已生成。

### 6.2 前端持久化对齐 ✅
1. `useAppPersistence.ts` — `LAST_RESORT_DEFAULTS` 加 `locale: "zh-CN"`
2. `normalizeSettings.ts` — `LOCALES` 常量 + `pickEnum`
3. `useSettingsController.ts` — `SettingsForm`/`settingsToForm`/`saveSettings` draft 加 `locale`；保存成功后若 `locale` 变化调 `changeAppLanguage(nextSettings.locale)` 即时切换
4. `useAppPersistence.ts` 启动流程 — settings 加载后调 `changeAppLanguage(settings.locale)`（含兜底路径的 `defaults.locale`）

### 6.3 设置弹窗 UI ✅
`SettingsModal.tsx` 顶部（标题下方、tab 导航上方）加 `settings:locale.label` 下拉，使用 `SUPPORTED_LOCALES` + `AppLocale` 类型。`locale.label` 已加入 settings.json：zh-CN `"界面语言"` / en `"Interface language"`。

---

## 7. 阶段 4 ✅ 收尾 — 已完成

- **`UpdateModal.tsx` 的 `formatDateRelative`** ✅：移除 `Asia/Shanghai` 硬编码——GitHub `publishedAt` 是 ISO 8601 UTC 时间戳，`Date.now()` 与 `date.getTime()` 都是 UTC，直接相减即得正确 elapsed，无需时区转换。绝对日期兜底改用 `date.toLocaleDateString(i18n.language, ...)` 跟随界面语言。相对时间词（`刚刚发布`/`X 分钟前` 等）已走 `updater.relative.*` 字典。
- **`index.html`** ✅：`<html lang="en">` 改成 `<html lang="zh-CN">`（跟随默认 UI 语言；运行时的动态修正仍保留）。
- **`window.confirm`/`window.alert`** ✅：`useTaskLogs.ts` 的确认框已用 `t("tasks.logs.clearConfirm")` 本地化。
- **全量 lint** ✅：0 错误（17 个 `exhaustive-deps` 预先存在的 warning）。修复了一个真问题：`AppErrorBoundary.tsx` 的匿名默认导出（改用命名 const 包裹 `withTranslation`）；并关闭了 `react-hooks/preserve-manual-memoization`（本项目用 `@vitejs/plugin-react` 标准 babel fast-refresh，**未使用 React Compiler**，该规则无编译器时会对合法手动的 `useMemo`/`useCallback` 报假阳性）。
- **测试** ✅：frontend 130/130，backend 198/198，tsc 清洁。

**剩余**：手测中英双语回归（`npm run dev` 检查各界面 + 切换语言下拉验证即时切换与重启持久化）。

---

## 8. 验证命令速查

```bash
# 前端
npx tsc -b                    # 类型检查
npx eslint src/               # lint
npm run test                  # vitest
npm run dev                   # 开发服务器（肉眼检查）

# 后端
cargo check -p voxtrans       # 编译检查
cargo test -p voxtrans        # 测试
npm run generate:types        # 重新生成 TS 绑定（改了 Rust 结构后必跑）

# 统计后端剩余中文（排除注释）—— 监控进度用
python -c "
import os,re
pat=re.compile(r'[\u4e00-\u9fff]')
tot=0
for root,_,files in os.walk('src-tauri/src'):
  for fn in files:
    if not fn.endswith('.rs'): continue
    try: txt=open(os.path.join(root,fn),encoding='utf-8').read()
    except: continue
    for line in txt.splitlines():
      s=line.lstrip()
      if s.startswith('//') or s.startswith('*'): continue
      if '//' in line: line=line.split('//')[0]
      tot+=len(pat.findall(line))
print('backend code Chinese chars (excl comments):',tot)
"
```

---

## 9. 建议的接手执行顺序

1. **先读完本文档 §2 决策**（尤其决策 4 的"不能动"清单）。
2. **继续阶段 1**：按 §4.1 清单逐文件转换，每改 2-3 个文件跑一次 `cargo check`。先转 emit message（零风险），再转服务层 Err，最后转命令层 map_err。`db/store.rs:493` 别忘了。
3. **阶段 1 完成后**：`cargo test` 全绿，后端零用户可见中文。
4. **进入阶段 2**：先改 `errors.ts` + 填 `errors.json`，再逐组件抽取。这是最长阶段。
5. **阶段 3、4** 见 §6、§7。

每个阶段结束建议 `git commit`，保持可回滚。

---

## 10. 当前未提交改动清单（git status 摘要）

**已修改（9 个文件）**：`package.json`、`package-lock.json`、`src/main.tsx`、`src-tauri/src/domain/{error,mod,task/stage}.rs`、`src-tauri/src/commands/workspace/translation_flow.rs`、`src-tauri/src/services/{file_download,youtube}.rs`

**新增（未跟踪）**：`src-tauri/src/domain/app_error.rs`、`src/i18n/` 整个目录（index.ts、resources.ts、16 个 JSON）

**建议**：接手后先 `git add -A && git commit -m "feat(i18n): scaffold frontend i18n + begin backend string conversion"` 保存当前进度，再继续。
