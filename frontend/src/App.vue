<template>
  <div class="absolute right-3 sm:right-6 top-4 flex flex-col items-end">
    <button @click.stop="showSettingPanel = !showSettingPanel">
      <IconSettings class="text-gray-500 hover:text-black w-6 h-6 transition-colors" />
    </button>
    <Transition name="popup-right">
      <div
        v-show="showSettingPanel"
        class="setting-panel border shadow-md rounded-lg px-4 py-3 z-50 bg-white mt-2 min-w-[200px]"
      >
        <h2 class="font-bold text-lg">
          设置
        </h2>
        <div class="flex flex-col gap-1">
          <SettingItem title="布局模式">
            <div class="rounded-lg overflow-hidden border h-[30px]">
              <button
                class="p-1 hover:bg-gray-200 text-gray-500 transition-colors"
                :class="{
                  '!bg-gray-300 !text-black': settings.layout === 'grid',
                }"
                @click="settings.layout = 'grid'"
              >
                <IconLayoutGrid class="w-5 h-5" />
              </button>
              <button
                class="p-1 hover:bg-gray-200 text-gray-500 transition-colors"
                :class="{
                  '!bg-gray-300 !text-black': settings.layout === 'flex',
                }"
                @click="settings.layout = 'flex'"
              >
                <IconLayoutFlex class="w-5 h-5" />
              </button>
              <button
                class="p-1 hover:bg-gray-200 text-gray-500 transition-colors"
                :class="{
                  '!bg-gray-300 !text-black': settings.layout === 'list',
                }"
                @click="settings.layout = 'list'"
              >
                <IconLayoutList class="w-5 h-5" />
              </button>
            </div>
          </SettingItem>
          <SettingItem title="精简显示">
            <Switch v-model="settings.compactMode" />
          </SettingItem>
          <SettingItem v-if="availableTags.length" title="标签筛选">
            <select v-model="settings.tagFilter" class="max-w-[130px]">
              <option value="">
                全部
              </option>
              <option v-for="tag in availableTags" :key="tag" :value="tag">
                {{ tag }}
              </option>
            </select>
          </SettingItem>
          <SettingItem v-if="availableTags.length" title="按标签分组">
            <Switch v-model="settings.groupByTag" />
          </SettingItem>
          <SettingItem title="指标图表">
            <Switch v-model="settings.showMetricChart" />
          </SettingItem>
          <SettingItem v-show="settings.showMetricChart" title="指标">
            <select v-model="settings.chartMetric">
              <option v-for="option in METRIC_OPTIONS" :key="option.key" :value="option.key">
                {{ option.label }}
              </option>
            </select>
          </SettingItem>
          <SettingItem v-show="settings.showMetricChart" title="记录时间">
            <select v-model="settings.chartHistoryKeep">
              <option value="300">
                5分钟
              </option>
              <option value="3600">
                1小时
              </option>
              <option value="21600">
                6小时
              </option>
              <option value="86400">
                24小时
              </option>
              <option value="604800">
                7天
              </option>
              <option value="2592000">
                30天
              </option>
            </select>
          </SettingItem>
          <div v-show="settings.showMetricChart" class="text-xs text-gray-500 pt-1">
            页面打开后优先使用浏览器实时积累的数据，仅对本地缺失区间查询服务端历史。
          </div>
        </div>
      </div>
    </Transition>
  </div>
  <div v-if="loading" class="w-fit mx-auto my-2 rounded-lg bg-gray-100 px-4 py-2">
    加载中
  </div>
  <div v-if="error" class="w-fit mx-auto my-2 rounded-lg bg-gray-100 px-4 py-2">
    数据加载失败，请尝试刷新页面或检查 ServerStatus 服务端状态
  </div>
  <Transition name="popup-bottom">
    <div v-if="serverData" class="flex flex-col gap-y-5">
      <section v-for="group in serverGroups" :key="group.key">
        <div v-if="settings.groupByTag" class="flex items-center gap-2 mb-2">
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
        当前标签下没有服务器
      </div>
    </div>
  </Transition>
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
  tagFilter: '',
  groupByTag: false,
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
const showSettingPanel = ref(false)
const latestUpdated = ref(0)
const timer = ref<Worker>()
const metricHistories = ref<Record<string, ChartPoint[]>>({})
const historyRequests = new Map<string, number>()
const HISTORY_RETRY_INTERVAL = 5 * 60 * 1000

const serverCardCount = computed(() => {
  return Math.floor(WindowWidth.value / CARD_WIDTH) || 1
})
const selectedMetric = computed(() => metricOption(settings.value.chartMetric as MetricKey))
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
const filteredServers = computed(() => {
  const selected = settings.value.tagFilter.toLocaleLowerCase()
  if (!selected)
    return serverData.value?.servers || []
  return (serverData.value?.servers || []).filter(server => (
    server.tags || []
  ).some(tag => tag.toLocaleLowerCase() === selected))
})
const serverGroups = computed(() => {
  if (!settings.value.groupByTag) {
    return filteredServers.value.length
      ? [{ key: 'all', label: '', servers: filteredServers.value }]
      : []
  }

  const selected = settings.value.tagFilter
  if (selected) {
    return filteredServers.value.length
      ? [{ key: selected, label: selected, servers: filteredServers.value }]
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

onMounted(() => {
  document.addEventListener('click', (e) => {
    if (showSettingPanel.value && !(e.target as HTMLElement).closest('.setting-panel'))
      showSettingPanel.value = false
  })
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
