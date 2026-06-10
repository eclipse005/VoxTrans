# VoxTrans

VoxTrans 是一个本地化的音视频转录与字幕翻译工具，支持多语种转录与多语种互译；翻译需在设置中配置 LLM 接口参数。

## 下载地址

- 下载: https://wwbxg.lanzouq.com/b00jf5bdch
- 密码: h3oe

## 功能介绍

- 音视频一键转字幕
- 支持多语种转录：英语、中文普通话、粤语、日语、韩语、法语、德语、意大利语、西班牙语、葡萄牙语
- 支持多语种翻译（18 种目标语言，含简中、繁中、英、日、韩、法、德、西、意、葡、俄、阿、越、泰、印尼、土、荷、波）
- 自动优化标点与断句，减少手动整理时间
- 内置人声分离（Demucs），降低背景音乐对转录的干扰
- 支持导入 YouTube 链接或本地文件
- 支持任务进度查看、失败重试与断点续跑
- 内置字幕编辑器，可逐句校对与微调
- 支持多种字幕格式导出：
  - 原文单语（`src.srt`）
  - 译文单语（`trans.srt`）
  - 双语·原文在上（`src_trans.srt`）
  - 双语·译文在上（`trans_src.srt`）
- 支持字幕硬压制（烧录到视频），可在设置中调整字体大小

## 使用步骤

1. 打开设置 → 模型下载，按需下载以下模型：
   - ASR 模型（必选）：Qwen3-ASR-0.6B 或 Qwen3-ASR-1.7B 二选一
   - 对齐模型（必选）：Qwen3-ForcedAligner-0.6B，用于词级时间戳对齐
   - 人声分离模型（可选）：htdemucs_ft，仅在需要"人声分离"功能时下载
2. 在主界面选择源语言与目标语言（不翻译时目标语言可不选）
3. 导入本地音视频文件，或粘贴 YouTube 链接
4. 选择处理方式：仅转录 / 转录 + 翻译，是否启用人声分离
5. 等待任务完成，在字幕编辑器中检查并微调内容
6. 在"导出字幕"弹窗中选择 4 种格式之一，输出 SRT 文件到 `output/` 目录
7. 如需硬字幕压制，在设置 → 字幕样式中调整字体大小后导出视频

## 从源码构建

### 前置依赖

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install) 1.75+
- [Visual Studio 2022 Build Tools](https://visualstudio.microsoft.com/downloads/)（含 C++ 桌面开发工作负载）

### 获取二进制工具

构建前需将以下 exe 放入 `src-tauri/bin/`：

| 文件 | 来源 |
|------|------|
| `ffmpeg.exe` | [gyan.dev FFmpeg builds](https://www.gyan.dev/ffmpeg/builds/)（下载 essentials 版本） |
| `yt-dlp.exe` | [yt-dlp GitHub Releases](https://github.com/yt-dlp/yt-dlp/releases) |
| `fireredvad.exe` | 从 [GitHub Releases](https://github.com/eclipse005/VoxTrans/releases) 下载 |
| `demucs.exe` | 从 [GitHub Releases](https://github.com/eclipse005/VoxTrans/releases) 下载 |

也可以运行脚本自动从最新 Release 下载：

```powershell
.\scripts\download-binaries.ps1
```

### 构建

```powershell
npm install
npm run tauri build          # CPU 版本
npm run tauri build -- --features cuda   # CUDA 版本（需 NVIDIA GPU + CUDA Toolkit）
```

产出的安装包位于 `target\release\bundle\nsis\`。

## 硬件加速

- 默认使用 CPU 运行
- 若有 NVIDIA 显卡，可通过构建脚本启用 CUDA 加速（参考 `scripts/tauri-cuda.ps1`），可显著缩短转录与对齐耗时

## 流程约定（Checkpoint）

任务输出目录按 step 落地中间结果，采用“**文件存在即跳过**”策略。

- `step_01_asr.json`：转录词级时间戳
- `step_02_segments.json`：组句结果
- `step_03_terminology.json`：术语层
- `step_04_translation.json`：翻译草稿层
- `step_05_01_source_split.json`：原文展示断句（带时间戳重映射）
- `step_05_02_translation_align.json`：译文按断句对齐
- `step_05_03_translation_polish.json`：译文压缩/润色（最终同构输出）

重跑规则：

1. 仅重跑 step5：删除 `step_05_01_source_split.json`、`step_05_02_translation_align.json`、`step_05_03_translation_polish.json`
2. 重跑术语+翻译+step5：删除 `step_03_terminology.json`、`step_04_translation.json` 及所有 `step_05_*`
3. 从组句后重跑：删除 `step_02_segments.json`、`step_03_terminology.json`、`step_04_translation.json` 及所有 `step_05_*`
4. 从头重跑：删除 `step_01_asr.json` 到 `step_05_03_translation_polish.json` 全部 step 文件

说明：

- 这是显式可控的断点续跑机制，不做输入指纹自动失效。
- 用户希望重算时，按上述规则手动删除对应 step 文件即可。

## 更新记录

### v1.0.3
- 多语种转录支持（10 种源语言）
- 多语种翻译支持（18 种目标语言）
- 新增人声分离（Demucs）功能
- 字幕导出支持 4 种格式（原文 / 译文 / 双语 × 2）
- 模型架构升级为 ASR + ForcedAligner + Demucs 三套模型

### v0.1.2
- 缺陷修复

### v0.1.1
- 增加字幕压制功能
- 修复翻译后字幕时间戳异常拆分的问题
