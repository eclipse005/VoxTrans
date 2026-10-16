# AGENTS.md

VoxTrans —— 本地离线的音视频转字幕桌面应用：ASR（Qwen3-ASR / Cohere / MOSS，含 CTC
时间轴对齐）、LLM 翻译、字幕编辑与 SRT 导出全在本机完成。Tauri 2 + React 19 + Rust
workspace；发布目标是 Windows x64，出 CPU / CUDA 两个安装包。

这是给 AI 代理和协作者的执行说明。**改代码前先读 `docs/lessons.md`**——AGENTS.md 是
规矩，lessons 是每条规矩的来历，新规矩先在那边攒证据。

## 常用命令

- 前端开发：`npm run dev`；桌面开发：`npm run tauri dev`（CUDA：`npm run tauri:dev:cuda`）
- 提交前检查（本地门禁；CI 会跑同一套，见「CI 与发布」）：
  - `npm run verify:fast` —— 版本一致性、二进制锚点、eslint、vitest、tsc + vite build
  - `npm run verify` —— 以上全部，另加 `cargo clippy --workspace --all-targets -- -D warnings`
    与 Rust 测试 `cargo test -j 1 -p voxtrans -p voxtrans-core`
    （**两个包都要点名**：只写 `-p voxtrans` 时 `voxtrans-core` 的测试进不了视野）
  - 单条等价命令按需单独跑：`npm run lint` / `npm run test` / `npm run build` /
    `npm run verify:version` / `npm run verify:binaries`；Rust 两条同上
- 类型导出：`npm run generate:types`（ts-rs → `src/generated/`，**已入库**：改了 Rust
  侧导出类型就重新生成并连同 diff 一起提交；生成物不手改）
- CUDA 特性编译检查：`npm run cargo:check:cuda`
- 依赖审计：`cargo deny check advisories sources`（配置 `deny.toml`；CI 的 rust job 必跑，
  用的是钉版 0.20.2 + 官方 sha256 校验的二进制。本机网络对 github HTTPS 不通时加
  `--offline` 复用已缓存的 advisory-db）
- 无头 CLI：`cargo run -p voxtrans --bin voxtrans-cli -- --help`
  （用法与设计见 `src-tauri/src/cli/README.md`）
- 发版打包：`.\build-release.ps1`（本地出 CPU + CUDA 安装包）

## 项目结构

- `src/` —— React 前端。`app/`（components / hooks / state / styles / api）、
  `features/media/`（字幕等域逻辑）、`i18n/locales/`（zh-CN 与 en 两份 JSON，逐键成对）、
  `generated/`（ts-rs 生成、已入库、勿手改；改 Rust 类型后 `npm run generate:types` 并提交
  diff，CI 有漂移检查）
- `src-tauri/` —— Tauri 壳与业务服务。`domain/`（纯领域模型/状态）、`services/`
  （transcribe、translation、llm、terminology_agent、demucs 等）、`commands/`（IPC 面）、
  `db/`（SQLite 存储）、`cli/`（无头 CLI）
- `voxtrans-core/` —— 后端无关的纯逻辑（VAD 分段、音频读取、字幕切分……），依赖最少，
  新算法优先落在这里写单测
- `scripts/` —— `download-binaries.ps1`（ffmpeg / yt-dlp / demucs → `src-tauri/bin/`，
  下载当场对 GitHub 的 sha256 digest 校验）、`tauri-cuda.ps1`、`apply-cuda-runtime.ps1`；
  门禁：`verify.mjs`（一键）、`verify-version.mjs`、`verify-binaries.mjs` + `binaries.sha256`；
  发布：`release-notes.mjs`（从 `github-release.md` 提取 `## v<版本>` 段，release workflow 用）
- `rust-toolchain.toml` —— **Rust 工具链的唯一出处**：本地、CI、发版都读它（workflow 从文件里
  解析 channel，不另写版本号）。升版是主动行为：改 channel → 本地跑
  `cargo clippy --workspace --all-targets -- -D warnings` 与测试 → 修新编译器/lint 报出的
  问题 → 再推（曾经吃过教训：本地旧版、CI stable 新版，新 lint 只在 CI 上炸）
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
  新增），组件里不硬编码中文/英文；插值占位符（`{{count}}` 等）两侧一致——
  `src/i18n/locales.test.ts` 会拦：命名空间文件、深键、值类型、`{{插值}}` 逐项对齐
- 新增行为要带测试：前端行为进 vitest，纯逻辑进 `voxtrans-core` 单测

## CI 与发布

- **CI**（`.github/workflows/ci.yml`）：push 到 `main`、或开 PR 时自动跑（`main` 是唯一
  工作分支，直接提交，不走过第二条线）。
  `static`（Linux：版本一致性、locale 守护、eslint、vitest、tsc + vite build）与
  `rust`（Windows：clippy `-D warnings`、两个包的测试、cargo-deny 依赖审计、
  bindings 漂移检查）。
  CI 是网不是主循环：本地先跑 `npm run verify`（或 `verify:fast`）再推
- **发版**（`.github/workflows/release.yml`，tag 触发）：
  1. `github-release.md` 顶部「未发布」段改名为 `## v<版本>`（发布说明单一出处，
     写"用户能感觉到什么"）；
  2. 三处版本号改成同值（`build-release.ps1` 的 Step 1 会一起写），`npm run verify:version` 确认；
  3. commit → `git tag v<版本>` → push tag；
  4. workflow 断言 tag == 版本 → 构建 CPU 与 CUDA 安装包（CUDA 走 `apply-cuda-runtime.ps1`
     的 NSIS 补丁）→ 用发布说明段建 Release，挂 `VoxTrans_<版本>_cpu.exe` / `_cuda.exe`。
  `workflow_dispatch` 只出 artifact、不建 Release（干跑用）；说明段缺失会直接失败
- 本地 `build-release.ps1` 保留为离线/备用路径，产出与 CI 同名的资产；全新克隆直接打包的
  前置是 `scripts/download-binaries.ps1`（`bundle.resources` 引用 `bin/` 下三个 exe）
- 版本号三处用 `scripts/verify-version.mjs` 断言（`--expect` / `--tag` / `--print`）；
  `src-tauri/bin/` 里随包发布的 ffmpeg / demucs 与 `scripts/binaries.sha256` 对锚
  （`npm run verify:binaries`，换文件时把新摘要和文件放同一个提交）。yt-dlp 除外：
  它是滚动更新，两条下载路径都在下载当场对 GitHub API 的 digest 校验

## 不要提交的东西

- 产物与中间件：`output/`、`release/`、`dist/`、`target/`、`src-tauri/output/`、
  `voxtrans-out/`、`runtime/`
- 二进制：`src-tauri/bin/` 下按 `.gitignore` 清单的 exe（ffmpeg / yt-dlp / demucs / fireredvad）
- 机器生成：`src-tauri/fixtures/`
- 本地/内部资料：`.workbuddy/`、`ROADMAP.md`、`CODEBUDDY.md`、`.claude/`、`.worktrees/`
- 任何密钥；AI 能力一律由使用者自行配置
