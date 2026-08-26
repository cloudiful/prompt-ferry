import type {
  RequestRecordFormatting,
  RequestRecordTiming,
} from '../models/request-record-formatting'
import {
  requestRecordStateTagSeverity,
  requestRecordStateLabelKey,
} from '../request-records'

export function formatBytes(value?: number | null): string {
  if (value == null) return '-'
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`
  return `${(value / (1024 * 1024)).toFixed(2)} MiB`
}

export function formatCompressionRatio(value?: number | null): string {
  if (value == null) return '-'
  return `${value.toFixed(2)}x`
}

export function formatRequestRecordTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(new Date(value))
}

export function formatRequestRecordCount(value?: number | null): string {
  if (value == null) return '-'
  return new Intl.NumberFormat().format(value)
}

export function formatTokenQuantity(value?: number | null): string {
  if (value == null || !Number.isFinite(value)) return '-'
  const abs = Math.abs(value)
  if (abs < 1000) {
    return new Intl.NumberFormat(undefined).format(Math.round(value))
  }
  let scaled = value
  let unit = ''
  if (abs >= 1_000_000_000_000) {
    scaled = value / 1_000_000_000_000
    unit = 'T'
  } else if (abs >= 1_000_000_000) {
    scaled = value / 1_000_000_000
    unit = 'B'
  } else if (abs >= 1_000_000) {
    scaled = value / 1_000_000
    unit = 'M'
  } else {
    scaled = value / 1_000
    unit = 'K'
  }
  const rounded = Math.round(scaled * 10) / 10
  const fixed = rounded.toFixed(1)
  const trimmed = fixed.endsWith('.0') ? fixed.slice(0, -2) : fixed
  const localized = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 1,
  }).format(Number(trimmed))
  return `${localized}${unit}`
}

export function formatTokensPerSecondValue(value?: number | null): string {
  if (value == null || !Number.isFinite(value) || value <= 0) return '-'
  const rounded = value >= 100 ? value.toFixed(0) : value.toFixed(1)
  return `${rounded} token/s`
}

export function formatRequestRecordPercent(value?: number | null): string {
  if (value == null) return '-'
  return `${Math.round(value * 100)}%`
}

export function formatRequestRecordMs(value?: number | null): string {
  if (value == null) return '-'
  const ms = Math.max(0, value)
  if (ms < 1000) return `${Math.round(ms)}ms`
  if (ms < 60_000) {
    const seconds = ms / 1000
    return `${seconds >= 10 ? seconds.toFixed(0) : seconds.toFixed(1)}s`
  }
  const totalSeconds = Math.round(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return seconds === 0 ? `${minutes}m` : `${minutes}m ${seconds}s`
}

export function requestRecordStreamMs(
  record: RequestRecordTiming,
): number | null {
  if (record.duration_ms == null || record.ttft_ms == null) return null
  return Math.max(0, record.duration_ms - record.ttft_ms)
}

function formatRate(
  tokens?: number | null,
  durationMs?: number | null,
): string {
  if (tokens == null || durationMs == null || durationMs <= 0) return '-'
  const tokensPerSecond = tokens / (durationMs / 1000)
  if (!Number.isFinite(tokensPerSecond) || tokensPerSecond <= 0) return '-'
  return `${tokensPerSecond >= 100 ? tokensPerSecond.toFixed(0) : tokensPerSecond.toFixed(1)}`
}

export function formatRequestRecordOutputTokensPerSecond(
  record: RequestRecordTiming,
): string {
  if (
    record.request_category !== 'ai' ||
    record.request_state !== 'completed'
  ) {
    return '-'
  }
  return formatRate(record.output_tokens, record.duration_ms)
}

export function hasOutputRate(record: RequestRecordTiming): boolean {
  return (
    record.request_category === 'ai' &&
    record.request_state === 'completed' &&
    record.output_tokens != null &&
    record.output_tokens > 0 &&
    record.duration_ms != null &&
    record.duration_ms > 0
  )
}

export function formatRequestRecordInputTokensPerSecond(
  record: RequestRecordTiming,
): string {
  return formatRate(record.input_tokens, record.ttft_ms)
}

export function createRequestRecordFormatting(
  labels: TranslateFn,
): RequestRecordFormatting {
  return {
    formatCount: formatRequestRecordCount,
    formatTokenQuantity,
    formatMs: formatRequestRecordMs,
    formatPercent: formatRequestRecordPercent,
    formatOutputTokensPerSecond: formatRequestRecordOutputTokensPerSecond,
    formatTokensPerSecond: formatTokensPerSecondValue,
    hasOutputRate,
    formatInputTokensPerSecond: formatRequestRecordInputTokensPerSecond,
    formatRequestStateLabel: (state) =>
      labels(requestRecordStateLabelKey(state)),
    formatTime: formatRequestRecordTime,
    requestStateSeverity: requestRecordStateTagSeverity,
    streamMs: requestRecordStreamMs,
  }
}
