import type { RequestRecordDetailView } from '@/models'

export type JsonRecord = Record<string, unknown>

export type RequestRecordDetailWithAssistant = RequestRecordDetailView & {
  assistant_message_json?: unknown
  assistant_output_items_json?: unknown
  has_reasoning_content?: boolean | null
}

export function stringifyJson(value: unknown): string {
  return JSON.stringify(value, null, 2)
}
