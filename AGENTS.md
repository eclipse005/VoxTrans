# AGENTS.md

VoxTrans —— 本地离线的音视频转字幕桌面应用：ASR（Qwen3-ASR / Cohere / MOSS，含 CTC
时间轴对齐）、LLM 翻译、字幕编辑与 SRT 导出全在本机完成。Tauri 2 + React 19 + Rust
workspace；发布目标是 Windows x64，出 CPU / CUDA 两个安装包。

这是给 AI 代理和协作者的执行说明。**改代码前先读 `docs/lessons.md`**——AGENTS.md 是
规矩，lessons 是每条规矩的来历，新规矩先在那边攒证据。

## 常用命令

- 前端开发：`npm run dev`；桌面开发：`npm run tauri dev`（CUDA：`npm run tauri:dev:cuda`）
- 提交前检查（都要干净）：
  - `npm run lint`（eslint）、`npm run test`（vitest）、`npm run build`（tsc -b + vite build）
  - `cargo clippy --workspace --all-targets`（保持 0 警告）
  - `cargo test -p voxtrans -p voxtrans-core`
    （**两个包都要点名**：只写 `-p voxtrans` 时 `voxtrans-core` 的测试进不了视野）
- 类型导出：`npm run generate:types`（ts-rs → `src/generated/`；生成物不手改、不提交）
- CUDA 特性编译检查：`npm run cargo:check:cuda`
- 无头 CLI：`cargo run -p voxtrans --bin voxtrans-cli -- --help`
  （用法与设计见 `src-tauri/src/cli/README.md`）
- 发版打包：`.\build-release.ps1`（本地出 CPU + CUDA 安装包）

## 项目结构

- `src/` —— React 前端。`app/`（components / hooks / state / styles / api）、
  `features/media/`（字幕等域逻辑）、`i18n/locales/`（zh-CN 与 en 两份 JSON，逐键成对）、
  `generated/`（ts-rs 生成，勿手改）
- `src-tauri/` —— Tauri 壳与业务服务。`domain/`（纯领域模型/状态）、`services/`
  （transcribe、translation、llm、terminology_agent、demucs 等）、`commands/`（IPC 面）、
  `db/`（SQLite 存储）、`cli/`（无头 CLI）
- `voxtrans-core/` —— 后端无关的纯逻辑（VAD 分段、音频读取、字幕切分……），依赖最少，
  新算法优先落在这里写单测
- `scripts/` —— `download-binaries.ps1`（ffmpeg / yt-dlp / demucs → `src-tauri/bin/`）、
  `tauri-cuda.ps1`、`apply-cuda-runtime.ps1`
- 模块级单一出处：`src-tauri/src/cli/README.md` 与
  `src-tauri/src/services/translation/README.md`——改这两个模块先改对应文档，再改代码

## 代码风格与提交

每条规矩底下都有具体的翻车，来历与测量在 `docs/lessons.md`，不是审美偏好。

- 提交信息：conventional commits + 英文简述（例：
  `perf(subtitle-editor): skip layout for off-screen cue rows`）。正文写清"为什么"和
  用户能感知的影响，不写"优化代码"这类空话；性能改动带测量数字
- **验证边界**：正文末行写 `Verified:`（跑了什么：测试数、lint、构建），没验的部分写
  `Not verified:`（例：`Not verified: real audio MaxTokens path (needs a triggering clip)`）。
  照实写，没验的明说——别让"假绿"变成没人知道
- **新测试要断骨验证**：写完把被测的修复还原（或手动破坏它守护的行为），测试必须变红
  才算数；在旧代码上照样绿的测试等于没写
- **修一类，不修一个**：改 bug 前先 grep 同构的兄弟位置，一次改完
- **分层是机械的**：`domain/` 与 `db/` 不许引用 `services::` / `commands::`——
  `src-tauri/src/layering.rs` 的测试会红；往里加代码前先看那个文件头
- **结构性数字只有一个出处**：列清单 / VALUES 占位符 / 批大小 / 契约常量这类"手抄一遍"
  的东西必须收敛到单一来源（同一常量、同一函数），并配守护测试；改动前 grep 其余手抄处
  （`62ac344`、`62501b4` 都栽在这上面，见 lessons 第 2 条）
- 界面文案必须双语成对：用户可见字符串走 `src/i18n/locales/`（zh-CN 与 en 同一提交内成对
  新增），组件里不硬编码中文/英文；插值占位符（`{{count}}` 等）两侧一致
- 新增行为要带测试：前端行为进 vitest，纯逻辑进 `voxtrans-core` 单测

## 发布流程

- `github-release.md` 顶部的「未发布」段是面向用户的发布说明**单一出处**：发版时整理为
  `## v<版本>` 段落。写"用户能感觉到什么"，不搬运提交原文
- 版本号目前有三处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`——
  改版本三处一起改（`build-release.ps1` 目前也代写这三处）。计划加 `verify-version` 脚本
  收敛到单一来源，落地前照现状来
- 打包走本地 `build-release.ps1`；`.github/` 目录被 `.gitignore` 忽略（workflow 未上库），
  不要把它当成已生效的 CI

## 不要提交的东西

- 产物与中间件：`output/`、`release/`、`dist/`、`target/`、`src-tauri/output/`、
  `voxtrans-out/`、`runtime/`
- 二进制：`src-tauri/bin/` 下按 `.gitignore` 清单的 exe（ffmpeg / yt-dlp / demucs / fireredvad）
- 机器生成：`src-tauri/fixtures/`、`src/generated/`
- 本地/内部资料：`.workbuddy/`、`.github/`、`ROADMAP.md`、`CODEBUDDY.md`、`.claude/`、
  `.worktrees/`
- 任何密钥；AI 能力一律由使用者自行配置
