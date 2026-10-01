import { themeMode } from '@/theme/appTheme'

export type AppChartTheme = {
  text: string
  muted: string
  grid: string
  axis: string
  bg: string
  border: string
  accent: string
  info: string
  warn: string
  input: string
  output: string
  cached: string
  error: string
}

/**
 * Issue #34 P2: the trend option is a tree-shaken ECharts option, so every
 * color comes from this palette. Each token keeps one meaning across modes
 * (`error` is always the error series, `cached` the cache meter) and the values
 * are contrast-checked against the mode surface by
 * `frontend/tests/request-overview-charts.test.ts`: text tokens clear 4.5:1,
 * axis lines and series colors 3:1, and the grid stays a deliberately subtle
 * dashed guide.
 */
const darkTheme: AppChartTheme = {
  text: '#d8fce2',
  muted: '#8bcf9f',
  grid: 'rgba(88, 232, 121, 0.28)',
  axis: 'rgba(88, 232, 121, 0.5)',
  bg: 'rgba(3, 8, 5, 0.95)',
  border: '#1c5d35',
  accent: '#58e879',
  info: '#1fbf9c',
  warn: '#facc15',
  input: '#58e879',
  output: '#1fbf9c',
  cached: '#3fb35f',
  error: '#f87171',
}

const lightTheme: AppChartTheme = {
  text: '#1f2937',
  muted: '#475569',
  grid: 'rgba(100, 116, 139, 0.45)',
  axis: 'rgba(71, 85, 105, 0.85)',
  bg: 'rgba(255, 255, 255, 0.96)',
  border: '#94a3b8',
  accent: '#2563eb',
  info: '#0e7490',
  warn: '#b45309',
  input: '#2563eb',
  output: '#0e7490',
  cached: '#15803d',
  error: '#b91c1c',
}

export function getChartTheme(): AppChartTheme {
  return themeMode.value === 'light' ? lightTheme : darkTheme
}
