[English](README_EN.md) · [中文](README.md)

# VoxTrans

本地化音视频转录与字幕翻译工具。完全离线运行，支持 11 种源语言的语音识别，以及 18 种语言的字幕互译。

> 原帖发布于 [52破解论坛](https://www.52pojie.cn/thread-2099216-1-1.html) · 更新记录见 [Releases](https://github.com/eclipse005/VoxTrans/releases)

## 下载

- [GitHub Releases](https://github.com/eclipse005/VoxTrans/releases)（CPU / CUDA 版本）
- 蓝奏云: <https://wwbxg.lanzouq.com/b00jf5bdch> 密码: `h3oe`

## 功能

- **多语种转录**：11 种源语言（英语、中文普通话、粤语、日语、韩语、法语、德语、意大利语、西班牙语、葡萄牙语、俄语）
- **多引擎 ASR**：Qwen3-ASR / Cohere Transcribe / MOSS-Transcribe-Diarize，设置中切换
- **多语种翻译**：18 种目标语言（简中、繁中、英、日、韩、法、德、西、意、葡、俄、阿、越、泰、印尼、土、荷、波）
- **流式翻译预览**：LLM 译文 token 级边生成边显示在字幕编辑器
- **人声分离**：内置 Demucs HT，降低背景音乐对转录的干扰
- **字幕导出**：原文 / 译文 / 双语（原文在上）/ 双语（译文在上）共 4 种 SRT 格式
- **字幕编辑器**：逐句校对与微调
- **硬字幕压制**：烧录到视频，可调字体大小
- **断点续跑**：任务进度自动持久化，中途关闭不丢失，支持失败重试
- **YouTube 支持**：粘贴链接即可导入

## 模型

内置三类模型：**ASR（语音识别）**把音频转成文本，**对齐模型**负责打词级时间戳，**人声分离**（可选）在转录前压制背景音乐。

VoxTrans 当前 UI 启用 11 种源语言（中英粤日韩德意西葡法俄）。实际可用的语言以对齐模型支持的 11 种为限：中、英、粤语、日、韩、法、德、西、意、葡、俄。

### ASR 模型

| 模型 | 参数量 | 体积 | 推荐场景 |
|------|--------|------|----------|
| **Qwen3-ASR-0.6B**（默认） | ~0.6B | ~1.8 GB | 精度与速度平衡，低显存友好 |
| **Qwen3-ASR-1.7B** | ~1.7B | ~4.7 GB | 开源 SOTA 精度，需更多显存 |
| **cohere-transcribe-03-2026** | 2B | ~3.9 GB | 英语、欧洲语言 |
| **MOSS-Transcribe-Diarize** | Whisper-M + Qwen3-0.6B | ~1.8 GB | 会议/对话；固定约 180s 分段 |

### 对齐模型（必需）

| 模型 | 参数量 | 体积 | 支持语言 |
|------|--------|------|----------|
| **Qwen3-ForcedAligner-0.6B** | ~0.6B | ~1.8 GB | 中、英、粤语、日、韩、法、德、西、意、葡、俄 |

对齐模型把 ASR 转录文本与音频对齐，输出词级别时间戳。仅此一个，所有 ASR 模型共享。

### 人声分离（可选）

| 模型 | 体积 | 说明 |
|------|------|------|
| **htdemucs_ft** | ~349 MB | Hybrid Transformer Demucs v4，可在转录前分离背景音乐与人声，降低背景音乐导致的识别错误 |

## 使用
1. 设置 → 模型下载，下载 ASR 模型和对齐模型（首次使用推荐 Qwen3-ASR-0.6B + Qwen3-ForcedAligner-0.6B）
2. 选择源语言与目标语言，导入文件或粘贴 YouTube 链接
3. 等待任务完成，在编辑器中微调，导出字幕

可选：下载 htdemucs_ft 启用人声分离。有 NVIDIA 显卡可在设置中启用 CUDA 加速，显著缩短处理时间。

## 构建

**前置依赖：** Node.js 18+ · Rust 1.75+ · [VS2022 Build Tools](https://visualstudio.microsoft.com/downloads/)（C++ 桌面开发）

**获取二进制工具：**

从 [Releases（tools）](https://github.com/eclipse005/VoxTrans/releases/tag/tools) 下载 ffmpeg.exe、yt-dlp.exe、demucs.exe，放入 `src-tauri/bin/`。

或运行脚本自动下载：

```powershell
.\scripts\download-binaries.ps1
```

**构建命令：**

```powershell
npm install
npm run tauri build                  # CPU 版本
npm run tauri build -- --features cuda  # CUDA 版本（需 CUDA Toolkit）
```

安装包输出到 `target\release\bundle\nsis\`。

## 许可

MIT License
