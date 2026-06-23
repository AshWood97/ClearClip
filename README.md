<div align="center">

<img src="public/tauri.svg" width="80" alt="ClearClip Logo" />

# ClearClip · 无印良片

**本地优先的 AI 视频增强桌面应用**

*去水印 · 4K/2K 超分辨率 · 批量处理*

<br/>

[![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8D8?style=for-the-badge&logo=tauri&logoColor=white)](https://tauri.app)
[![Vue](https://img.shields.io/badge/Vue-3.x-42b883?style=for-the-badge&logo=vue.js&logoColor=white)](https://vuejs.org)
[![Rust](https://img.shields.io/badge/Rust-stable-CE422B?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.x-3178c6?style=for-the-badge&logo=typescript&logoColor=white)](https://www.typescriptlang.org)
[![License](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)](LICENSE)

</div>

---

## ✨ 功能特性

| 工作流 | 说明 | 工作流 ID |
|--------|------|-----------|
| 🚫 **去水印** | 智能识别并移除视频水印 | `2059943323456065537` |
| 🔺 **4K 超分** | AI 驱动的 4K 超分辨率增强 | `2023058520156934145` |
| 🔹 **2K 超分** | 高效 2K 超分辨率增强 | `2023054533156409346` |

- 🖥️ **本地优先** — 所有处理通过 RunningHub AI 工作流完成，无需自建服务器
- 🔒 **API Key 安全存储** — 密钥存入系统凭据，不写入任何本地文件
- 📦 **批量任务** — 支持最多 2 个任务并行运行，任务断点自动恢复
- 📋 **完整任务历史** — 本地持久化任务记录，支持重试和状态追踪
- 🔍 **智能节点检测** — 自动识别工作流中的视频上传节点

---

## 🚀 快速开始

### 1. 获取 RunningHub API Key

前往 [RunningHub](https://www.runninghub.cn) 注册并获取 API Key。

### 2. 配置应用

1. 打开 ClearClip，点击右侧 **设置** 区域
2. 将 API Key 粘贴到「保存并验证 API Key」
3. 应用会自动验证密钥有效性（验证失败时不会保存）

### 3. 开始处理

拖入视频文件 → 选择工作流 → 等待结果自动下载至本地 `results` 目录 🎉

---

## 🏗️ 技术栈

```
ClearClip
├── 前端    Vue 3 + TypeScript + Tailwind CSS v4
├── 动画    GSAP · Motion · Lenis · Tres.js (Three.js)
├── UI      Radix Vue · Lucide Icons
├── 后端    Rust (Tauri v2)
├── 构建    Vite 6 · pnpm
└── 分发    Tauri Updater (自动更新)
```

---

## 🔧 本地开发

```bash
# 安装依赖
pnpm install

# 启动开发服务器（热更新）
pnpm tauri dev

# 类型检查
pnpm exec vue-tsc --noEmit

# Rust 检查
cd src-tauri && cargo check

# 生产构建
pnpm tauri build
```

> **提示**：Vite 已排除监听 `src-tauri/target/`、`node_modules/`、`dist/`，避免 Windows 上 `.pdb` 文件锁死导致开发服务器崩溃。

---

## 📂 结果文件

处理完成后，结果保存至应用数据目录下的 `results/` 文件夹，命名规则为：

```
源文件名-模型-taskId.扩展名
```

若 RunningHub 返回多个结果文件，ClearClip 会**全部下载**，任务卡片展示首个结果，完整路径列表可在详情中查看。

---

## 📊 任务状态

```
UPLOADING → CONFIGURING → PENDING → RUNNING → DOWNLOADING → SUCCESS
                                                          ↘ FAILED
                                                          ↘ CANCELED
```

- 应用启动时自动恢复**未完成的远端任务**，继续轮询并下载结果
- 取消任务仅停止本机跟踪，已提交至 RunningHub 的远端任务**不承诺取消**
- 任务中断未提交时，本地标记为失败，**避免重复扣费**

---

## 🛠️ 常见问题

<details>
<summary><b>API Key 未授权或已失效</b></summary>

重新打开设置，粘贴有效的 API Key 并点击「保存并验证」。

</details>

<details>
<summary><b>余额不足 / 并发超限</b></summary>

前往 RunningHub 充值，或等待当前任务完成后重试。

</details>

<details>
<summary><b>节点识别失败</b></summary>

点击工作流设置中的「检测节点」，从候选节点列表手动选择正确的 `nodeId + fieldName` 并保存。

</details>

<details>
<summary><b>需要导出诊断日志</b></summary>

点击应用内「导出诊断」，生成 `diagnostics-export-*.jsonl` 文件。日志为脱敏 JSONL，**不含 API Key**，不会上传至任何远端。

</details>

---

## 📄 License

[MIT](LICENSE) © 2025 AshWood97
