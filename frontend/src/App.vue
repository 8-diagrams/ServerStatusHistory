<template>
  <div
    v-if="serverData"
    class="sticky top-0 z-40 -mx-3 sm:-mx-6 mb-4 border-b bg-white/95 px-3 py-2 shadow-sm backdrop-blur sm:px-6"
  >
    <div class="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
      <div class="flex items-center gap-2">
        <span class="font-medium">布局</span>
        <div class="rounded-lg overflow-hidden border h-[30px]">
          <button
            class="p-1 hover:bg-gray-200 text-gray-500 transition-colors"
            :class="{ '!bg-gray-300 !text-black': settings.layout === 'grid' }"
            title="网格布局"
            @click="settings.layout = 'grid'"
          >
            <IconLayoutGrid class="w-5 h-5" />
          </button>
          <button
            class="p-1 hover:bg-gray-200 text-gray-500 transition-colors"
            :class="{ '!bg-gray-300 !text-black': settings.layout === 'flex' }"
            title="自适应布局"
            @click="settings.layout = 'flex'"
          >
            <IconLayoutFlex class="w-5 h-5" />
          </button>
          <button
            class="p-1 hover:bg-gray-200 text-gray-500 transition-colors"
            :class="{ '!bg-gray-300 !text-black': settings.layout === 'list' }"
            title="列表布局"
            @click="settings.layout = 'list'"
          >
            <IconLayoutList class="w-5 h-5" />
          </button>
        </div>
      </div>
      <label class="flex items-center gap-2">
        <span class="font-medium">精简</span>
        <Switch v-model="settings.compactMode" />
      </label>
      <label class="flex items-center gap-2">
        <span class="font-medium">趋势图</span>
        <Switch v-model="settings.showMetricChart" />
      </label>
      <label v-if="settings.showMetricChart" class="flex items-center gap-2">
        <span class="font-medium">指标</span>
        <select v-model="settings.chartMetric" class="max-w-[150px] rounded border px-2 py-1">
          <option v-for="option in METRIC_OPTIONS" :key="option.key" :value="option.key">
            {{ option.label }}
          </option>
        </select>
      </label>
      <label v-if="settings.showMetricChart" class="flex items-center gap-2">
        <span class="font-medium">范围</span>
        <select v-model="settings.chartHistoryKeep" class="rounded border px-2 py-1">
          <option value="300">5分钟</option>
          <option value="3600">1小时</option>
          <option value="21600">6小时</option>
          <option value="86400">24小时</option>
          <option value="604800">7天</option>
          <option value="2592000">30天</option>
        </select>
      </label>
      <span v-if="settings.showMetricChart" class="text-xs text-gray-500">
        实时数据优先使用浏览器本地积累，缺失区间才查询服务端。
      </span>
    </div>
  </div>
  <div v-if="loading" class="w-fit mx-auto my-2 rounded-lg bg-gray-100 px-4 py-2">
    加载中
  </div>
  <div v-if="error" class="w-fit mx-auto my-2 rounded-lg bg-gray-100 px-4 py-2">
    数据加载失败，请尝试刷新页面或检查 ServerStatus 服务端状态
  </div>
  <div v-if="serverData" class="flex flex-col gap-4 lg:flex-row lg:items-start">
    <aside class="w-full shrink-0 rounded-xl border bg-white p-3 shadow-sm lg:sticky lg:top-16 lg:w-56">
      <div class="mb-2 px-2 text-xs font-semibold uppercase tracking-wide text-gray-500">
        主机视图
      </div>
      <div class="flex gap-2 lg:flex-col">
        <button
          class="flex flex-1 items-center justify-between rounded-lg px-3 py-2 text-left transition-colors lg:flex-none"
          :class="settings.viewMode === 'directory' ? 'bg-gray-800 text-white' : 'hover:bg-gray-100'"
          @click="switchView('directory')"
        >
          <span>目录</span>
          <span class="text-xs opacity-70">{{ directories.length }}</span>
        </button>
        <button
          class="flex flex-1 items-center justify-between rounded-lg px-3 py-2 text-left transition-colors lg:flex-none"
          :class="settings.viewMode === 'tag' ? 'bg-gray-800 text-white' : 'hover:bg-gray-100'"
          @click="switchView('tag')"
        >
          <span>标签</span>
          <span class="text-xs opacity-70">{{ availableTags.length }}</span>
        </button>
      </div>
      <div class="mt-3 border-t pt-3">
        <button
          class="mb-1 flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition-colors"
          :class="isBrowseAll ? 'bg-gray-100 font-medium' : 'hover:bg-gray-100'"
          @click="clearBrowseSelection"
        >
          <span>{{ settings.viewMode === 'tag' ? '全部标签' : '全部主机' }}</span>
          <span class="text-xs text-gray-500">{{ serverData.servers.length }}</span>
        </button>
        <template v-if="settings.viewMode === 'directory'">
          <button
            v-for="directory in directories" :key="directory.key"
            class="flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition-colors"
            :class="settings.directoryFilter === directory.key ? 'bg-blue-50 font-medium text-blue-700' : 'hover:bg-gray-100'"
            @click="settings.directoryFilter = directory.key"
          >
            <span class="truncate">{{ directory.label }}</span>
            <span class="ml-2 text-xs text-gray-500">{{ directory.count }}</span>
          </button>
        </template>
        <template v-else>
          <button
            v-for="tag in availableTags" :key="tag"
            class="flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition-colors"
            :class="settings.tagFilter.toLocaleLowerCase() === tag.toLocaleLowerCase() ? 'bg-blue-50 font-medium text-blue-700' : 'hover:bg-gray-100'"
            @click="settings.tagFilter = tag"
          >
            <span class="truncate">{{ tag }}</span>
            <span class="ml-2 text-xs text-gray-500">{{ tagCounts[tag.toLocaleLowerCase()] || 0 }}</span>
          </button>
          <div v-if="availableTags.length === 0" class="px-3 py-2 text-sm text-gray-500">
            暂无标签
          </div>
        </template>
      </div>
    </aside>
    <main class="min-w-0 flex-1">
      <Transition name="popup-bottom">
        <div class="flex flex-col gap-y-5">
          <section v-for="group in serverGroups" :key="group.key">
            <div v-if="group.label" class="mb-2 flex items-center gap-2">
              <h2 class="font-bold text-lg">
                {{ group.label }}
              </h2>
              <span class="text-xs text-gray-500 bg-gray-100 rounded-full px-2 py-0.5">
                {{ group.servers.length }} 台
              </span>
            </div>
            <div
              :class="{
                'grid gap-x-4 gap-y-3': settings.layout === 'grid',
                'flex flex-wrap gap-x-4 gap-y-3': settings.layout === 'flex',
                'flex flex-col gap-y-3': settings.layout === 'list',
              }"
              :style="{
                gridTemplateColumns: `repeat(${serverCardCount}, minmax(0, 1fr))`,
              }"
            >
              <template v-if="settings.layout === 'grid' || settings.layout === 'flex'">
                <ServerCard
                  v-for="server in group.servers" :key="`${group.key}:${server.name}`"
                  :server="server"
                  :compact-mode="settings.compactMode"
                  :show-metric-chart="settings.showMetricChart"
                  :chart-history="metricHistories[server.name] || []"
                  :chart-label="selectedMetric.label"
                  :chart-unit="selectedMetric.unit"
                  :chart-decimals="selectedMetric.decimals"
                  :chart-max="selectedMetric.max"
                  :class="{
                    'col-span-1': settings.layout === 'grid',
                    'flex-1 min-w-[300px]': settings.layout === 'flex',
                  }"
                />
              </template>
              <template v-if="settings.layout === 'list'">
                <ServerItem
                  v-for="server in group.servers" :key="`${group.key}:${server.name}`"
                  :server="server"
                  :compact-mode="settings.compactMode"
                  class="col-span-1"
                />
              </template>
            </div>
          </section>
          <div v-if="serverGroups.length === 0" class="text-center text-gray-500 py-8">
            当前视图下没有服务器
          </div>
        </div>
      </Transition>
    </main>
  </div>
  <div class="h-16" />
</template>

<script setup lang="ts">
import { useLocalStorage, useWindowSize } from '@vueuse/core'
import type { ChartPoint, MetricKey } from './history'
import { METRIC_OPTIONS, appendLiveMetrics, ensureMetricHistory, localMetricHistory, metricOption } from './history'
import type { ServerData } from './types'

const JSON_API = '/json/stats.json'
const CARD_WIDTH = 350
const MIN_FETCH_INTERVAL = 500

const { width: WindowWidth } = useWindowSize()

const settings = useLocalStorage('sstl-settings', {
  layout: 'grid',
  compactMode: false,
  viewMode: 'directory' as 'directory' | 'tag',
  directoryFilter: '',
  tagFilter: '',
  showMetricChart: false,
  chartMetric: 'cpu' as MetricKey,
  chartHistoryKeep: 300,
}, {
  mergeDefaults: true,
})

const serverData = ref<{
  updated: number
  servers: ServerData[]
}>()
const loading = ref(true)
const error = ref(false)
const fetching = ref(false)
const latestUpdated = ref(0)
const timer = ref<Worker>()
const metricHistories = ref<Record<string, ChartPoint[]>>({})
const historyRequests = new Map<string, number>()
const HISTORY_RETRY_INTERVAL = 5 * 60 * 1000

const serverCardCount = computed(() => {
  return Math.floor(WindowWidth.value / CARD_WIDTH) || 1
})
const selectedMetric = computed(() => metricOption(settings.value.chartMetric as MetricKey))
const allServers = computed(() => serverData.value?.servers || [])
const directories = computed(() => {
  const groups = new Map<string, { key: string; label: string; count: number }>()
  for (const server of allServers.value) {
    const key = server.gid?.trim() || '__ungrouped__'
    const label = key === '__ungrouped__' ? '未分组' : key
    const group = groups.get(key) || { key, label, count: 0 }
    group.count += 1
    groups.set(key, group)
  }
  return [...groups.values()].sort((left, right) => {
    if (left.key === '__ungrouped__')
      return 1
    if (right.key === '__ungrouped__')
      return -1
    return left.label.localeCompare(right.label)
  })
})
const availableTags = computed(() => {
  const tags = new Map<string, string>()
  for (const server of serverData.value?.servers || []) {
    for (const tag of server.tags || []) {
      const normalized = tag.trim()
      if (normalized && !tags.has(normalized.toLocaleLowerCase()))
        tags.set(normalized.toLocaleLowerCase(), normalized)
    }
  }
  return [...tags.values()].sort((left, right) => left.localeCompare(right))
})
const tagCounts = computed(() => {
  const counts: Record<string, number> = {}
  for (const server of allServers.value) {
    for (const tag of new Set(server.tags || [])) {
      const key = tag.trim().toLocaleLowerCase()
      if (key)
        counts[key] = (counts[key] || 0) + 1
    }
  }
  return counts
})
const isBrowseAll = computed(() => settings.value.viewMode === 'directory'
  ? !settings.value.directoryFilter
  : !settings.value.tagFilter)
const filteredServers = computed(() => {
  if (settings.value.viewMode === 'directory') {
    if (!settings.value.directoryFilter)
      return allServers.value
    return allServers.value.filter(server => (
      (server.gid?.trim() || '__ungrouped__') === settings.value.directoryFilter
    ))
  }
  const selected = settings.value.tagFilter.toLocaleLowerCase()
  if (!selected)
    return allServers.value
  return allServers.value.filter(server => (
    server.tags || []
  ).some(tag => tag.toLocaleLowerCase() === selected))
})
const serverGroups = computed(() => {
  if (settings.value.viewMode === 'directory') {
    return filteredServers.value.length
      ? [{ key: settings.value.directoryFilter || 'all', label: '', servers: filteredServers.value }]
      : []
  }

  const selected = settings.value.tagFilter.toLocaleLowerCase()
  if (selected) {
    return filteredServers.value.length
      ? [{ key: selected, label: settings.value.tagFilter, servers: filteredServers.value }]
      : []
  }

  const groups = new Map<string, { key: string; label: string; servers: ServerData[] }>()
  for (const server of filteredServers.value) {
    const tags = server.tags?.length ? server.tags : ['未标记']
    for (const tag of tags) {
      const key = tag.toLocaleLowerCase()
      const group = groups.get(key) || { key, label: tag, servers: [] }
      group.servers.push(server)
      groups.set(key, group)
    }
  }
  return [...groups.values()].sort((left, right) => left.label.localeCompare(right.label))
})

function clearBrowseSelection() {
  settings.value.directoryFilter = ''
  settings.value.tagFilter = ''
}

function switchView(view: 'directory' | 'tag') {
  settings.value.viewMode = view
  if (view === 'directory')
    settings.value.tagFilter = ''
  else
    settings.value.directoryFilter = ''
}

onMounted(() => {
  fetch(JSON_API)
    .then(res => res.json())
    .then((data) => {
      acceptServerData(data)
      timer.value = new Worker(new URL('./worker/timer.js', import.meta.url))
      timer.value.addEventListener('message', () => {
        fetchData()
      })
      timer.value.postMessage('start')
    })
    .catch(() => {
      error.value = true
    })
    .finally(() => {
      loading.value = false
    })
})

onUnmounted(() => {
  if (timer.value)
    timer.value.postMessage('stop')
})

function fetchData() {
  if (fetching.value)
    return
  if (Date.now() - latestUpdated.value < MIN_FETCH_INTERVAL)
    return
  fetching.value = true
  fetch(JSON_API)
    .then(res => res.json())
    .then((data) => {
      acceptServerData(data)
      error.value = false
    })
    .catch(() => {
      error.value = true
    })
    .finally(() => {
      fetching.value = false
      latestUpdated.value = Date.now()
    })
}

function acceptServerData(data: { updated: number; servers: ServerData[] }) {
  serverData.value = data
  const range = Number(settings.value.chartHistoryKeep)
  const metric = settings.value.chartMetric as MetricKey
  for (const server of data.servers) {
    appendLiveMetrics(server, data.updated)
    metricHistories.value[server.name] = localMetricHistory(server.name, metric, range)
    if (settings.value.showMetricChart)
      refreshServerMetricHistory(server, metric, range).catch(() => undefined)
  }
}

async function refreshMetricHistory() {
  if (!serverData.value || !settings.value.showMetricChart)
    return
  const range = Number(settings.value.chartHistoryKeep)
  const metric = settings.value.chartMetric as MetricKey
  await Promise.all(serverData.value.servers.map(server => refreshServerMetricHistory(server, metric, range, true)))
}

async function refreshServerMetricHistory(server: ServerData, metric: MetricKey, range: number, force = false) {
  const requestKey = `${server.name}:${metric}:${range}`
  const previousRequest = historyRequests.get(requestKey) || 0
  if (!force && Date.now() - previousRequest < HISTORY_RETRY_INTERVAL)
    return
  historyRequests.set(requestKey, Date.now())
  metricHistories.value[server.name] = await ensureMetricHistory(server.name, metric, range)
}

watch(
  () => [settings.value.showMetricChart, settings.value.chartMetric, settings.value.chartHistoryKeep],
  async () => refreshMetricHistory(),
)
</script>

<style>
.popup-bottom-enter-active,
.popup-bottom-leave-active {
  transition: all 0.3s ease;
}

.popup-bottom-enter-from,
.popup-bottom-leave-to {
  opacity: 0;
  transform: translateY(10px)
}

.popup-right-enter-active,
.popup-right-leave-active {
  transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.popup-right-enter-from,
.popup-right-leave-to {
  opacity: 0;
  transform: translate(10px, -10px) scale(0.9);
}
</style>
