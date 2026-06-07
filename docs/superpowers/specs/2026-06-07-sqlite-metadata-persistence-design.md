# VoxTrans SQLite 持久化 V1 — 元数据层

## Goal

把 `settings.json` 和 `task_meta.json` 切换到 SQLite，DB 作为唯一真源。无破坏性兼容、不做迁移、不改用户可见行为。这一版建立项目级的 SQLite 建表准则，作为后续持久化改造（artifacts 记账、断点续传、模型下载历史等）的规范基础。

## Scope

### 本版接管

| 类别 | 当前位置 | 接管后 |
|---|---|---|
| 应用设置 | `app_data_dir/settings.json` | 删除文件，改读/写 `settings` 表 |
| 任务元数据 | `output/<stem>_<task_id>/task_meta.json` | 删除文件，改读/写 `tasks` 表 |
| 字幕段 | 嵌套在 `task_meta.json` 的 `subtitle_segments_json` | 拆 `subtitle_segments` + `subtitle_words` 两表 |
| 导出项枚举 | 嵌套在 `settings.json` 的 `flat_srt_items` | 拆 `flat_srt_items` 子表 |
| 术语库 | 嵌套在 `settings.json` 的 `terminology_groups` | 拆 `terminology_groups` + `terminology_terms` 两表 |

### 本版不动

- B2 pipeline 中间产物（`artifacts/step_*.json`）—— 文件照写，pipeline step 零改动
- B3 任务日志（`artifacts/logs/*.log`）
- B4 LLM 缓存（`artifacts/gpt.log`）
- C 模型文件
- D SRT 产物（不存 DB）
- E 用户原始音视频

### 不做迁移

- 启动时**完全不读** `settings.json` / `task_meta.json`
- 第一次启动 DB 为空 = 无设置 + 无任务
- 用户的旧数据视为丢失

### 行为不变

- 用户上传任务、点启动、看进度、看错误 —— API / UI 行为不变
- `WorkspaceQueueItem`、`SavedSettings` 形态不变（前端无感）
- 状态字段 5 个值（`""` / `queued` / `processing` / `done` / `error`）保持
- 启动时 `processing → error` 中断修复逻辑保留（在 DB 上做）

## 建表准则（项目级，本版及后续复用）

### 准则 1：永远不用 JSON 列

无论 TEXT 存 JSON 还是 SQLite 真 JSON1 扩展，禁。

**理由**：JSON 列把 schema 演进权交给"业务对象结构"，DB 层失去约束能力。后续要做索引、约束、外键都难。

### 准则 2：1:1 嵌套 → 平铺到父表，前缀消歧

子对象的所有字段直接平铺到父表，列名加嵌套路径作为前缀（SQLite 列名不允许点，用下划线连接）。

**示例**：

- `SavedSettings.subtitle_render_style.source.font_family` → `settings.source_font_family`
- `WorkspaceQueueItem.task_progress.stage.code` → `tasks.task_progress_stage_code`

平铺到最底层，**不停**。

### 准则 3：1:N 嵌套 → 拆子表 + 外键 + CASCADE

无差别拆表，不论 N 多大、不论是否"附属"。

**示例**：

- `SavedSettings.terminology_groups → Vec<TerminologyGroup>` → `terminology_groups` + `terminology_terms` 两表
- `Task.segments → Vec<SubtitleSegment>` → `subtitle_segments` 表
- `SubtitleSegment.words → Vec<SubtitleWord>` → `subtitle_words` 表（不豁免"附属"）

**子表最小列**：

- `id` TEXT PK（业务 ID 不用自增）
- 父表 id 字段 TEXT NOT NULL + FK ON DELETE CASCADE
- `idx` INTEGER（顺序，ORDER BY 用）
- 业务字段全散列
- `updated_at` INTEGER NOT NULL

**外键**：依赖 `PRAGMA foreign_keys = ON`（`db/mod.rs` 已开）。

### 准则 4：列表 / 枚举值 → 1:N 子表

`Vec<T>` 必须拆子表。**不用**逗号分隔字符串、**不用**布尔列展开。

**示例**：

- `flat_srt_items: Vec<String>` → `flat_srt_items` 子表
- 反例：4 个 `export_source` / `export_target` 布尔列，违反准则 4

**例外**：固定枚举的真值（`subtitle_burn_mode: String`，值域 4 个固定字符串）→ 1:1 平铺成 1 列 TEXT（CHECK 约束可选），不拆表。

### 准则 5：所有表都有 `updated_at` INTEGER NOT NULL（毫秒时间戳）

### 准则 6：单行配置表用 `id INTEGER PRIMARY KEY CHECK (id = 1)`

`settings` 表用此模式。`CHECK` 约束保证永远只有 1 行。

## Architecture

### 数据流

```
启动：
  db::init_pool (已有)
    → init_pool 已跑 migrations
    → services/preferences::load_saved_settings_from_default_path
        → SELECT * FROM settings (1 行)
        → SELECT * FROM flat_srt_items (拼成 Vec<String>)
        → SELECT * FROM terminology_groups + JOIN terminology_terms
        → 拼回 SavedSettings
    → commands/workspace::hydrate_workspace_from_disk
        → 改名 hydrate_workspace_from_db
        → SELECT * FROM tasks ORDER BY updated_at DESC
        → 残留 processing → error 修复（保留原逻辑）
        → SELECT segments + words 拼成 subtitle_segments_json 字符串
        → 装回 WorkspaceQueueItem（前端拿到的还是同一形态）

运行时：
  queue_ops::enqueue / patch_task_item
    → 写内存 TaskStore（前端 push 用）
    → 同时 INSERT/UPDATE tasks 表
    → segments/words 单独写子表

  execution_flow 读 settings_snapshot
    → SELECT settings_snapshot_* 列 → 拼回 JSON Value

  services::preferences::save_app_settings
    → 写 settings 表 1 行（UPSERT id=1）
    → 删 + 重写 flat_srt_items 行
    → 删 + 重写 terminology_groups + terminology_terms 行
```

### 内存态与 DB 态的关系

| 维度 | 内存态（TaskStore）| DB 态（tasks 表）|
|---|---|---|
| 用途 | 前端 event push 推 | 持久化、启动重建 |
| 读路径 | 前端读：O(1) 内存访问 | 启动读：SELECT 全表 |
| 写路径 | 业务代码改 → 同时写 DB | 启动时不写 |
| 一致性 | 写双份；写 DB 失败要回滚内存 | — |

**前端读路径不变**：仍从 `TaskStore` 拿，不查 DB。

### 后端合并点

`load_saved_settings_from_default_path` 改造为：

1. `SELECT * FROM settings WHERE id = 1` → 拼成 `SavedSettings`（无 `terminology_groups`、无 `flat_srt_items`）
2. `SELECT value FROM flat_srt_items` → `Vec<String>` 装回 `flat_srt_items`
3. `SELECT * FROM terminology_groups` + `SELECT * FROM terminology_terms` → 按 group_id 拼成 `Vec<TerminologyGroup>`
4. 装回 `SavedSettings.terminology_groups`
5. 返回

`save_app_settings` 改造为：

1. UPSERT `settings` 行（id=1）
2. `DELETE FROM flat_srt_items` → INSERT 选中值
3. `DELETE FROM terminology_groups`（CASCADE 删 terms）→ INSERT groups → INSERT terms

## Schema

7 张表。

### `settings`（单行，1:1 全平铺）

| 列 | 类型 | 来源 |
|---|---|---|
| `id` | INTEGER PRIMARY KEY CHECK (id = 1) | 强制单行 |
| `provider` | TEXT NOT NULL | `SavedSettings.provider` |
| `chunk_target_seconds` | INTEGER NOT NULL | |
| `subtitle_length_preset` | TEXT NOT NULL | |
| `asr_model` | TEXT NOT NULL | |
| `align_model` | TEXT NOT NULL | |
| `demucs_model` | TEXT NOT NULL | |
| `enable_vocal_separation` | INTEGER NOT NULL | bool |
| `translate_api_key` | TEXT NOT NULL | |
| `translate_base_url` | TEXT NOT NULL | |
| `translate_model` | TEXT NOT NULL | |
| `llm_concurrency` | INTEGER NOT NULL | |
| `enable_terminology` | INTEGER NOT NULL | |
| `enable_subtitle_beautify` | INTEGER NOT NULL | |
| `enable_click_sound` | INTEGER NOT NULL | |
| `auto_burn_hard_subtitle` | INTEGER NOT NULL | |
| `subtitle_burn_mode` | TEXT NOT NULL | 固定枚举，平铺 1 列 |
| `source_font_family` | TEXT NOT NULL | `SubtitleRenderStyle.source.*` |
| `source_font_size` | INTEGER NOT NULL | |
| `source_primary_color` | TEXT NOT NULL | |
| `source_outline_color` | TEXT NOT NULL | |
| `source_back_color` | TEXT NOT NULL | |
| `source_outline` | REAL NOT NULL | |
| `source_shadow` | REAL NOT NULL | |
| `source_border_style` | TEXT NOT NULL | |
| `source_border_opacity` | INTEGER NOT NULL | |
| `target_font_family` | TEXT NOT NULL | `SubtitleRenderStyle.target.*` |
| `target_font_size` | INTEGER NOT NULL | |
| `target_primary_color` | TEXT NOT NULL | |
| `target_outline_color` | TEXT NOT NULL | |
| `target_back_color` | TEXT NOT NULL | |
| `target_outline` | REAL NOT NULL | |
| `target_shadow` | REAL NOT NULL | |
| `target_border_style` | TEXT NOT NULL | |
| `target_border_opacity` | INTEGER NOT NULL | |
| `margin_v` | INTEGER NOT NULL | `SubtitleRenderStyle.layout.*` |
| `alignment` | INTEGER NOT NULL | |
| `bilingual_line_gap` | INTEGER NOT NULL | |
| `flat_srt_output` | INTEGER NOT NULL | |
| `updated_at` | INTEGER NOT NULL | |

合计 40 列。

### `flat_srt_items`（settings 1:N 子表）

| 列 | 类型 |
|---|---|
| `id` | INTEGER PRIMARY KEY AUTOINCREMENT |
| `value` | TEXT NOT NULL UNIQUE |
| `updated_at` | INTEGER NOT NULL |

典型值：`source` / `target` / `bilingualSourceFirst` / `bilingualTargetFirst`。

### `tasks`（1:1 全平铺）

| 列 | 类型 | 来源 |
|---|---|---|
| `id` | TEXT PRIMARY KEY | `WorkspaceQueueItem.id` |
| `media_path` | TEXT NOT NULL | |
| `name` | TEXT NOT NULL | |
| `media_kind` | TEXT NOT NULL | |
| `size_bytes` | INTEGER NOT NULL | |
| `source_lang` | TEXT NOT NULL | |
| `target_lang` | TEXT NOT NULL | |
| `transcribe_status` | TEXT NOT NULL | |
| `task_progress_stage_code` | TEXT NOT NULL DEFAULT '' | `task_progress.stage.*` 平铺 |
| `task_progress_stage_label` | TEXT NOT NULL DEFAULT '' | |
| `task_progress_stage_order` | INTEGER NOT NULL DEFAULT 0 | |
| `task_progress_detail` | TEXT NOT NULL DEFAULT '' | |
| `task_progress_current` | INTEGER NOT NULL DEFAULT 0 | |
| `task_progress_total` | INTEGER NOT NULL DEFAULT 0 | |
| `transcribe_error` | TEXT NOT NULL DEFAULT '' | |
| `result_text` | TEXT NOT NULL DEFAULT '' | |
| `result_srt` | TEXT NOT NULL DEFAULT '' | |
| `llm_total_tokens` | INTEGER NOT NULL DEFAULT 0 | |
| `intent` | TEXT NOT NULL DEFAULT '' | wrapper 字段 |
| `max_retries` | INTEGER NOT NULL DEFAULT 0 | |
| `settings_snapshot_provider` | TEXT NOT NULL DEFAULT '' | `settings_snapshot` 拆 10 列 |
| `settings_snapshot_asr_model` | TEXT NOT NULL DEFAULT '' | |
| `settings_snapshot_align_model` | TEXT NOT NULL DEFAULT '' | |
| `settings_snapshot_demucs_model` | TEXT NOT NULL DEFAULT '' | |
| `settings_snapshot_translate_api_key` | TEXT NOT NULL DEFAULT '' | |
| `settings_snapshot_translate_base_url` | TEXT NOT NULL DEFAULT '' | |
| `settings_snapshot_translate_model` | TEXT NOT NULL DEFAULT '' | |
| `settings_snapshot_llm_concurrency` | INTEGER NOT NULL DEFAULT 0 | |
| `settings_snapshot_chunk_target_seconds` | INTEGER NOT NULL DEFAULT 0 | |
| `settings_snapshot_enable_vocal_separation` | INTEGER NOT NULL DEFAULT 0 | |
| `updated_at` | INTEGER NOT NULL | |

合计 31 列。

**`settings_snapshot` 拆解说明**：原本存为 `serde_json::Value`。按准则 2 平铺为 10 个 `settings_snapshot_*` 列（取自 `execution_flow.rs:37` 实际读哪些字段）。读时拼回 `serde_json::Value`，写时拆字段。

### `subtitle_segments`（tasks 1:N 子表）

| 列 | 类型 |
|---|---|
| `id` | TEXT PRIMARY KEY |
| `task_id` | TEXT NOT NULL, FK → `tasks.id` ON DELETE CASCADE |
| `idx` | INTEGER NOT NULL |
| `start_ms` | INTEGER NOT NULL |
| `end_ms` | INTEGER NOT NULL |
| `source_text` | TEXT NOT NULL DEFAULT '' |
| `translated_text` | TEXT NOT NULL DEFAULT '' |
| `updated_at` | INTEGER NOT NULL |

### `subtitle_words`（segments 1:N 子表，无豁免）

| 列 | 类型 |
|---|---|
| `id` | TEXT PRIMARY KEY |
| `segment_id` | TEXT NOT NULL, FK → `subtitle_segments.id` ON DELETE CASCADE |
| `idx` | INTEGER NOT NULL |
| `start_ms` | INTEGER NOT NULL |
| `end_ms` | INTEGER NOT NULL |
| `word` | TEXT NOT NULL |
| `updated_at` | INTEGER NOT NULL |

### `terminology_groups`

| 列 | 类型 |
|---|---|
| `id` | TEXT PRIMARY KEY |
| `name` | TEXT NOT NULL |
| `updated_at` | INTEGER NOT NULL |

### `terminology_terms`

| 列 | 类型 |
|---|---|
| `id` | TEXT PRIMARY KEY |
| `group_id` | TEXT NOT NULL, FK → `terminology_groups.id` ON DELETE CASCADE |
| `origin` | TEXT NOT NULL |
| `target` | TEXT NOT NULL |
| `note` | TEXT NOT NULL DEFAULT '' |
| `updated_at` | INTEGER NOT NULL |

## Indexes

| 表 | 索引 | 用途 |
|---|---|---|
| `tasks` | `(transcribe_status)` | 启动时 `processing → error` 修复 |
| `tasks` | `(updated_at DESC)` | 任务列表排序 |
| `tasks` | `(source_lang, target_lang)` | 按语言过滤 |
| `tasks` | `(media_kind)` | 按媒体类型过滤 |
| `subtitle_segments` | `(task_id, idx)` | 按任务拼装 |
| `subtitle_words` | `(segment_id, idx)` | 按段拼装 |
| `terminology_terms` | `(group_id)` | 拼 `SavedSettings.terminology_groups` |
| `flat_srt_items` | UNIQUE `(value)` | 重复值防护 |

## Migration

**目录**：`src-tauri/migrations/`（相对 `src-tauri/Cargo.toml`）

**文件**：`20260607000001_init.sql`（含 7 张表 + 8 个索引）

**执行**：`db/mod.rs` 现有的 `sqlx::migrate!("./migrations").run(&pool)` 已处理。本版**不写**新代码调用 migration。

## Component Changes

### 新增

| 路径 | 作用 |
|---|---|
| `src-tauri/migrations/20260607000001_init.sql` | 7 表 + 8 索引 |
| `src-tauri/src/db/store.rs` | 集中所有 DB 操作函数（settings / tasks / segments / words / terminology）|
| `src-tauri/src/db/models.rs` | 4 个 row struct：SettingsRow / TaskRow / SubtitleSegmentRow / SubtitleWordRow / TerminologyGroupRow / TerminologyTermRow / FlatSrtItemRow |
| `src-tauri/src/db/conversion.rs` | row ↔ 业务对象 转换函数 |

### 修改

| 路径 | 改动 |
|---|---|
| `src-tauri/src/db/mod.rs` | `init_pool` 已有，加 `pub fn store(pool: &SqlitePool) -> TaskStore` 工厂 |
| `src-tauri/src/services/preferences.rs` | `load_saved_settings_from_default_path` / `load_settings` / `save_app_settings` 改为走 store |
| `src-tauri/src/commands/workspace/meta.rs` | `persist_task_meta` / `remove_task_meta` / `load_task_meta_artifacts` / `ensure_workspace_hydrated_from_disk` 改为走 store；`hydrate_workspace_from_disk` 改名为 `hydrate_workspace_from_db` |
| `src-tauri/src/commands/workspace/queue_ops.rs` | 任务 CRUD 后同步写 DB（含 segments/words 写子表）|
| `src-tauri/src/commands/workspace/execution_flow.rs` | 任务执行时通过 store 写 status / progress；读 `settings_snapshot` 走 store |
| `src-tauri/src/commands/workspace/preview.rs` / `output_completion.rs` / `save_subtitle_editor` | segments 写库（同时写 segments + words 子表） |
| `src-tauri/src/main.rs` | 启动时建 store，注入到 commands |

### 不动

| 路径 | 原因 |
|---|---|
| `commands/workspace/pipeline_steps/*.rs` | artifact 文件读写不变 |
| `services/llm/cache.rs` | gpt.log 路径不变 |
| `services/task_path.rs` | 任务目录结构不变 |
| `services/subtitle_srt.rs` | SRT 导出不变 |
| `services/model/`、`services/file_download.rs`、`services/youtube.rs` | 与本版无关 |
| `services/output.rs` | `resolve_output_dir` 不变（artifacts 还写在那） |
| 前端 `src/` | API 形态不变，前端无感 |

## Error Handling

### DB 写失败

- 写 DB 失败（迁移失败、约束冲突、连接断开）→ 返回错误给上层
- 内存态与 DB 态可能短暂不一致 → 重启时从 DB 重建，DB 是真值
- 不做自动重试；上层决定如何处理（提示用户 / 退避重试 / 终止任务）

### 启动时

- `db::init_pool` 失败 → 应用启动失败（与现状一致：`init_pool` 失败已返回 String）
- 迁移失败 → 应用启动失败
- 启动时不读任何 JSON 文件，零路径可能

### Schema 演进（未来）

- 新建 `20260608000001_xxx.sql` 加列/表
- `sqlx::migrate!` 跟踪执行历史
- 旧用户升级时自动跑新迁移
- **不在本版范围**

## Testing

### 单元测试

| 模块 | 测试 |
|---|---|
| `db/store.rs` | 每个 CRUD 函数 happy path + 错误 path |
| `db/conversion.rs` | row ↔ 业务对象 round-trip（settings、tasks、segments 含 words 嵌套）|
| `services/preferences.rs` | 改 `load_saved_settings_from_default_path` 后语义不变（用 store mock 或临时 DB）|
| `commands/workspace/meta.rs` | `hydrate_workspace_from_db` 行为同原 `hydrate_workspace_from_disk`（用临时 DB）|

### 集成测试

- 临时 `SqlitePool`（`:memory:` 或 temp file）跑 7 表迁移
- 写入 → 读回 → 比对业务对象一致
- `processing → error` 启动修复逻辑可重现

### 端到端验证（手测）

1. 全新安装（DB 为空）→ 启动 → 上传任务 → 跑完 → 关闭 → 重开 → 任务在
2. 改设置（启用术语库、加术语组）→ 关闭 → 重开 → 设置在
3. 跑任务到一半 → 关闭 → 重开 → 任务状态 `error`（按现状行为）
4. 编辑字幕段 → 关闭 → 重开 → 段编辑在
5. 老的 `settings.json` / `task_meta.json` 残留 → 重启时被忽略（不导入、不报错）

## Out of Scope

- 老的 `settings.json` / `task_meta.json` 导入（破坏性升级）
- pipeline 中间产物（artifacts）记账 —— 留待后续版本
- 断点续传改造 —— 留待后续版本
- 模型下载历史进 DB —— 留待后续版本
- YouTube 历史进 DB —— 留待后续版本
- 任务日志进 DB —— 留待后续版本
- LLM 缓存进 DB —— 留待后续版本
- SRT 产物进 DB —— 不进（你确认）

## Future Considerations

按建表准则，后续改造（artifacts 记账、断点续传、模型下载历史等）可以无脑套用本规范：

- artifacts 记账：`task_artifacts` 子表（task_id FK） + 索引 `(task_id, step_name)`
- 断点续传：`task_checkpoints` 子表（task_id FK，cursor 列用 TEXT 或 INTEGER 看具体语义）
- 模型下载历史：`model_downloads` 独立表
- YouTube 历史：`youtube_history` 独立表
- 任务日志：日志量级大，**单独建库**（`voxtrans_logs.db`）避免主库膨胀 —— 但本版不建，留待决策

## Open Questions

无。
