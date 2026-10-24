---
by: Codex GPT-6
date: 2026-10-24
---

# 卸载 Ollama 并移除已发现的本地模型

用户要求移除所有本地模型和 Ollama。卸载 Ollama 0.34.3（卸载器 exit code 0），移除主模型库 `D:\AI\Models\ollama`（约 45.63 GiB）、Ollama 安装目录（约 2.76 GiB）、`%USERPROFILE%\.ollama`、`%LOCALAPPDATA%\Ollama`，并清理用户级 `OLLAMA_*` 环境变量。

移除 AnythingLLM 内嵌 Ollama 的模型库（约 3.07 GiB）和引擎（约 4.83 GiB）；保留 AnythingLLM 的其它存储。另删除 Hugging Face hub 中发现的 6 个模型目录（约 0.91 GiB）、AnythingLLM Xenova 模型、两个 pyannote 权重、Tesseract `eng.traineddata`，以及 Chrome `OptGuideOnDeviceModel`（约 3.98 GiB）。Ubuntu WSL 常见模型目录检查未发现大于 100 MiB 的权重；WSL 虚拟盘保留。

自动审核拒绝过一次同时删除整个 AnythingLLM `storage\models` 和完整 Hugging Face hub 的宽泛命令；之后改为删除经过核实的具体模型目录/权重文件，保留应用和非模型数据。

最终检查：无 Ollama 安装登记、进程、命令或用户级 Ollama 环境变量；所有已识别模型路径不存在。C 盘可用约 256.5 GiB，D 盘约 113.8 GiB。
