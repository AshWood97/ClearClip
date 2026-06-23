<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { check, type DownloadEvent } from '@tauri-apps/plugin-updater';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { VueLenis } from 'lenis/vue';
import Dropzone, { type SelectedVideo } from './components/Dropzone.vue';
import OptionsPanel, {
  type ModelOverride,
  type NodeInspection,
} from './components/OptionsPanel.vue';
import TaskList, { type TaskItem } from './components/TaskList.vue';

type NoticeKind = 'success' | 'error' | 'info';

interface Notice {
  kind: NoticeKind;
  message: string;
}

interface SettingsSnapshot {
  hasApiKey: boolean;
  settings: {
    modelOverrides?: Record<string, ModelOverride>;
    apiKeyVerifiedAt?: string | null;
    apiKeyLastError?: string | null;
  };
}

const selectedFile = ref<SelectedVideo | null>(null);
const tasks = ref<TaskItem[]>([]);
const isSubmitting = ref(false);
const isSavingApiKey = ref(false);
const isInspectingNodes = ref(false);
const notice = ref<Notice | null>(null);
const updateVisible = ref(false);
const updateStatus = ref('');
const updateProgress = ref(0);
const hasApiKey = ref(false);
const apiKeyVerifiedAt = ref<string | null>(null);
const apiKeyLastError = ref<string | null>(null);
const modelOverrides = ref<Record<string, ModelOverride>>({});
const nodeInspection = ref<NodeInspection | null>(null);

let noticeTimer: number | undefined;
const unlisteners: UnlistenFn[] = [];

const activeStatuses = ['UPLOADING', 'CONFIGURING', 'PENDING', 'RUNNING', 'DOWNLOADING'];
const activeTaskCount = computed(() => tasks.value.filter((task) => activeStatuses.includes(task.status)).length);
const completedTaskCount = computed(() => tasks.value.filter((task) => task.status === 'SUCCESS').length);
const failedTaskCount = computed(() => tasks.value.filter((task) => task.status === 'FAILED').length);
const canceledTaskCount = computed(() => tasks.value.filter((task) => task.status === 'CANCELED').length);
const hasSelectedPath = computed(() => Boolean(selectedFile.value?.path));

onMounted(async () => {
  if (isTauri()) {
    await registerTaskEvents();
    await Promise.all([loadAppSettings(), loadTasks()]);
    void checkForUpdates();
  }
});

onUnmounted(() => {
  unlisteners.splice(0).forEach((unlisten) => unlisten());
  if (noticeTimer) window.clearTimeout(noticeTimer);
});

async function registerTaskEvents() {
  unlisteners.push(
    await listen<TaskItem>('task-updated', (event) => {
      upsertTask(event.payload);
    })
  );
}

async function loadTasks() {
  try {
    tasks.value = await invoke<TaskItem[]>('list_tasks');
  } catch (error) {
    setNotice('error', `读取任务历史失败：${formatError(error)}`);
  }
}

async function checkForUpdates() {
  try {
    const update = await check();
    if (!update) return;

    updateVisible.value = true;
    updateStatus.value = `发现新版本 ${update.version}，正在下载`;
    updateProgress.value = 0;

    let downloadedBytes = 0;
    let totalBytes: number | undefined;

    await update.downloadAndInstall((event: DownloadEvent) => {
      if (event.event === 'Started') {
        downloadedBytes = 0;
        totalBytes = event.data.contentLength;
        updateProgress.value = totalBytes ? 0 : 8;
        updateStatus.value = `正在下载 ${update.version}`;
      }

      if (event.event === 'Progress') {
        downloadedBytes += event.data.chunkLength;
        updateProgress.value = totalBytes
          ? Math.min(99, Math.round((downloadedBytes / totalBytes) * 100))
          : Math.min(95, updateProgress.value + 4);
      }

      if (event.event === 'Finished') {
        updateProgress.value = 100;
        updateStatus.value = '更新已安装，重启应用后生效';
      }
    });
  } catch (error) {
    console.info('Updater check skipped:', error);
  }
}

function handleFileSelected(file: SelectedVideo) {
  selectedFile.value = file;
  if (file.path) {
    setNotice('success', `已选择 ${file.name}`);
  } else {
    setNotice('error', '没有获取到本地路径，请用桌面文件选择器重新选择。');
  }
}

async function handleSubmit({ appId, appName, params }: { appId: string; appName: string; params: Record<string, unknown> }) {
  if (!selectedFile.value?.path) {
    setNotice('error', '请先选择一个可读取路径的视频文件。');
    return;
  }

  if (!hasApiKey.value) {
    setNotice('error', '请先保存并验证 RunningHub API Key。');
    return;
  }

  if (!isSupportedVideoFile(selectedFile.value.name || selectedFile.value.path)) {
    setNotice('error', '仅支持 mp4、mov、avi、mkv、webm、m4v 视频文件。');
    return;
  }

  if (!isTauri()) {
    setNotice('error', '当前是浏览器预览环境，请在 Tauri 桌面应用中提交任务。');
    return;
  }

  isSubmitting.value = true;
  const file = selectedFile.value;

  try {
    const taskId = await invoke<string>('submit_video_task', {
      appId,
      appName,
      filePath: file.path,
      params,
    });
    setNotice(tasks.value.some((task) => task.taskId === taskId) ? 'info' : 'success', '任务已加入队列。');
    await loadTasks();
  } catch (error) {
    setNotice('error', `提交失败：${formatError(error)}`);
  } finally {
    isSubmitting.value = false;
  }
}

async function handleRetry(task: TaskItem) {
  if (!task.appId || !task.filePath) {
    setNotice('error', '这个任务缺少原始提交信息，无法重试。');
    return;
  }

  if (!isTauri()) {
    setNotice('error', '当前是浏览器预览环境，请在 Tauri 桌面应用中重试任务。');
    return;
  }

  try {
    await invoke<string>('submit_video_task', {
      appId: task.appId,
      appName: task.appName || task.modelName || '视频处理',
      filePath: task.filePath,
      params: task.params ?? {},
    });
    await loadTasks();
    setNotice('success', '已重新提交任务。');
  } catch (error) {
    setNotice('error', `重试失败：${formatError(error)}`);
  }
}

async function handleCancelTask(task: TaskItem) {
  try {
    const updated = await invoke<TaskItem>('cancel_task', { taskId: task.taskId });
    upsertTask(updated);
    setNotice('info', '已取消本机任务跟踪。');
  } catch (error) {
    setNotice('error', `取消任务失败：${formatError(error)}`);
  }
}

async function handleClearHistory() {
  try {
    tasks.value = await invoke<TaskItem[]>('clear_task_history');
    setNotice('success', '已清除完成、失败和取消的任务记录。');
  } catch (error) {
    setNotice('error', `清除任务记录失败：${formatError(error)}`);
  }
}

async function loadAppSettings() {
  try {
    applySettingsSnapshot(await invoke<SettingsSnapshot>('get_app_settings'));
  } catch (error) {
    setNotice('error', `读取设置失败：${formatError(error)}`);
  }
}

async function handleSaveApiKey(apiKey: string) {
  if (!isTauri()) {
    setNotice('error', '当前是浏览器预览环境，请在 Tauri 桌面应用中保存 API Key。');
    return;
  }

  isSavingApiKey.value = true;
  try {
    applySettingsSnapshot(await invoke<SettingsSnapshot>('save_api_key', { apiKey }));
    setNotice('success', 'API Key 验证通过，已保存到本机系统凭据。');
  } catch (error) {
    apiKeyLastError.value = formatError(error);
    setNotice('error', `保存 API Key 失败：${formatError(error)}`);
  } finally {
    isSavingApiKey.value = false;
  }
}

async function handleClearApiKey() {
  if (!isTauri()) {
    setNotice('error', '当前是浏览器预览环境，无法清除本机凭据。');
    return;
  }

  try {
    applySettingsSnapshot(await invoke<SettingsSnapshot>('clear_api_key'));
    setNotice('success', 'API Key 已清除。');
  } catch (error) {
    setNotice('error', `清除 API Key 失败：${formatError(error)}`);
  }
}

async function handleInspectModelNodes(appId: string) {
  if (!hasApiKey.value) {
    setNotice('error', '请先保存并验证 RunningHub API Key。');
    return;
  }

  isInspectingNodes.value = true;
  try {
    nodeInspection.value = await invoke<NodeInspection>('inspect_model_nodes', { appId });
    setNotice(nodeInspection.value.recommended ? 'success' : 'info', nodeInspection.value.reason);
  } catch (error) {
    setNotice('error', `检测节点失败：${formatError(error)}`);
  } finally {
    isInspectingNodes.value = false;
  }
}

async function handleSaveModelOverride({
  appId,
  overrideConfig,
}: {
  appId: string;
  overrideConfig: ModelOverride | null;
}) {
  if (!isTauri()) {
    setNotice('error', '当前是浏览器预览环境，无法保存节点配置。');
    return;
  }

  try {
    const settings = await invoke<SettingsSnapshot['settings']>('save_model_override', {
      appId,
      overrideConfig,
    });
    modelOverrides.value = settings.modelOverrides ?? {};
    setNotice('success', overrideConfig ? '节点配置已保存。' : '已恢复自动识别节点。');
  } catch (error) {
    setNotice('error', `保存节点配置失败：${formatError(error)}`);
  }
}

async function handleOpenResult(path: string) {
  if (!isTauri()) {
    setNotice('error', '当前是浏览器预览环境，无法打开本地文件位置。');
    return;
  }

  try {
    await revealItemInDir(path);
  } catch (error) {
    setNotice('error', `打开结果位置失败：${formatError(error)}`);
  }
}

async function handleOpenResultsDir() {
  try {
    const path = await invoke<string>('open_results_dir');
    setNotice('success', `已打开结果目录：${path}`);
  } catch (error) {
    setNotice('error', `打开结果目录失败：${formatError(error)}`);
  }
}

async function handleExportDiagnostics() {
  try {
    const path = await invoke<string>('export_diagnostics');
    await revealItemInDir(path);
    setNotice('success', '诊断日志已导出。');
  } catch (error) {
    setNotice('error', `导出诊断失败：${formatError(error)}`);
  }
}

function applySettingsSnapshot(snapshot: SettingsSnapshot) {
  hasApiKey.value = snapshot.hasApiKey;
  modelOverrides.value = snapshot.settings.modelOverrides ?? {};
  apiKeyVerifiedAt.value = snapshot.settings.apiKeyVerifiedAt ?? null;
  apiKeyLastError.value = snapshot.settings.apiKeyLastError ?? null;
}

function upsertTask(nextTask: TaskItem) {
  const index = tasks.value.findIndex((task) => task.taskId === nextTask.taskId);
  if (index >= 0) {
    tasks.value[index] = nextTask;
  } else {
    tasks.value.unshift(nextTask);
  }
  tasks.value = [...tasks.value].sort((left, right) => (right.createdAt ?? '').localeCompare(left.createdAt ?? ''));
}

function isSupportedVideoFile(nameOrPath: string) {
  const extension = nameOrPath.split(/[\\/]/).pop()?.split('.').pop()?.toLowerCase() ?? '';
  return ['mp4', 'mov', 'avi', 'mkv', 'webm', 'm4v'].includes(extension);
}

function setNotice(kind: NoticeKind, message: string) {
  notice.value = { kind, message };
  if (noticeTimer) window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    notice.value = null;
  }, 4200);
}

function formatError(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return JSON.stringify(error);
}
</script>

<template>
  <VueLenis root>
    <div class="min-h-screen bg-zinc-100 text-zinc-950 antialiased">
      <header class="sticky top-0 z-40 border-b border-zinc-200 bg-white/95 backdrop-blur" data-tauri-drag-region>
        <div class="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-4 px-6 py-3">
          <div>
            <h1 class="text-xl font-semibold">ClearClip 无印良片</h1>
            <p class="mt-1 text-sm text-zinc-500">视频去水印与超分处理工作台</p>
          </div>
          <div class="grid grid-cols-4 gap-2 text-center text-sm">
            <div class="rounded-lg border border-zinc-200 bg-zinc-50 px-4 py-1.5">
              <p class="font-semibold text-zinc-950">{{ activeTaskCount }}</p>
              <p class="text-xs text-zinc-500">进行中</p>
            </div>
            <div class="rounded-lg border border-zinc-200 bg-zinc-50 px-4 py-1.5">
              <p class="font-semibold text-emerald-700">{{ completedTaskCount }}</p>
              <p class="text-xs text-zinc-500">已完成</p>
            </div>
            <div class="rounded-lg border border-zinc-200 bg-zinc-50 px-4 py-1.5">
              <p class="font-semibold text-red-700">{{ failedTaskCount }}</p>
              <p class="text-xs text-zinc-500">失败</p>
            </div>
            <div class="rounded-lg border border-zinc-200 bg-zinc-50 px-4 py-1.5">
              <p class="font-semibold text-zinc-600">{{ canceledTaskCount }}</p>
              <p class="text-xs text-zinc-500">已取消</p>
            </div>
          </div>
        </div>
      </header>

      <div
        v-if="notice"
        class="fixed right-5 top-24 z-50 max-w-sm rounded-lg border px-4 py-3 text-sm shadow-lg"
        :class="{
          'border-emerald-200 bg-emerald-50 text-emerald-800': notice.kind === 'success',
          'border-red-200 bg-red-50 text-red-800': notice.kind === 'error',
          'border-zinc-200 bg-white text-zinc-800': notice.kind === 'info',
        }"
      >
        {{ notice.message }}
      </div>

      <div v-if="updateVisible" class="fixed inset-0 z-50 flex items-center justify-center bg-zinc-950/45 p-6 backdrop-blur-sm">
        <div class="w-full max-w-md rounded-lg border border-zinc-200 bg-white p-6 shadow-xl">
          <div class="flex items-start justify-between gap-4">
            <div>
              <h2 class="text-base font-semibold text-zinc-950">应用更新</h2>
              <p class="mt-2 text-sm text-zinc-600">{{ updateStatus }}</p>
            </div>
            <button
              v-if="updateProgress === 100"
              type="button"
              class="rounded-md border border-zinc-300 px-2 py-1 text-xs text-zinc-600 hover:border-zinc-500"
              @click="updateVisible = false"
            >
              关闭
            </button>
          </div>
          <div class="mt-5 h-2 overflow-hidden rounded-full bg-zinc-200">
            <div class="h-full rounded-full bg-teal-600 transition-all duration-300" :style="{ width: `${updateProgress}%` }" />
          </div>
          <p class="mt-2 text-right text-xs text-zinc-500">{{ updateProgress }}%</p>
        </div>
      </div>

      <main class="mx-auto grid max-w-7xl grid-cols-1 gap-5 px-6 py-5 xl:grid-cols-[420px_minmax(0,1fr)]">
        <div class="space-y-5">
          <Dropzone @file-selected="handleFileSelected" />
          <OptionsPanel
            :has-file="Boolean(selectedFile)"
            :has-path="hasSelectedPath"
            :has-api-key="hasApiKey"
            :api-key-verified-at="apiKeyVerifiedAt"
            :api-key-last-error="apiKeyLastError"
            :is-submitting="isSubmitting"
            :is-saving-api-key="isSavingApiKey"
            :is-inspecting-nodes="isInspectingNodes"
            :model-overrides="modelOverrides"
            :node-inspection="nodeInspection"
            @submit="handleSubmit"
            @save-api-key="handleSaveApiKey"
            @clear-api-key="handleClearApiKey"
            @inspect-model-nodes="handleInspectModelNodes"
            @save-model-override="handleSaveModelOverride"
          />
        </div>

        <TaskList
          :tasks="tasks"
          @retry-task="handleRetry"
          @cancel-task="handleCancelTask"
          @clear-history="handleClearHistory"
          @open-result="handleOpenResult"
          @open-results-dir="handleOpenResultsDir"
          @export-diagnostics="handleExportDiagnostics"
        />
      </main>
    </div>
  </VueLenis>
</template>
