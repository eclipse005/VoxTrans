# VoxTrans Release Notes

## v1.0.2

**修复 N 卡用户 CUDA 版启动报"找不到 cudart64_12.dll"等 DLL 缺失问题**

- CUDA 版不再需要装 CUDA Toolkit,装好 NVIDIA 驱动即可
- CUDA Runtime 4 个 DLL 改为安装时从 ModelScope 下载(国内直连,1-3 分钟)
- **目标用户**:之前 CUDA 版用不了的 N 卡用户。CUDA 版之前能正常用的用户无需更新

## v1.0.1

**修复**
- 字幕编辑器修改内容后导出仍是原始版本的问题

## v1.0.0

**新功能**
- 转录引擎从 Parakeet 升级为 Qwen3-ASR
- 新增多语言支持（中英粤日韩法德意西葡等）
- 转录完全离线运行，无需网络
- 新增内置在线更新，后续版本自动检测升级

**说明**
- 翻译仍走 LLM，需自行配置 API
