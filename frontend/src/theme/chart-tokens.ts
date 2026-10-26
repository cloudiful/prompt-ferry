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
  categorical: string[]
}

export type ChartThemeMode = 'dark' | 'light'

export const DARK_CHART_THEME: AppChartTheme = {
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
  categorical: ['#60a5fa', '#c084fc', '#2dd4bf', '#fb923c', '#f472b6'],
}

export const LIGHT_CHART_THEME: AppChartTheme = {
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
  categorical: ['#1d4ed8', '#7c3aed', '#0f766e', '#c2410c', '#be185d'],
}

export function chartThemeFor(mode: ChartThemeMode): AppChartTheme {
  return mode === 'light' ? LIGHT_CHART_THEME : DARK_CHART_THEME
}
