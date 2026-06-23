<template>
  <section class="min-h-full rounded-lg border border-zinc-200 bg-white shadow-sm">
    <div class="flex flex-wrap items-center justify-between gap-3 border-b border-zinc-200 px-5 py-4">
      <div>
        <h2 class="text-base font-semibold text-zinc-950">任务队列</h2>
        <p class="mt-1 text-sm text-zinc-500">{{ tasks.length ? `${tasks.length} 个任务` : '暂无任务' }}</p>
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-xs font-medium text-zinc-700 transition hover:border-zinc-500"
          @click="$emit('open-results-dir')"
        >
          <FolderOpen class="h-3.5 w-3.5" :stroke-width="1.8" />
          结果目录
        </button>
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-xs font-medium text-zinc-700 transition hover:border-zinc-500"
          @click="$emit('export-diagnostics')"
        >
          <FileDown class="h-3.5 w-3.5" :stroke-width="1.8" />
          诊断
        </button>
        <button
          type="button"
          class="rounded-md bg-zinc-100 px-3 py-1.5 text-xs font-medium text-zinc-600 transition hover:bg-zinc-200"
          @click="$emit('clear-history')"
        >
          清理记录
        </button>
      </div>
    </div>

    <div v-if="tasks.length === 0" class="flex min-h-96 items-center justify-center p-8">
      <div class="max-w-sm text-center">
        <ListChecks class="mx-auto mb-4 h-10 w-10 text-zinc-300" :stroke-width="1.5" />
        <p class="text-sm font-medium text-zinc-700">队列是空的</p>
        <p class="mt-2 text-sm text-zinc-500">任务会自动保存，重启应用后仍会显示历史和可恢复进度。</p>
      </div>
    </div>

    <div v-else v-auto-animate class="space-y-3 p-5">
      <article
        v-for="task in tasks"
        :key="task.taskId"
        class="rounded-lg border border-zinc-200 bg-zinc-50 p-4"
      >
        <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2">
              <span class="rounded-md px-2 py-1 text-xs font-semibold" :class="statusClass(task.status)">
                {{ statusLabel(task.status) }}
              </span>
              <span class="text-sm font-semibold text-zinc-950">{{ task.appName || task.modelName || '视频处理' }}</span>
            </div>
            <p class="mt-2 truncate text-sm text-zinc-600">{{ task.fileName || task.filePath || '未记录文件名' }}</p>
            <div class="mt-1 flex flex-wrap gap-x-3 gap-y-1 font-mono text-xs text-zinc-400">
              <span>{{ shortTaskId(task.taskId) }}</span>
              <span v-if="task.remoteTaskId">RH {{ shortTaskId(task.remoteTaskId) }}</span>
              <span v-if="task.updatedAt">更新 {{ task.updatedAt }}</span>
            </div>
          </div>

          <div class="flex shrink-0 flex-wrap gap-2">
            <button
              v-if="task.savePath"
              type="button"
              class="rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-xs font-medium text-zinc-700 transition hover:border-zinc-500"
              @click="$emit('open-result', task.savePath)"
            >
              打开位置
            </button>
            <button
              v-if="isActive(task.status)"
              type="button"
              class="rounded-md border border-red-200 bg-white px-3 py-1.5 text-xs font-medium text-red-700 transition hover:border-red-400"
              @click="$emit('cancel-task', task)"
            >
              取消
            </button>
            <button
              v-if="task.status === 'FAILED' || task.status === 'CANCELED'"
              type="button"
              class="rounded-md bg-zinc-950 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-zinc-800"
              @click="$emit('retry-task', task)"
            >
              重试
            </button>
          </div>
        </div>

        <div v-if="isActive(task.status)" class="mt-4 space-y-2">
          <div class="flex justify-between text-xs text-zinc-500">
            <span>{{ progressLabel(task.status) }}</span>
            <span>{{ displayProgress(task.progress) }}%</span>
          </div>
          <div class="h-2 overflow-hidden rounded-full bg-zinc-200">
            <div
              class="h-full rounded-full bg-teal-600 transition-all duration-300"
              :style="{ width: `${displayProgress(task.progress)}%` }"
            />
          </div>
        </div>

        <div v-if="task.savePath || task.error" class="mt-4 border-t border-zinc-200 pt-4">
          <div v-if="task.savePath" class="space-y-1">
            <p
              v-for="path in task.savePaths?.length ? task.savePaths : [task.savePath]"
              :key="path"
              class="break-all font-mono text-xs text-emerald-700"
            >
              {{ path }}
            </p>
          </div>
          <p v-if="task.error" class="text-sm text-red-700">
            {{ task.error }}
          </p>
        </div>
      </article>
    </div>
  </section>
</template>

<script setup lang="ts">
import { vAutoAnimate } from '@formkit/auto-animate/vue';
import { FileDown, FolderOpen, ListChecks } from 'lucide-vue-next';

export interface TaskItem {
  taskId: string;
  remoteTaskId?: string | null;
  status: string;
  progress: number;
  savePath?: string | null;
  savePaths?: string[];
  error?: string | null;
  appId?: string;
  appName?: string;
  modelName?: string;
  fileName?: string;
  filePath?: string;
  params?: Record<string, unknown>;
  createdAt?: string;
  updatedAt?: string;
}

defineProps<{
  tasks: TaskItem[];
}>();

defineEmits<{
  (event: 'retry-task', task: TaskItem): void;
  (event: 'cancel-task', task: TaskItem): void;
  (event: 'clear-history'): void;
  (event: 'open-result', path: string): void;
  (event: 'open-results-dir'): void;
  (event: 'export-diagnostics'): void;
}>();

function isActive(status: string) {
  return ['UPLOADING', 'CONFIGURING', 'PENDING', 'RUNNING', 'DOWNLOADING'].includes(status);
}

function displayProgress(progress: number) {
  if (!Number.isFinite(progress)) return 0;
  return Math.min(100, Math.max(0, Math.round(progress)));
}

function shortTaskId(taskId: string) {
  if (taskId.length <= 18) return taskId;
  return `${taskId.slice(0, 14)}...${taskId.slice(-4)}`;
}

function statusLabel(status: string) {
  const labels: Record<string, string> = {
    UPLOADING: '上传中',
    CONFIGURING: '配置中',
    PENDING: '等待中',
    RUNNING: '运行中',
    DOWNLOADING: '下载中',
    SUCCESS: '已完成',
    FAILED: '失败',
    CANCELED: '已取消',
  };
  return labels[status] ?? status;
}

function statusClass(status: string) {
  if (status === 'SUCCESS') return 'bg-emerald-100 text-emerald-700';
  if (status === 'FAILED') return 'bg-red-100 text-red-700';
  if (status === 'CANCELED') return 'bg-zinc-200 text-zinc-700';
  if (status === 'DOWNLOADING') return 'bg-sky-100 text-sky-700';
  if (status === 'UPLOADING') return 'bg-indigo-100 text-indigo-700';
  if (status === 'CONFIGURING') return 'bg-violet-100 text-violet-700';
  if (status === 'PENDING') return 'bg-amber-100 text-amber-700';
  return 'bg-teal-100 text-teal-700';
}

function progressLabel(status: string) {
  const labels: Record<string, string> = {
    UPLOADING: '正在上传视频',
    CONFIGURING: '正在识别工作流节点',
    PENDING: '等待处理',
    RUNNING: 'RunningHub 正在处理',
    DOWNLOADING: '正在下载结果',
  };
  return labels[status] ?? '处理进度';
}
</script>
