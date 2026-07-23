import { computed } from 'vue'
import { themeMode } from '@/theme/appTheme'

export type AppChartTheme = {
  text: string
  muted: string
  subtle: string
  grid: string
  axis: string
  bg: string
  border: string
  accent: string
  info: string
  warn: string
  heatLow: string
  heatHigh: string
  input: string
  output: string
  cached: string
  error: string
  labelStrong: string
  emphasisBorder: string
}

const darkTheme: AppChartTheme = {
  text: '#d8fce2',
  muted: '#8bcf9f',
  subtle: 'rgba(127, 182, 144, 0.38)',
  grid: 'rgba(46, 113, 68, 0.24)',
  axis: 'rgba(88, 232, 121, 0.34)',
  bg: 'rgba(3, 8, 5, 0.95)',
  border: '#1c5d35',
  accent: '#58e879',
  info: '#1fbf9c',
  warn: '#facc15',
  heatLow: '#123c24',
  heatHigh: '#58e879',
  input: '#58e879',
  output: '#1fbf9c',
  cached: '#2b8f4d',
  error: '#facc15',
  labelStrong: '#041006',
  emphasisBorder: '#ffffff',
}

const lightTheme: AppChartTheme = {
  text: '#1f2937',
  muted: '#64748b',
  subtle: 'rgba(100, 116, 139, 0.48)',
  grid: 'rgba(148, 163, 184, 0.28)',
  axis: 'rgba(100, 116, 139, 0.38)',
  bg: 'rgba(255, 255, 255, 0.96)',
  border: '#cbd5e1',
  accent: '#2563eb',
  info: '#0891b2',
  warn: '#d97706',
  heatLow: '#dbeafe',
  heatHigh: '#2563eb',
  input: '#2563eb',
  output: '#0891b2',
  cached: '#16a34a',
  error: '#d97706',
  labelStrong: '#eff6ff',
  emphasisBorder: '#0f172a',
}

export function getChartTheme(): AppChartTheme {
  return themeMode.value === 'light' ? lightTheme : darkTheme
}

export function useChartTheme() {
  return computed(() => getChartTheme())
}
