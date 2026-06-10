# VoxTrans

本地化音视频转录与字幕翻译工具，支持多语种转录与互译。

> 原帖发布于 [52破解论坛](https://www.52pojie.cn/thread-2099216-1-1.html) · 更新记录见 [Releases](https://github.com/eclipse005/VoxTrans/releases)

## 下载

- [GitHub Releases](https://github.com/eclipse005/VoxTrans/releases)（CPU / CUDA 版本）
- 蓝奏云: <https://wwbxg.lanzouq.com/b00jf5bdch> 密码: `h3oe`

## 功能

- **多语种转录**：英语、中文普通话、粤语、日语、韩语、法语、德语、意大利语、西班牙语、葡萄牙语
- **多语种翻译**：18 种目标语言（简中、繁中、英、日、韩、法、德、西、意、葡、俄、阿、越、泰、印尼、土、荷、波）
- **人声分离**：内置 Demucs，降低背景音乐对转录的干扰
- **字幕导出**：原文 / 译文 / 双语（原文在上）/ 双语（译文在上）共 4 种 SRT 格式
- **字幕编辑器**：逐句校对与微调
- **硬字幕压制**：烧录到视频，可调字体大小
- **断点续跑**：任务进度自动持久化，中途关闭不丢失，支持失败重试
- **YouTube 支持**：粘贴链接即可导入

## 使用

1. 设置 → 模型下载，下载 ASR 模型（Qwen3-ASR-0.6B 或 1.7B）和对齐模型（Qwen3-ForcedAligner-0.6B）
2. 选择源语言与目标语言，导入文件或粘贴 YouTube 链接
3. 等待任务完成，在编辑器中微调，导出字幕

人声分离可选（需下载 htdemucs_ft 模型）。有 NVIDIA 显卡可启用 CUDA 加速，显著缩短处理时间。

## 构建

**前置依赖：** Node.js 18+ · Rust 1.75+ · [VS2022 Build Tools](https://visualstudio.microsoft.com/downloads/)（C++ 桌面开发）

**获取二进制工具：**

```powershell
.\scripts\download-binaries.ps1
```

或手动下载放入 `src-tauri/bin/`：ffmpeg ([gyan.dev](https://www.gyan.dev/ffmpeg/builds/))、yt-dlp ([GitHub](https://github.com/yt-dlp/yt-dlp/releases))、fireredvad / demucs（[本仓库 Releases](https://github.com/eclipse005/VoxTrans/releases)）。

**构建命令：**

```powershell
npm install
npm run tauri build                  # CPU 版本
npm run tauri build -- --features cuda  # CUDA 版本（需 CUDA Toolkit）
```

安装包输出到 `target\release\bundle\nsis\`。

## 许可

MIT License
