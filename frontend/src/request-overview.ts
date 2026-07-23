export type RequestOverviewMetricCard = {
  key: string
  label: string
  value: number
  unit: string
}

export type RequestOverviewTrendBucket = {
  bucket_at: string
  request_count: number
  success_rate: number
  quality_score: number
  p95_total_ms?: number | null
  p95_first_token_ms?: number | null
}

export type RequestOverviewRankingRow = {
  label: string
  secondary_label?: string | null
  request_count: number
  success_rate: number
  quality_score: number
  p95_total_ms?: number | null
  p95_first_token_ms?: number | null
  rate_limit_rate?: number | null
  auth_error_rate?: number | null
  upstream_5xx_rate?: number | null
  empty_success_rate?: number | null
  cache_hit_rate?: number | null
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
}

export type RequestOverviewRankingGroup = {
  key: string
  title: string
  rows: RequestOverviewRankingRow[]
}

export type RequestOverviewHeatmapCell = {
  x_index: number
  y_index: number
  request_count: number
  success_rate: number
  quality_score: number
  p95_total_ms?: number | null
  p95_first_token_ms?: number | null
  error_rate: number
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
}

export type RequestOverviewHeatmap = {
  key: string
  x_labels: string[]
  y_labels: string[]
  cells: RequestOverviewHeatmapCell[]
}

export type RequestOverviewErrorBreakdownRow = {
  key: string
  label: string
  count: number
  rate: number
}

export type RequestOverviewQualityComponent = {
  key: string
  label: string
  weight: number
  description: string
}

export type RequestOverviewQualityFormula = {
  score_kind: string
  components: RequestOverviewQualityComponent[]
}

export type RequestOverviewResponse = {
  summary_cards: RequestOverviewMetricCard[]
  trend: RequestOverviewTrendBucket[]
  quality_formula: RequestOverviewQualityFormula
  top_rankings: RequestOverviewRankingGroup[]
  heatmap: RequestOverviewHeatmap
  error_breakdown: RequestOverviewErrorBreakdownRow[]
}

export type RequestOverviewMode = 'overview' | 'records'

export type RequestOverviewDrilldown = {
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
}
