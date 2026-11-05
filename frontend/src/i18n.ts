import { ref, watch } from 'vue'

export type Locale = 'zh-CN' | 'zh-TW' | 'en'

const en = {
  appTitle: 'Server monitoring',
  language: 'Language',
  layoutMode: 'Layout',
  layoutGrid: 'Grid layout',
  layoutAdaptive: 'Adaptive layout',
  layoutList: 'List layout',
  compact: 'Compact',
  trendChart: 'Trend chart',
  trendMetric: 'Trend metric',
  trendRange: 'Trend range',
  range5Minutes: '5 minutes',
  range15Minutes: '15 minutes',
  range1Hour: '1 hour',
  range6Hours: '6 hours',
  range24Hours: '24 hours',
  range7Days: '7 days',
  range30Days: '30 days',
  range1Year: '1 year',
  range3Years: '3 years',
  range5Years: '5 years',
  range10Years: '10 years',
  loadError: 'Unable to load data. Refresh the page or check the server.',
  allHosts: 'All hosts',
  defaultDirectory: 'Default',
  tagFilter: 'Tag filter · Multiple',
  allTags: 'All tags',
  noTags: 'No tags',
  online: 'Online',
  offline: 'Offline',
  loadingHosts: 'Loading host data…',
  noHosts: 'No hosts match the current filters',
  directoryAllDescription: 'All hosts in the selected directory. Add tag filters from the sidebar.',
  directoryTagDescription: 'Selected directory AND ({tags})',
  lastReportTime: 'Last report',
  uptime: 'Uptime',
  uptimeDay: '{count} day',
  uptimeDays: '{count} days',
  load: 'Load',
  noData: 'No data',
  noLoadData: 'No load data',
  memory: 'Memory',
  disk: 'Disk',
  network: 'Network',
  traffic: 'Traffic',
  download: 'Download',
  upload: 'Upload',
  process: 'Processes',
  thread: 'Threads',
  flagAlt: '{location} flag',
  osAlt: '{os} operating system',
  metricCpu: 'CPU',
  metricLoad1: '1-minute load',
  metricLoad5: '5-minute load',
  metricLoad15: '15-minute load',
  metricMemoryPercent: 'Memory usage',
  metricSwapPercent: 'Swap usage',
  metricDiskPercent: 'Disk usage',
  metricNetworkRx: 'Download rate',
  metricNetworkTx: 'Upload rate',
  metricTrafficIn: 'Interval download traffic',
  metricTrafficOut: 'Interval upload traffic',
  metricTcp: 'TCP connections',
  metricUdp: 'UDP connections',
  metricProcess: 'Process count',
  metricThread: 'Thread count',
  metricUnicomLatency: 'China Unicom latency',
  metricTelecomLatency: 'China Telecom latency',
  metricMobileLatency: 'China Mobile latency',
  metricUnicomLoss: 'China Unicom packet loss',
  metricTelecomLoss: 'China Telecom packet loss',
  metricMobileLoss: 'China Mobile packet loss',
  carrierUnicom: 'Unicom',
  carrierTelecom: 'Telecom',
  carrierMobile: 'Mobile',
  poweredBy: 'Powered by',
  theme: 'Theme',
  by: 'by',
} as const

export type MessageKey = keyof typeof en

const zhCN: Record<MessageKey, string> = {
  appTitle: '服务器监控',
  language: '语言',
  layoutMode: '布局模式',
  layoutGrid: '网格布局',
  layoutAdaptive: '自适应布局',
  layoutList: '列表布局',
  compact: '精简',
  trendChart: '趋势图',
  trendMetric: '趋势指标',
  trendRange: '趋势范围',
  range5Minutes: '5 分钟',
  range15Minutes: '15 分钟',
  range1Hour: '1 小时',
  range6Hours: '6 小时',
  range24Hours: '24 小时',
  range7Days: '7 天',
  range30Days: '30 天',
  range1Year: '1 年',
  range3Years: '3 年',
  range5Years: '5 年',
  range10Years: '10 年',
  loadError: '数据加载失败，请刷新页面或检查服务端状态',
  allHosts: '全部机器',
  defaultDirectory: '默认',
  tagFilter: '标签筛选 · 多选',
  allTags: '全部标签',
  noTags: '暂无标签',
  online: '在线',
  offline: '离线',
  loadingHosts: '正在加载主机数据…',
  noHosts: '当前条件下没有主机',
  directoryAllDescription: '当前目录内的全部主机；可从左侧附加标签筛选。',
  directoryTagDescription: '当前目录 AND（{tags}）',
  lastReportTime: '最后上报时间',
  uptime: '运行时间',
  uptimeDay: '{count} 天',
  uptimeDays: '{count} 天',
  load: '负载',
  noData: '无数据',
  noLoadData: '无负载数据',
  memory: '内存',
  disk: '硬盘',
  network: '网络',
  traffic: '流量',
  download: '下载',
  upload: '上传',
  process: '进程',
  thread: '线程',
  flagAlt: '{location} 国旗',
  osAlt: '{os} 操作系统',
  metricCpu: 'CPU',
  metricLoad1: '1 分钟负载',
  metricLoad5: '5 分钟负载',
  metricLoad15: '15 分钟负载',
  metricMemoryPercent: '内存使用率',
  metricSwapPercent: 'Swap 使用率',
  metricDiskPercent: '磁盘使用率',
  metricNetworkRx: '下载速率',
  metricNetworkTx: '上传速率',
  metricTrafficIn: '区间下载流量',
  metricTrafficOut: '区间上传流量',
  metricTcp: 'TCP 连接',
  metricUdp: 'UDP 连接',
  metricProcess: '进程数',
  metricThread: '线程数',
  metricUnicomLatency: '联通延迟',
  metricTelecomLatency: '电信延迟',
  metricMobileLatency: '移动延迟',
  metricUnicomLoss: '联通丢包',
  metricTelecomLoss: '电信丢包',
  metricMobileLoss: '移动丢包',
  carrierUnicom: '联通',
  carrierTelecom: '电信',
  carrierMobile: '移动',
  poweredBy: '技术支持',
  theme: '主题',
  by: '作者',
}

const zhTW: Record<MessageKey, string> = {
  appTitle: '伺服器監控',
  language: '語言',
  layoutMode: '版面配置',
  layoutGrid: '網格版面',
  layoutAdaptive: '自適應版面',
  layoutList: '列表版面',
  compact: '精簡',
  trendChart: '趨勢圖',
  trendMetric: '趨勢指標',
  trendRange: '趨勢範圍',
  range5Minutes: '5 分鐘',
  range15Minutes: '15 分鐘',
  range1Hour: '1 小時',
  range6Hours: '6 小時',
  range24Hours: '24 小時',
  range7Days: '7 天',
  range30Days: '30 天',
  range1Year: '1 年',
  range3Years: '3 年',
  range5Years: '5 年',
  range10Years: '10 年',
  loadError: '資料載入失敗，請重新整理頁面或檢查伺服器狀態',
  allHosts: '全部機器',
  defaultDirectory: '預設',
  tagFilter: '標籤篩選 · 多選',
  allTags: '全部標籤',
  noTags: '暫無標籤',
  online: '上線',
  offline: '離線',
  loadingHosts: '正在載入主機資料…',
  noHosts: '目前條件下沒有主機',
  directoryAllDescription: '目前目錄內的全部主機；可從左側附加標籤篩選。',
  directoryTagDescription: '目前目錄 AND（{tags}）',
  lastReportTime: '最後回報時間',
  uptime: '執行時間',
  uptimeDay: '{count} 天',
  uptimeDays: '{count} 天',
  load: '負載',
  noData: '無資料',
  noLoadData: '無負載資料',
  memory: '記憶體',
  disk: '硬碟',
  network: '網路',
  traffic: '流量',
  download: '下載',
  upload: '上傳',
  process: '程序',
  thread: '執行緒',
  flagAlt: '{location} 國旗',
  osAlt: '{os} 作業系統',
  metricCpu: 'CPU',
  metricLoad1: '1 分鐘負載',
  metricLoad5: '5 分鐘負載',
  metricLoad15: '15 分鐘負載',
  metricMemoryPercent: '記憶體使用率',
  metricSwapPercent: 'Swap 使用率',
  metricDiskPercent: '磁碟使用率',
  metricNetworkRx: '下載速率',
  metricNetworkTx: '上傳速率',
  metricTrafficIn: '區間下載流量',
  metricTrafficOut: '區間上傳流量',
  metricTcp: 'TCP 連線',
  metricUdp: 'UDP 連線',
  metricProcess: '程序數',
  metricThread: '執行緒數',
  metricUnicomLatency: '聯通延遲',
  metricTelecomLatency: '電信延遲',
  metricMobileLatency: '移動延遲',
  metricUnicomLoss: '聯通封包遺失',
  metricTelecomLoss: '電信封包遺失',
  metricMobileLoss: '移動封包遺失',
  carrierUnicom: '聯通',
  carrierTelecom: '電信',
  carrierMobile: '移動',
  poweredBy: '技術支援',
  theme: '主題',
  by: '作者',
}

const messages: Record<Locale, Record<MessageKey, string>> = {
  'zh-CN': zhCN,
  'zh-TW': zhTW,
  en,
}

export const languageOptions: Array<{ value: Locale; label: string }> = [
  { value: 'zh-CN', label: '简体中文' },
  { value: 'zh-TW', label: '正體中文' },
  { value: 'en', label: 'English' },
]

function detectBrowserLocale(): Locale {
  if (typeof navigator === 'undefined')
    return 'en'
  const language = navigator.languages?.[0] || navigator.language
  const normalized = language.toLowerCase()
  if (/^zh-(tw|hk|mo|hant)/.test(normalized))
    return 'zh-TW'
  if (normalized.startsWith('zh'))
    return 'zh-CN'
  return 'en'
}

function initialLocale(): Locale {
  if (typeof localStorage !== 'undefined') {
    const stored = localStorage.getItem('sstl-locale')
    if (stored === 'zh-CN' || stored === 'zh-TW' || stored === 'en')
      return stored
  }
  return detectBrowserLocale()
}

export const locale = ref<Locale>(initialLocale())

watch(locale, (value) => {
  if (typeof localStorage !== 'undefined')
    localStorage.setItem('sstl-locale', value)
  if (typeof document !== 'undefined') {
    document.documentElement.lang = value
    document.title = `${messages[value].appTitle} · ServerStatus History`
  }
})

if (typeof document !== 'undefined') {
  document.documentElement.lang = locale.value
  document.title = `${messages[locale.value].appTitle} · ServerStatus History`
}

export function translate(key: MessageKey, params: Record<string, string | number> = {}) {
  return Object.entries(params).reduce(
    (text, [name, value]) => text.replaceAll(`{${name}}`, String(value)),
    messages[locale.value][key],
  )
}

export function formatUptime(value: string) {
  const dayMatch = value.match(/^(\d+)\s*天$/)
  if (!dayMatch)
    return value
  return translate(dayMatch[1] === '1' ? 'uptimeDay' : 'uptimeDays', { count: dayMatch[1] })
}

export function useI18n() {
  return { locale, languageOptions, t: translate }
}
