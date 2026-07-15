[中文](README.md) · [English](README_EN.md)

# VoxTrans

A localized audio/video transcription and subtitle translation tool. Runs fully offline, with speech recognition for 11 source languages and subtitle translation across 18 languages.

> Originally posted on the [52pojie forum](https://www.52pojie.cn/thread-2099216-1-1.html) · Changelog: [Releases](https://github.com/eclipse005/VoxTrans/releases)

## Download

- [GitHub Releases](https://github.com/eclipse005/VoxTrans/releases) (CPU / CUDA versions)
- Lanzou Cloud: <https://wwbxg.lanzouq.com/b00jf5bdch> Password: `h3oe`

## Features

- **Multilingual transcription**: 11 source languages (English, Mandarin Chinese, Cantonese, Japanese, Korean, French, German, Italian, Spanish, Portuguese, Russian)
- **Multi-engine ASR**: Qwen3-ASR / Cohere Transcribe / MOSS-Transcribe-Diarize, switchable in Settings
- **Multilingual translation**: 18 target languages (Simplified Chinese, Traditional Chinese, English, Japanese, Korean, French, German, Spanish, Italian, Portuguese, Russian, Arabic, Vietnamese, Thai, Indonesian, Turkish, Dutch, Polish)
- **Streaming translation preview**: LLM tokens appear live in the subtitle editor as they generate
- **Vocal separation**: built-in Demucs HT to reduce background-music interference with transcription
- **Subtitle export**: 4 SRT formats — source / target / bilingual (source first) / bilingual (target first)
- **Subtitle editor**: sentence-by-sentence proofreading and fine-tuning
- **Hardsub burn-in**: burn subtitles into the video with adjustable font size
- **Resume from checkpoint**: task progress is persisted automatically; closing midway loses nothing, and failed tasks can be retried
- **YouTube support**: paste a link to import

## Models

Three built-in model types: **ASR (speech recognition)** converts audio to text, the **aligner** produces word-level timestamps, and **vocal separation** (optional) suppresses background music before transcription.

VoxTrans currently enables 11 source languages in the UI (Chinese, English, Cantonese, Japanese, Korean, German, Italian, Spanish, Portuguese, French, Russian). The actually usable languages are limited by the aligner's 11 supported languages: Chinese, English, Cantonese, Japanese, Korean, French, German, Spanish, Italian, Portuguese, Russian.

### ASR Models

| Model | Params | Size | Recommended for |
|------|--------|------|--------|
| **Qwen3-ASR-0.6B** (default) | ~0.6B | ~1.8 GB | balanced accuracy/speed, low-VRAM friendly |
| **Qwen3-ASR-1.7B** | ~1.7B | ~4.7 GB | open-source SOTA accuracy, needs more VRAM |
| **cohere-transcribe-03-2026** | 2B | ~3.9 GB | English, European languages |
| **MOSS-Transcribe-Diarize** | Whisper-M + Qwen3-0.6B | ~1.8 GB | meetings/dialogue; fixed ~180s chunks |

### Aligner (required)

| Model | Params | Size | Supported languages |
|------|--------|------|--------|
| **Qwen3-ForcedAligner-0.6B** | ~0.6B | ~1.8 GB | Chinese, English, Cantonese, Japanese, Korean, French, German, Spanish, Italian, Portuguese, Russian |

The aligner aligns the ASR transcript with the audio to produce word-level timestamps. There is only this one model, shared by all ASR models.

### Vocal Separation (optional)

| Model | Size | Notes |
|------|------|------|
| **htdemucs_ft** | ~349 MB | Hybrid Transformer Demucs v4; separates background music from vocals before transcription to reduce recognition errors caused by BGM |

## Usage

1. Settings → Model Download, download the ASR model and the aligner (first-time users recommended: Qwen3-ASR-0.6B + Qwen3-ForcedAligner-0.6B)
2. Select the source and target languages, then import a file or paste a YouTube link
3. Wait for the task to finish, fine-tune it in the editor, and export the subtitles

Optional: download htdemucs_ft to enable vocal separation. If you have an NVIDIA GPU, enable CUDA acceleration in Settings to significantly shorten processing time.

**CUDA edition requirements:** A recent NVIDIA GPU driver is enough (if you can play games, you are fine). **No CUDA Toolkit install.** The installer downloads CUDA 12.8 compute libraries (cudart / cublas / …) on demand; kernels ship as precompiled multi-arch PTX (no NVRTC).

## Build

**Prerequisites:** Node.js 18+ · Rust 1.75+ · [VS2022 Build Tools](https://visualstudio.microsoft.com/downloads/) (C++ Desktop Development)

Building the CUDA edition also needs CUDA Toolkit 12.x on the **developer** machine only. After changing engine `.cu` sources, run `scripts/compile-ptx.ps1` in each engine repo to regenerate `ptx/`, then bump the git revs here.

**Get the binary tools:**

Download ffmpeg.exe, yt-dlp.exe, and demucs.exe from [Releases (tools)](https://github.com/eclipse005/VoxTrans/releases/tag/tools) and place them in `src-tauri/bin/`.

Or run the script to download them automatically:

```powershell
.\scripts\download-binaries.ps1
```

**Build commands:**

```powershell
npm install
npm run tauri build                  # CPU version
npm run tauri build -- --features cuda  # CUDA version (dev machine needs Toolkit 12.x; end users do not)
```

The installer is output to `target\release\bundle\nsis\`.

## License

MIT License
