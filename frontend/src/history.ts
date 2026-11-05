import type { ServerData } from '@/types'
import type { MessageKey } from '@/i18n'

export interface ChartPoint { name: number; value: [number, number] }
export interface ChartSeries { name: string; data: ChartPoint[]; color?: string }
export type MetricKey =
  | 'online4' | 'online6'
  | 'cpu' | 'load_1' | 'load_5' | 'load_15'
  | 'memory_percent' | 'swap_percent' | 'hdd_percent'
  | 'network_rx' | 'network_tx' | 'traffic_in' | 'traffic_out'
  | 'tcp' | 'udp' | 'process' | 'thread'
  | 'time_10010' | 'time_189' | 'time_10086'
  | 'ping_10010' | 'ping_189' | 'ping_10086'

export interface MetricOption {
  key: MetricKey
  labelKey: MessageKey
  unit: string
  decimals: number
  max?: number
  dependencies: string[]
  liveValue: (server: ServerData) => number | undefined
  historyValue: (values: Record<string, number>) => number | undefined
}

interface HistoryResponse { series?: Record<string, Array<[number, number]>> }
interface StoredSnapshot {
  host: string
  ts: number
  values: Partial<Record<MetricKey, number>>
  size: number
}

const HISTORY_API = '/api/history'
const LIVE_SAMPLE_INTERVAL = 5
const PERSIST_INTERVAL = 30
const GAP_THRESHOLD = 120
const MAX_CHART_POINTS = 900
const DB_NAME = 'serverstatus-web-history'
const DB_VERSION = 2
const SNAPSHOT_STORE = 'snapshots'
const META_STORE = 'metadata'
const CACHE_RETENTION = 30 * 24 * 60 * 60
const MAX_CACHE_BYTES = 512 * 1024 * 1024
const CACHE_TARGET_BYTES = 448 * 1024 * 1024

function percent(used: number | undefined, total: number | undefined) {
  if (used === undefined || total === undefined || total <= 0)
    return undefined
  return used * 100 / total
}

function directMetric(key: MetricKey,
  labelKey: MessageKey,
  unit: string,
  field: keyof ServerData,
  decimals = 1,
  max?: number): MetricOption {
  return {
    key,
    labelKey,
    unit,
    decimals,
    max,
    dependencies: [key],
    liveValue: server => typeof server[field] === 'number' ? server[field] as number : undefined,
    historyValue: values => values[key],
  }
}

export const METRIC_OPTIONS: MetricOption[] = [
  {
    key: 'online4',
    labelKey: 'metricOnline4',
    unit: '',
    decimals: 2,
    max: 1,
    dependencies: ['online4'],
    liveValue: server => server.online4 ? 1 : 0,
    historyValue: values => values.online4,
  },
  {
    key: 'online6',
    labelKey: 'metricOnline6',
    unit: '',
    decimals: 2,
    max: 1,
    dependencies: ['online6'],
    liveValue: server => server.online6 ? 1 : 0,
    historyValue: values => values.online6,
  },
  directMetric('cpu', 'metricCpu', '%', 'cpu', 1, 100),
  directMetric('load_1', 'metricLoad1', '', 'load_1', 2),
  directMetric('load_5', 'metricLoad5', '', 'load_5', 2),
  directMetric('load_15', 'metricLoad15', '', 'load_15', 2),
  {
    key: 'memory_percent',
    labelKey: 'metricMemoryPercent',
    unit: '%',
    decimals: 1,
    max: 100,
    dependencies: ['memory_used', 'memory_total'],
    liveValue: server => percent(server.memory_used, server.memory_total),
    historyValue: values => percent(values.memory_used, values.memory_total),
  },
  {
    key: 'swap_percent',
    labelKey: 'metricSwapPercent',
    unit: '%',
    decimals: 1,
    max: 100,
    dependencies: ['swap_used', 'swap_total'],
    liveValue: server => percent(server.swap_used, server.swap_total),
    historyValue: values => percent(values.swap_used, values.swap_total),
  },
  {
    key: 'hdd_percent',
    labelKey: 'metricDiskPercent',
    unit: '%',
    decimals: 1,
    max: 100,
    dependencies: ['hdd_used', 'hdd_total'],
    liveValue: server => percent(server.hdd_used, server.hdd_total),
    historyValue: values => percent(values.hdd_used, values.hdd_total),
  },
  directMetric('network_rx', 'metricNetworkRx', 'B/s', 'network_rx', 0),
  directMetric('network_tx', 'metricNetworkTx', 'B/s', 'network_tx', 0),
  {
    key: 'traffic_in',
    labelKey: 'metricTrafficIn',
    unit: 'B',
    decimals: 0,
    dependencies: ['traffic_in'],
    liveValue: () => undefined,
    historyValue: values => values.traffic_in,
  },
  {
    key: 'traffic_out',
    labelKey: 'metricTrafficOut',
    unit: 'B',
    decimals: 0,
    dependencies: ['traffic_out'],
    liveValue: () => undefined,
    historyValue: values => values.traffic_out,
  },
  directMetric('tcp', 'metricTcp', '', 'tcp_count', 0),
  directMetric('udp', 'metricUdp', '', 'udp_count', 0),
  directMetric('process', 'metricProcess', '', 'process_count', 0),
  directMetric('thread', 'metricThread', '', 'thread_count', 0),
  directMetric('time_10010', 'metricUnicomLatency', 'ms', 'time_10010', 0),
  directMetric('time_189', 'metricTelecomLatency', 'ms', 'time_189', 0),
  directMetric('time_10086', 'metricMobileLatency', 'ms', 'time_10086', 0),
  directMetric('ping_10010', 'metricUnicomLoss', '%', 'ping_10010', 1, 100),
  directMetric('ping_189', 'metricTelecomLoss', '%', 'ping_189', 1, 100),
  directMetric('ping_10086', 'metricMobileLoss', '%', 'ping_10086', 1, 100),
]

const options = new Map(METRIC_OPTIONS.map(option => [option.key, option]))
const memory = new Map<string, Map<MetricKey, Map<number, number>>>()
const pendingLoads = new Map<string, Promise<ChartPoint[]>>()
const pendingBatchLoads = new Map<string, Promise<Record<MetricKey, ChartPoint[]>>>()
const previousNetworkCounters = new Map<string, { incoming: number; outgoing: number; ts: number }>()
const lastPersisted = new Map<string, number>()
const loadedFrom = new Map<string, number>()
let databasePromise: Promise<IDBDatabase | undefined> | undefined
let cleanupPromise: Promise<void> | undefined

export function metricOption(metric: MetricKey) {
  return options.get(metric) || options.get('cpu')!
}

function historyHostKey(host: string, gid?: string) {
  return gid === undefined ? host : `${host}\u0000${gid}`
}

function metricPoints(host: string, metric: MetricKey) {
  let hostMetrics = memory.get(host)
  if (!hostMetrics) {
    hostMetrics = new Map()
    memory.set(host, hostMetrics)
  }
  let points = hostMetrics.get(metric)
  if (!points) {
    points = new Map()
    hostMetrics.set(metric, points)
  }
  return points
}

function openDatabase() {
  if (databasePromise)
    return databasePromise
  databasePromise = new Promise((resolve) => {
    if (!('indexedDB' in window)) {
      resolve(undefined)
      return
    }
    const request = indexedDB.open(DB_NAME, DB_VERSION)
    request.onupgradeneeded = () => {
      const database = request.result
      if (database.objectStoreNames.contains('metric_points'))
        database.deleteObjectStore('metric_points')
      if (!database.objectStoreNames.contains(SNAPSHOT_STORE)) {
        const store = database.createObjectStore(SNAPSHOT_STORE, { keyPath: ['host', 'ts'] })
        store.createIndex('by_timestamp', 'ts')
      }
      if (!database.objectStoreNames.contains(META_STORE))
        database.createObjectStore(META_STORE, { keyPath: 'key' })
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => resolve(undefined)
  })
  return databasePromise
}

function snapshotSize(snapshot: Omit<StoredSnapshot, 'size'>) {
  // Include a conservative allowance for IndexedDB keys and record overhead.
  return new Blob([JSON.stringify(snapshot)]).size + 256
}

async function readSnapshots(host: string, from: number, to: number) {
  const database = await openDatabase()
  if (!database)
    return []
  return new Promise<StoredSnapshot[]>((resolve) => {
    const transaction = database.transaction(SNAPSHOT_STORE, 'readonly')
    const request = transaction.objectStore(SNAPSHOT_STORE).getAll(
      IDBKeyRange.bound([host, from], [host, to]),
    )
    request.onsuccess = () => resolve(request.result as StoredSnapshot[])
    request.onerror = () => resolve([])
  })
}

async function writeSnapshots(incoming: Array<Omit<StoredSnapshot, 'size'>>) {
  if (incoming.length === 0)
    return
  const database = await openDatabase()
  if (!database)
    return

  const existing = await new Promise<Map<string, StoredSnapshot>>((resolve) => {
    const result = new Map<string, StoredSnapshot>()
    const transaction = database.transaction(SNAPSHOT_STORE, 'readonly')
    const store = transaction.objectStore(SNAPSHOT_STORE)
    incoming.forEach((snapshot) => {
      const request = store.get([snapshot.host, snapshot.ts])
      request.onsuccess = () => {
        if (request.result)
          result.set(`${snapshot.host}:${snapshot.ts}`, request.result as StoredSnapshot)
      }
    })
    transaction.oncomplete = () => resolve(result)
    transaction.onerror = () => resolve(result)
  })

  const snapshots = incoming.map((snapshot) => {
    const previous = existing.get(`${snapshot.host}:${snapshot.ts}`)
    const merged = {
      host: snapshot.host,
      ts: snapshot.ts,
      values: { ...previous?.values, ...snapshot.values },
    }
    return { ...merged, size: snapshotSize(merged) }
  })
  const sizeDelta = snapshots.reduce((total, snapshot) => {
    return total + snapshot.size - (existing.get(`${snapshot.host}:${snapshot.ts}`)?.size || 0)
  }, 0)

  await new Promise<void>((resolve) => {
    const transaction = database.transaction([SNAPSHOT_STORE, META_STORE], 'readwrite')
    const store = transaction.objectStore(SNAPSHOT_STORE)
    snapshots.forEach(snapshot => store.put(snapshot))
    const metadata = transaction.objectStore(META_STORE)
    const request = metadata.get('total_size')
    request.onsuccess = () => {
      const current = Number(request.result?.value || 0)
      metadata.put({ key: 'total_size', value: Math.max(0, current + sizeDelta) })
    }
    transaction.oncomplete = () => resolve()
    transaction.onerror = () => resolve()
  })
  void cleanupCache()
}

async function cleanupCache() {
  if (cleanupPromise)
    return cleanupPromise
  cleanupPromise = (async () => {
    const database = await openDatabase()
    if (!database)
      return
    const cutoff = Math.floor(Date.now() / 1000) - CACHE_RETENTION
    const estimate = await navigator.storage?.estimate?.()
    const originPressure = (estimate?.usage || 0) > 768 * 1024 * 1024
      || Boolean(estimate?.quota && estimate.usage && estimate.usage / estimate.quota > 0.8)
    await new Promise<void>((resolve) => {
      const transaction = database.transaction([SNAPSHOT_STORE, META_STORE], 'readwrite')
      const store = transaction.objectStore(SNAPSHOT_STORE)
      const metadata = transaction.objectStore(META_STORE)
      const sizeRequest = metadata.get('total_size')
      sizeRequest.onsuccess = () => {
        let totalSize = Number(sizeRequest.result?.value || 0)
        const compactToTarget = totalSize > MAX_CACHE_BYTES || originPressure
        const cursorRequest = store.index('by_timestamp').openCursor()
        cursorRequest.onsuccess = () => {
          const cursor = cursorRequest.result
          if (!cursor) {
            metadata.put({ key: 'total_size', value: Math.max(0, totalSize) })
            return
          }
          const snapshot = cursor.value as StoredSnapshot
          const expired = snapshot.ts < cutoff
          const overBudget = compactToTarget && totalSize > CACHE_TARGET_BYTES
          if (expired || overBudget) {
            totalSize -= snapshot.size || 0
            cursor.delete()
            cursor.continue()
            return
          }
          // The timestamp index is ordered, so no later record is expired.
          metadata.put({ key: 'total_size', value: Math.max(0, totalSize) })
        }
      }
      transaction.oncomplete = () => resolve()
      transaction.onerror = () => resolve()
    })
  })().finally(() => {
    cleanupPromise = undefined
  })
  return cleanupPromise
}

function restoreSnapshots(host: string, snapshots: StoredSnapshot[]) {
  for (const snapshot of snapshots) {
    for (const [metric, value] of Object.entries(snapshot.values)) {
      if (value !== undefined)
        metricPoints(host, metric as MetricKey).set(snapshot.ts, value)
    }
  }
}

function responseStep(rangeSeconds: number) {
  const minimumResolution = rangeSeconds <= 15 * 60
    ? 1
    : rangeSeconds <= 30 * 24 * 60 * 60
      ? 60
      : rangeSeconds <= 90 * 24 * 60 * 60
        ? 5 * 60
        : rangeSeconds <= 365 * 24 * 60 * 60
          ? 60 * 60
          : rangeSeconds <= 3 * 365 * 24 * 60 * 60
            ? 24 * 60 * 60
            : 30 * 24 * 60 * 60
  const targetStep = Math.ceil(rangeSeconds / MAX_CHART_POINTS)
  return Math.max(minimumResolution, Math.ceil(targetStep / minimumResolution) * minimumResolution)
}

function missingRanges(timestamps: number[], from: number, to: number, step: number) {
  if (timestamps.length === 0)
    return [[from, to] as [number, number]]
  const ranges: Array<[number, number]> = []
  const maxGap = Math.max(GAP_THRESHOLD, step * 2)
  if (timestamps[0] - from > maxGap)
    ranges.push([from, timestamps[0] - 1])
  for (let index = 1; index < timestamps.length; index++) {
    if (timestamps[index] - timestamps[index - 1] > maxGap)
      ranges.push([timestamps[index - 1] + 1, timestamps[index] - 1])
  }
  if (to - timestamps[timestamps.length - 1] > maxGap)
    ranges.push([timestamps[timestamps.length - 1] + 1, to])
  return ranges.length > 8 ? [[from, to] as [number, number]] : ranges
}

async function fetchServerHistory(host: string, gid: string | undefined, option: MetricOption, from: number, to: number, step: number) {
  const query = new URLSearchParams({
    host,
    from: String(from),
    to: String(to),
    step: String(step),
    metrics: option.dependencies.join(','),
  })
  if (gid !== undefined)
    query.set('gid', gid)
  const response = await fetch(`${HISTORY_API}?${query}`)
  if (!response.ok)
    return []
  const data = await response.json() as HistoryResponse
  const valuesByTimestamp = new Map<number, Record<string, number>>()
  for (const dependency of option.dependencies) {
    for (const [ts, value] of data.series?.[dependency] || []) {
      const values = valuesByTimestamp.get(ts) || {}
      values[dependency] = value
      valuesByTimestamp.set(ts, values)
    }
  }
  return [...valuesByTimestamp.entries()].flatMap(([ts, values]) => {
    const value = option.historyValue(values)
    return value === undefined ? [] : [[ts, value] as [number, number]]
  })
}

function toChartPoints(host: string, metric: MetricKey, from: number) {
  const visible = [...metricPoints(host, metric).entries()]
    .filter(([ts]) => ts >= from)
    .sort(([left], [right]) => left - right)
  const stride = Math.max(1, Math.ceil(visible.length / MAX_CHART_POINTS))
  return visible
    .filter((_, index) => index % stride === 0 || index === visible.length - 1)
    .map(([ts, value]) => ({ name: ts * 1000, value: [ts * 1000, value] as [number, number] }))
}

export function appendLiveMetrics(server: ServerData, fallbackTimestamp: number) {
  const ts = server.latest_ts || fallbackTimestamp
  if (!ts)
    return
  const hostKey = historyHostKey(server.name, server.gid)
  const previousCounters = previousNetworkCounters.get(hostKey)
  const trafficValues: Partial<Record<MetricKey, number>> = {}
  if (!previousCounters || ts - previousCounters.ts >= LIVE_SAMPLE_INTERVAL) {
    trafficValues.traffic_in = previousCounters ? Math.max(0, server.network_in - previousCounters.incoming) : 0
    trafficValues.traffic_out = previousCounters ? Math.max(0, server.network_out - previousCounters.outgoing) : 0
    previousNetworkCounters.set(hostKey, {
      incoming: server.network_in,
      outgoing: server.network_out,
      ts,
    })
  }

  const snapshotValues: Partial<Record<MetricKey, number>> = {}
  for (const option of METRIC_OPTIONS) {
    const value = trafficValues[option.key] ?? option.liveValue(server)
    if (value === undefined || !Number.isFinite(value))
      continue
    snapshotValues[option.key] = value
    const points = metricPoints(hostKey, option.key)
    const timestamps = [...points.keys()]
    const lastTimestamp = timestamps[timestamps.length - 1]
    if (lastTimestamp !== undefined && ts - lastTimestamp < LIVE_SAMPLE_INTERVAL) {
      if (lastTimestamp === ts)
        points.set(ts, value)
      continue
    }
    points.set(ts, value)
  }

  const previousPersist = lastPersisted.get(hostKey) || 0
  if (ts - previousPersist >= PERSIST_INTERVAL) {
    lastPersisted.set(hostKey, ts)
    void writeSnapshots([{ host: hostKey, ts, values: snapshotValues }])
  }
}

export function localMetricHistory(host: string, metric: MetricKey, rangeSeconds: number, gid?: string) {
  return toChartPoints(historyHostKey(host, gid), metric, Math.floor(Date.now() / 1000) - rangeSeconds)
}

export function ensureMetricHistory(host: string, metric: MetricKey, rangeSeconds: number, gid?: string) {
  const to = Math.floor(Date.now() / 1000)
  const from = to - rangeSeconds
  const hostKey = historyHostKey(host, gid)
  const key = `${hostKey}:${metric}:${rangeSeconds}`
  const existing = pendingLoads.get(key)
  if (existing)
    return existing
  const promise = (async () => {
    const option = metricOption(metric)
    const points = metricPoints(hostKey, metric)
    if ((loadedFrom.get(hostKey) || Number.POSITIVE_INFINITY) > from) {
      restoreSnapshots(hostKey, await readSnapshots(hostKey, from, to))
      loadedFrom.set(hostKey, from)
    }
    const localTimestamps = [...points.keys()]
      .filter(ts => ts >= from && ts <= to)
      .sort((left, right) => left - right)
    const persisted: Array<Omit<StoredSnapshot, 'size'>> = []
    for (const [gapFrom, gapTo] of missingRanges(localTimestamps, from, to, responseStep(rangeSeconds))) {
      try {
        const history = await fetchServerHistory(host, gid, option, gapFrom, gapTo, responseStep(rangeSeconds))
        history.forEach(([ts, value]) => {
          if (!points.has(ts))
            points.set(ts, value)
          persisted.push({ host: hostKey, ts, values: { [metric]: value } })
        })
      }
      catch {
        // Keep real-time charts available when history is disabled or temporarily unavailable.
      }
    }
    await writeSnapshots(persisted)
    return localMetricHistory(host, metric, rangeSeconds, gid)
  })().finally(() => pendingLoads.delete(key))
  pendingLoads.set(key, promise)
  return promise
}

function mergeRanges(ranges: Array<[number, number]>) {
  const sorted = ranges
    .filter(([from, to]) => from <= to)
    .sort(([left], [right]) => left - right)
  const merged: Array<[number, number]> = []
  for (const range of sorted) {
    const previous = merged[merged.length - 1]
    if (!previous || range[0] > previous[1] + 1)
      merged.push([...range])
    else
      previous[1] = Math.max(previous[1], range[1])
  }
  return merged.length > 8 && merged.length > 0
    ? [[merged[0][0], merged[merged.length - 1][1]] as [number, number]]
    : merged
}

async function fetchServerHistories(
  host: string,
  gid: string | undefined,
  requested: MetricOption[],
  from: number,
  to: number,
  step: number,
) {
  const dependencies = [...new Set(requested.flatMap(option => option.dependencies))]
  const query = new URLSearchParams({
    host,
    from: String(from),
    to: String(to),
    step: String(step),
    metrics: dependencies.join(','),
  })
  if (gid !== undefined)
    query.set('gid', gid)
  const response = await fetch(`${HISTORY_API}?${query}`)
  if (!response.ok)
    return new Map<MetricKey, Array<[number, number]>>()
  const data = await response.json() as HistoryResponse
  const valuesByTimestamp = new Map<number, Record<string, number>>()
  for (const dependency of dependencies) {
    for (const [ts, value] of data.series?.[dependency] || []) {
      const values = valuesByTimestamp.get(ts) || {}
      values[dependency] = value
      valuesByTimestamp.set(ts, values)
    }
  }
  const result = new Map<MetricKey, Array<[number, number]>>()
  for (const option of requested) {
    const values = [...valuesByTimestamp.entries()].flatMap(([ts, dependencies]) => {
      const value = option.historyValue(dependencies)
      return value === undefined ? [] : [[ts, value] as [number, number]]
    })
    result.set(option.key, values)
  }
  return result
}

/** Load several chart series with one history request per missing time span. */
export function ensureMetricsHistory(host: string, metrics: MetricKey[], rangeSeconds: number, gid?: string) {
  const uniqueMetrics = [...new Set(metrics)]
  const hostKey = historyHostKey(host, gid)
  const key = `${hostKey}:${uniqueMetrics.slice().sort().join(',')}:${rangeSeconds}`
  const existing = pendingBatchLoads.get(key)
  if (existing)
    return existing

  const promise = (async () => {
    const to = Math.floor(Date.now() / 1000)
    const from = to - rangeSeconds
    const step = responseStep(rangeSeconds)
    if ((loadedFrom.get(hostKey) || Number.POSITIVE_INFINITY) > from) {
      restoreSnapshots(hostKey, await readSnapshots(hostKey, from, to))
      loadedFrom.set(hostKey, from)
    }

    const ranges = mergeRanges(uniqueMetrics.flatMap((metric) => {
      const timestamps = [...metricPoints(hostKey, metric).keys()]
        .filter(ts => ts >= from && ts <= to)
        .sort((left, right) => left - right)
      return missingRanges(timestamps, from, to, step)
    }))
    const requested = uniqueMetrics.map(metricOption)
    const snapshots = new Map<number, Partial<Record<MetricKey, number>>>()
    for (const [gapFrom, gapTo] of ranges) {
      try {
        const histories = await fetchServerHistories(host, gid, requested, gapFrom, gapTo, step)
        for (const [metric, points] of histories) {
          const target = metricPoints(hostKey, metric)
          for (const [ts, value] of points) {
            if (!target.has(ts))
              target.set(ts, value)
            const values = snapshots.get(ts) || {}
            values[metric] = value
            snapshots.set(ts, values)
          }
        }
      }
      catch {
        // The live browser samples remain useful if server history is disabled.
      }
    }
    await writeSnapshots([...snapshots].map(([ts, values]) => ({ host: hostKey, ts, values })))
    return Object.fromEntries(uniqueMetrics.map(metric => [
      metric,
      toChartPoints(hostKey, metric, from),
    ])) as Record<MetricKey, ChartPoint[]>
  })().finally(() => pendingBatchLoads.delete(key))

  pendingBatchLoads.set(key, promise)
  return promise
}
