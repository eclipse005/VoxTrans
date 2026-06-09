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

## 流程约定（断点续跑）

任务元数据和中间结果都持久化到本地 SQLite（`output/voxtrans.db`），
按 step 缓存。重启或任务失败重跑时自动跳过已完成的 step：

- Step 1 ASR：`asr_transcripts` 表，按段缓存识别文本
- Step 1 强制对齐：`alignment_results` 表，按段缓存对齐结果
- Step 2 分句：`task_artifacts` 表，整 step 缓存
- Step 3 术语：`task_artifacts` 表
- Step 4 翻译批次：`translation_batch_results` 表，按 batch 缓存
- Step 5 切分+对齐合并：`step5_split_align_results` 表，按段缓存

重跑某个 step：删除任务后重新入队，所有缓存随 task 级 CASCADE 一起清理。
不需要手动删 step 文件 —— 系统已经不再使用文件作为 source of truth。

## 更新记录

### v0.1.2
- 缺陷修复

### v0.1.1
- 增加字幕压制功能
- 修复翻译后字幕时间戳异常拆分的问题
