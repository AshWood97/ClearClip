<template>
  <section class="rounded-lg border border-zinc-200 bg-white shadow-sm">
    <div class="border-b border-zinc-200 px-4 py-3">
      <h2 class="text-base font-semibold text-zinc-950">处理参数</h2>
      <p class="mt-1 text-sm text-zinc-500">选择 AI 工作流，验证密钥后即可加入队列。</p>
    </div>

    <div class="space-y-4 p-4">
      <div class="space-y-2">
        <label for="model-select" class="text-sm font-medium text-zinc-700">AI 工作流</label>
        <select
          id="model-select"
          v-model="selectedAppId"
          class="w-full rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-950 outline-none transition focus:border-teal-600 focus:ring-2 focus:ring-teal-100"
        >
          <option v-for="model in models" :key="model.id" :value="model.id">
            {{ model.name }} · {{ model.badge }}
          </option>
        </select>
      </div>

      <div class="rounded-lg border border-teal-100 bg-teal-50 px-3 py-2.5">
        <div class="flex items-start justify-between gap-3">
          <div>
            <p class="text-sm font-semibold text-teal-950">{{ selectedModel.name }}</p>
            <p class="mt-1 text-sm text-teal-800">{{ selectedModel.description }}</p>
          </div>
          <span class="shrink-0 rounded-md bg-teal-700 px-2 py-1 text-xs font-medium text-white">
            {{ selectedModel.badge }}
          </span>
        </div>
      </div>

      <div class="rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-3">
        <div class="flex items-center justify-between gap-3">
          <div class="flex items-center gap-2">
            <KeyRound class="h-4 w-4 text-zinc-600" :stroke-width="1.8" />
            <div>
              <p class="text-sm font-medium text-zinc-800">RunningHub API Key</p>
              <p class="mt-0.5 text-xs" :class="hasApiKey ? 'text-emerald-700' : 'text-amber-700'">
                {{ apiKeyStatusText }}
              </p>
            </div>
          </div>
          <button
            v-if="hasApiKey"
            type="button"
            class="rounded-md border border-zinc-300 bg-white px-2.5 py-1.5 text-xs font-medium text-zinc-700 transition hover:border-red-300 hover:text-red-700"
            @click="$emit('clear-api-key')"
          >
            清除
          </button>
        </div>

        <form class="mt-3 flex gap-2" @submit.prevent="saveApiKey">
          <input
            v-model="apiKeyInput"
            type="password"
            autocomplete="off"
            placeholder="粘贴 RunningHub API Key"
            class="min-w-0 flex-1 rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm outline-none transition focus:border-teal-600 focus:ring-2 focus:ring-teal-100"
          />
          <button
            type="submit"
            class="inline-flex shrink-0 items-center gap-1.5 rounded-md bg-zinc-950 px-3 py-2 text-sm font-semibold text-white transition hover:bg-zinc-800 disabled:cursor-not-allowed disabled:bg-zinc-300 disabled:text-zinc-500"
            :disabled="!apiKeyInput.trim() || isSavingApiKey"
          >
            <LoaderCircle v-if="isSavingApiKey" class="h-4 w-4 animate-spin" :stroke-width="1.8" />
            <Save v-else class="h-4 w-4" :stroke-width="1.8" />
            {{ isSavingApiKey ? '验证中' : '保存并验证' }}
          </button>
        </form>
        <p v-if="apiKeyLastError" class="mt-2 text-xs leading-5 text-red-700">{{ apiKeyLastError }}</p>
      </div>

      <details class="rounded-lg border border-zinc-200 bg-white px-3 py-3">
        <summary class="cursor-pointer text-sm font-medium text-zinc-800">高级节点配置</summary>
        <div class="mt-3 space-y-3">
          <p class="text-xs leading-5 text-zinc-500">
            默认会自动识别视频输入节点；如失败，可先检测节点，再保存候选节点。
          </p>

          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-xs font-medium text-zinc-700 transition hover:border-zinc-500 disabled:cursor-not-allowed disabled:bg-zinc-100 disabled:text-zinc-400"
            :disabled="!hasApiKey || isInspectingNodes"
            @click="$emit('inspect-model-nodes', selectedAppId)"
          >
            <LoaderCircle v-if="isInspectingNodes" class="h-3.5 w-3.5 animate-spin" :stroke-width="1.8" />
            <Search v-else class="h-3.5 w-3.5" :stroke-width="1.8" />
            {{ isInspectingNodes ? '检测中' : '检测节点' }}
          </button>

          <div v-if="nodeInspection" class="rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2">
            <p class="text-xs text-zinc-600">{{ nodeInspection.reason }}</p>
            <label class="mt-2 block space-y-1 text-xs font-medium text-zinc-600">
              候选节点
              <select
                v-model="selectedNodeKey"
                class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm font-normal text-zinc-950 outline-none transition focus:border-teal-600 focus:ring-2 focus:ring-teal-100"
              >
                <option value="">手动填写</option>
                <option v-for="node in nodeInspection.nodes" :key="nodeKey(node)" :value="nodeKey(node)">
                  {{ node.nodeId }} · {{ node.fieldName }}{{ node.nodeName ? ` · ${node.nodeName}` : '' }}
                </option>
              </select>
            </label>
          </div>

          <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <label class="space-y-1 text-xs font-medium text-zinc-600">
              nodeId
              <input
                v-model="overrideNodeId"
                class="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm font-normal text-zinc-950 outline-none transition focus:border-teal-600 focus:ring-2 focus:ring-teal-100"
                placeholder="例如 7"
              />
            </label>
            <label class="space-y-1 text-xs font-medium text-zinc-600">
              fieldName
              <input
                v-model="overrideFieldName"
                class="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm font-normal text-zinc-950 outline-none transition focus:border-teal-600 focus:ring-2 focus:ring-teal-100"
                placeholder="例如 video"
              />
            </label>
          </div>
          <div class="flex flex-wrap gap-2">
            <button
              type="button"
              class="rounded-md bg-zinc-950 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-zinc-800 disabled:cursor-not-allowed disabled:bg-zinc-300 disabled:text-zinc-500"
              :disabled="!canSaveOverride"
              @click="saveOverride"
            >
              保存节点配置
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-xs font-medium text-zinc-700 transition hover:border-zinc-500"
              @click="clearOverride"
            >
              <Trash2 class="h-3.5 w-3.5" :stroke-width="1.8" />
              使用自动识别
            </button>
          </div>
        </div>
      </details>

      <div class="rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2.5 text-sm">
        <div class="flex items-start gap-2">
          <CheckCircle2 v-if="canSubmit" class="mt-0.5 h-4 w-4 shrink-0 text-emerald-600" :stroke-width="1.8" />
          <AlertCircle v-else class="mt-0.5 h-4 w-4 shrink-0 text-amber-600" :stroke-width="1.8" />
          <p class="text-zinc-700">
            {{ helperText }}
          </p>
        </div>
      </div>

      <button
        type="button"
        :disabled="!canSubmit"
        class="flex w-full items-center justify-center gap-2 rounded-lg bg-zinc-950 px-5 py-2.5 text-sm font-semibold text-white shadow-sm transition hover:bg-zinc-800 disabled:cursor-not-allowed disabled:bg-zinc-300 disabled:text-zinc-500 disabled:shadow-none"
        @click="onSubmit"
      >
        <LoaderCircle v-if="isSubmitting" class="h-4 w-4 animate-spin" :stroke-width="2" />
        {{ isSubmitting ? '正在提交' : '加入处理队列' }}
      </button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import {
  AlertCircle,
  CheckCircle2,
  KeyRound,
  LoaderCircle,
  Save,
  Search,
  Trash2,
} from 'lucide-vue-next';

export interface ModelOverride {
  nodeId: string;
  fieldName: string;
}

export interface NodeInfo {
  nodeId: string;
  nodeName?: string | null;
  fieldName: string;
  fieldValue?: string;
  fieldData?: string | null;
  description?: string | null;
  descriptionEn?: string | null;
}

export interface NodeInspection {
  nodes: NodeInfo[];
  recommended?: ModelOverride | null;
  reason: string;
}

const models = [
  {
    id: '2059943323456065537',
    name: '去水印修复',
    description: '清理画面水印和干扰元素，适合成片前的快速净化。',
    badge: 'Clean',
  },
  {
    id: '2023058520156934145',
    name: '4K 超分增强',
    description: 'FlashVSR Ultra Fast full，高质量 720p 至 4K 放大。',
    badge: '4K',
  },
  {
    id: '2023054533156409346',
    name: '2K 平衡超分',
    description: 'SeedVR2 1280x720 至 2560x1440，速度和质量更均衡。',
    badge: '2K',
  },
] as const;

const props = defineProps<{
  hasFile: boolean;
  hasPath: boolean;
  hasApiKey: boolean;
  apiKeyVerifiedAt: string | null;
  apiKeyLastError: string | null;
  isSubmitting: boolean;
  isSavingApiKey: boolean;
  isInspectingNodes: boolean;
  modelOverrides: Record<string, ModelOverride>;
  nodeInspection: NodeInspection | null;
}>();

const emit = defineEmits<{
  (event: 'submit', payload: { appId: string; appName: string; params: Record<string, unknown> }): void;
  (event: 'save-api-key', apiKey: string): void;
  (event: 'clear-api-key'): void;
  (event: 'inspect-model-nodes', appId: string): void;
  (event: 'save-model-override', payload: { appId: string; overrideConfig: ModelOverride | null }): void;
}>();

const selectedAppId = ref<string>(models[0].id);
const apiKeyInput = ref('');
const overrideNodeId = ref('');
const overrideFieldName = ref('');
const selectedNodeKey = ref('');

const selectedModel = computed(() => models.find((model) => model.id === selectedAppId.value) ?? models[0]);
const canSubmit = computed(() => props.hasFile && props.hasPath && props.hasApiKey && !props.isSubmitting);
const canSaveOverride = computed(() => Boolean(overrideNodeId.value.trim() && overrideFieldName.value.trim()));
const apiKeyStatusText = computed(() => {
  if (!props.hasApiKey) return '尚未配置，无法提交真实任务';
  if (props.apiKeyVerifiedAt) return `已验证 · ${props.apiKeyVerifiedAt}`;
  return '已保存到本机系统凭据';
});
const helperText = computed(() => {
  if (!props.hasApiKey) return '请先保存并验证 RunningHub API Key。';
  if (!props.hasFile) return '请先选择一个视频文件。';
  if (!props.hasPath) return '已看到文件名，但没有拿到本地路径；请用桌面文件选择器重新选择。';
  return `将使用“${selectedModel.value.name}”提交到 RunningHub。`;
});

watch(
  () => [selectedAppId.value, props.modelOverrides] as const,
  () => {
    const override = props.modelOverrides[selectedAppId.value];
    overrideNodeId.value = override?.nodeId ?? '';
    overrideFieldName.value = override?.fieldName ?? '';
    selectedNodeKey.value = override ? `${override.nodeId}::${override.fieldName}` : '';
  },
  { immediate: true, deep: true }
);

watch(
  () => props.nodeInspection,
  (inspection) => {
    if (!inspection?.recommended) return;
    selectedNodeKey.value = `${inspection.recommended.nodeId}::${inspection.recommended.fieldName}`;
    overrideNodeId.value = inspection.recommended.nodeId;
    overrideFieldName.value = inspection.recommended.fieldName;
  }
);

watch(selectedNodeKey, (key) => {
  if (!key || !props.nodeInspection) return;
  const node = props.nodeInspection.nodes.find((candidate) => nodeKey(candidate) === key);
  if (!node) return;
  overrideNodeId.value = node.nodeId;
  overrideFieldName.value = node.fieldName;
});

function nodeKey(node: NodeInfo) {
  return `${node.nodeId}::${node.fieldName}`;
}

function saveApiKey() {
  const apiKey = apiKeyInput.value.trim();
  if (!apiKey) return;
  emit('save-api-key', apiKey);
  apiKeyInput.value = '';
}

function saveOverride() {
  if (!canSaveOverride.value) return;
  emit('save-model-override', {
    appId: selectedAppId.value,
    overrideConfig: {
      nodeId: overrideNodeId.value.trim(),
      fieldName: overrideFieldName.value.trim(),
    },
  });
}

function clearOverride() {
  selectedNodeKey.value = '';
  overrideNodeId.value = '';
  overrideFieldName.value = '';
  emit('save-model-override', {
    appId: selectedAppId.value,
    overrideConfig: null,
  });
}

function onSubmit() {
  if (!canSubmit.value) return;

  emit('submit', {
    appId: selectedModel.value.id,
    appName: selectedModel.value.name,
    params: {},
  });
}
</script>
