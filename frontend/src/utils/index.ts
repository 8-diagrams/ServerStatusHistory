import type { ServerData } from '@/types'

export function formatBytes(bytes: number, decimals = 2) {
  if (bytes === 0)
    return '0 B'
  const k = 1024
  const dm = decimals < 0 ? 0 : decimals
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${Number.parseFloat((bytes / k ** i).toFixed(dm))} ${sizes[i]}`
}

export function formatLoad(value: number) {
  return Number.isFinite(value) ? value.toFixed(2) : '0.00'
}

export function formatTime(seconds: number, locale?: string) {
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(new Date(seconds * 1000))
}

export function isOnline(server: ServerData) {
  return server.online4 || server.online6
}

export function isCountryFlagEmoji(text: string) {
  const regex = /[\uD800-\uDBFF][\uDC00-\uDFFF]/g
  return regex.test(text)
}

export function hasLoadData(server: ServerData) {
  return server.load === undefined
    && server.load_1 === undefined
    && server.load_5 === undefined
    && server.load_15 === undefined
}

export function parseLabels(labelsString: string | undefined) {
  if (labelsString === undefined)
    return {}
  const list = labelsString.split(';')
  const result: { [key: string]: string } = {}
  list.forEach((item) => {
    if (item === '')
      return
    const [key, value] = item.split('=')
    result[key] = value
  })
  return result
}

export function osIconName(os: string) {
  return ({ windows: 'window', arch: 'archlinux' } as Record<string, string>)[os] || os
}
