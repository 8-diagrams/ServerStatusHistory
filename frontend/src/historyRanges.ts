import type { MessageKey } from '@/i18n'

export interface HistoryRange {
  seconds: number
  labelKey: MessageKey
}

export const HISTORY_RANGES: HistoryRange[] = [
  { seconds: 5 * 60, labelKey: 'range5Minutes' },
  { seconds: 15 * 60, labelKey: 'range15Minutes' },
  { seconds: 60 * 60, labelKey: 'range1Hour' },
  { seconds: 6 * 60 * 60, labelKey: 'range6Hours' },
  { seconds: 24 * 60 * 60, labelKey: 'range24Hours' },
  { seconds: 7 * 24 * 60 * 60, labelKey: 'range7Days' },
  { seconds: 30 * 24 * 60 * 60, labelKey: 'range30Days' },
  { seconds: 365 * 24 * 60 * 60, labelKey: 'range1Year' },
  { seconds: 3 * 365 * 24 * 60 * 60, labelKey: 'range3Years' },
  { seconds: 5 * 365 * 24 * 60 * 60, labelKey: 'range5Years' },
  { seconds: 10 * 365 * 24 * 60 * 60, labelKey: 'range10Years' },
]
