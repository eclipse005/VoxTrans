# VoxTrans

VoxTrans 是一个 Windows 英文转录翻译工具，翻译需配置 LLM 相关参数。

## 下载地址

- 下载: https://wwbxg.lanzouq.com/b00jf5bdch
- 密码: h3oe

## 功能介绍

- 音视频一键转字幕（仅支持英文音视频）
- 支持字幕翻译（英译中）
- 自动优化标点与断句，减少手动整理时间
- 支持导入 YouTube 链接或本地文件
- 支持任务进度查看、失败重试与断点续跑
- 内置字幕编辑器

## 使用步骤

1. 设置模型中下载转录模型
2. 导入本地音视频或粘贴 YouTube 链接
3. 选择需要的处理方式（转录或转录+翻译）
4. 等待任务完成，在字幕编辑器中检查并微调内容
5. 处理完成后安装目录 output 中有相关字幕

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

### v0.1.2
- 缺陷修复

### v0.1.1
- 增加字幕压制功能
- 修复翻译后字幕时间戳异常拆分的问题
