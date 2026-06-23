# ClearClip 无印良片

ClearClip 是一个本地 Tauri 桌面应用，用 RunningHub AI 工作流处理视频。当前内置 3 个工作流：

- 去水印：`2059943323456065537`
- 4K 超分：`2023058520156934145`
- 2K 超分：`2023054533156409346`

## 首次配置

1. 在 RunningHub 获取 API Key。
2. 打开应用右侧设置区，将 API Key 粘贴到“保存并验证 API Key”。
3. 应用会调用默认工作流的 API 示例接口验证密钥。验证失败时不会把密钥标记为可用。
4. API Key 保存到本机系统凭据中，不写入前端存储或普通设置 JSON。

普通设置会写入应用数据目录的 `settings.json`，仅保存非敏感配置，例如每个工作流的节点覆盖配置和最近验证状态。

## 节点检测

RunningHub AI 应用提交任务需要 `nodeInfoList`。ClearClip 会先读取 `/api/webapp/apiCallDemo` 的示例，再按以下顺序自动识别视频上传节点：

- `fieldName === "video"`
- `fieldName === "upload"`
- `fieldName` 包含 `video`、`file` 或 `media`
- 节点描述包含“视频”“上传”“video”或“upload”
- 示例值看起来像视频文件名

如果自动识别失败或出现多个候选，请在高级设置里点击“检测节点”，从候选节点中选择正确的 `nodeId + fieldName` 并保存。失败任务重试时会复用当前视频、模型和节点配置。

## 任务与恢复

任务历史保存在应用数据目录的 `tasks.json`：

- 应用最多同时运行 2 个 RunningHub 任务。
- 任务状态包括 `UPLOADING`、`CONFIGURING`、`PENDING`、`RUNNING`、`DOWNLOADING`、`SUCCESS`、`FAILED`、`CANCELED`。
- 应用启动时会恢复已有远端 `taskId` 且未完成的任务，继续轮询并下载结果。
- 如果应用中断时任务还没有提交到 RunningHub，本地会标记失败，避免重复上传和潜在重复扣费。
- 取消任务只停止本机跟踪；已提交到 RunningHub 的远端任务不会被承诺取消。

## 结果目录

处理结果默认保存到应用数据目录下的 `results` 文件夹。文件名格式：

```text
源文件名-模型-taskId.扩展名
```

如果 RunningHub 返回多个结果文件，ClearClip 会全部下载，任务卡片显示首个结果，同时保留完整结果路径列表。应用内可以打开单个结果位置，也可以直接打开结果目录。

## 诊断日志

本地诊断日志写入应用数据目录的 `diagnostics.log`，内容为脱敏 JSONL：

- 任务状态变化
- HTTP/API 错误码
- 节点识别结果
- 历史清理和导出事件

日志不会记录 API Key，也不会上传到任何远端。应用内“导出诊断”会生成 `diagnostics-export-*.jsonl`，用于排查用户现场问题。

## 常见错误

- API Key 未授权或已失效：重新保存并验证 API Key。
- RunningHub 账户余额不足：充值后重试。
- RunningHub 并发上限已满：稍后重试，或等待当前任务完成。
- `nodeInfoList` 与工作流不匹配：点击“检测节点”，选择正确的视频上传节点并保存。
- 未找到指定任务：远端任务可能已过期或被清理。
- 服务端异常：稍后重试，并导出诊断文件辅助排查。

## 开发与验证

```bash
pnpm install
pnpm exec vue-tsc --noEmit
pnpm build

cd src-tauri
cargo check
cargo test

cd ..
pnpm tauri build
```

Vite 已排除监听 `src-tauri/target/**`、`node_modules/**`、`dist/**`，避免 Windows 上 `.pdb` 文件被占用导致开发服务器崩溃。
