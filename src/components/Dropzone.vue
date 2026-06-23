<template>
  <section class="rounded-lg border border-zinc-200 bg-white shadow-sm">
    <div class="border-b border-zinc-200 px-4 py-3">
      <div class="flex items-center justify-between gap-3">
        <div>
          <h2 class="text-base font-semibold text-zinc-950">选择视频</h2>
          <p class="mt-1 text-sm text-zinc-500">支持 mp4、mov、avi、mkv、webm。</p>
        </div>
        <span
          class="rounded-md px-2.5 py-1 text-xs font-medium"
          :class="selectedFile?.path ? 'bg-emerald-50 text-emerald-700' : 'bg-zinc-100 text-zinc-600'"
        >
          {{ selectedFile?.path ? '路径已就绪' : '等待文件' }}
        </span>
      </div>
    </div>

    <button
      type="button"
      class="m-4 flex min-h-36 w-[calc(100%-2rem)] flex-col items-center justify-center rounded-lg border border-dashed p-4 text-center transition"
      :class="[
        isDragging
          ? 'border-teal-500 bg-teal-50 text-teal-900'
          : 'border-zinc-300 bg-zinc-50 text-zinc-800 hover:border-zinc-500 hover:bg-white',
      ]"
      @click="selectFromDialog"
      @dragenter.prevent="isDragging = true"
      @dragover.prevent="isDragging = true"
      @dragleave.prevent="isDragging = false"
      @drop.prevent="onDrop"
    >
      <VideoIcon v-if="selectedFile" class="mb-3 h-8 w-8 text-teal-700" :stroke-width="1.6" />
      <UploadIcon v-else class="mb-3 h-8 w-8 text-zinc-500" :stroke-width="1.6" />

      <template v-if="selectedFile">
        <span class="max-w-full truncate text-sm font-semibold text-zinc-950">{{ selectedFile.name }}</span>
        <span class="mt-2 max-w-full truncate font-mono text-xs text-zinc-500">
          {{ selectedFile.path || '未取得系统路径，请点击重新选择' }}
        </span>
      </template>
      <template v-else>
        <span class="text-sm font-semibold text-zinc-950">点击选择，或将视频拖到这里</span>
        <span class="mt-2 text-sm text-zinc-500">桌面环境会直接返回可提交的本地路径。</span>
      </template>
    </button>

    <div v-if="warning" class="mx-4 mb-4 flex gap-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800">
      <AlertCircle class="mt-0.5 h-4 w-4 shrink-0" :stroke-width="1.8" />
      <span>{{ warning }}</span>
    </div>

    <input ref="fileInput" class="hidden" type="file" accept="video/*" @change="onFileSelect" />
  </section>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { AlertCircle, UploadIcon, VideoIcon } from 'lucide-vue-next';

export interface SelectedVideo {
  name: string;
  path?: string;
  size?: number;
  source: 'dialog' | 'drop' | 'browser';
}

const emit = defineEmits<{
  (event: 'file-selected', file: SelectedVideo): void;
}>();

const selectedFile = ref<SelectedVideo | null>(null);
const fileInput = ref<HTMLInputElement | null>(null);
const isDragging = ref(false);
const warning = ref('');

function fileNameFromPath(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

function selectFile(file: SelectedVideo, nextWarning = '') {
  selectedFile.value = file;
  warning.value = nextWarning;
  emit('file-selected', file);
}

async function selectFromDialog() {
  warning.value = '';

  try {
    const selected = await open({
      multiple: false,
      title: '选择需要处理的视频',
      filters: [
        {
          name: 'Video',
          extensions: ['mp4', 'mov', 'avi', 'mkv', 'webm', 'm4v'],
        },
      ],
    });

    if (typeof selected === 'string') {
      selectFile({
        name: fileNameFromPath(selected),
        path: selected,
        source: 'dialog',
      });
      return;
    }
  } catch {
    fileInput.value?.click();
    return;
  }
}

function onFileSelect(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;

  const maybePath = (file as File & { path?: string }).path;
  selectFile(
    {
      name: file.name,
      path: maybePath,
      size: file.size,
      source: maybePath ? 'dialog' : 'browser',
    },
    maybePath ? '' : '当前环境没有暴露真实文件路径，请在桌面应用里使用“点击选择”按钮。'
  );
  input.value = '';
}

function onDrop(event: DragEvent) {
  isDragging.value = false;

  const file = event.dataTransfer?.files?.[0];
  if (!file) return;

  const maybePath = (file as File & { path?: string }).path;
  selectFile(
    {
      name: file.name,
      path: maybePath,
      size: file.size,
      source: maybePath ? 'drop' : 'browser',
    },
    maybePath ? '' : '拖放事件没有提供系统路径，请点击选择文件后再提交。'
  );
}
</script>
