import type { RequestRecordDetailView } from '@/models'

export type RequestRecordDetailWithAssistant = RequestRecordDetailView & {
  assistant_message_json?: unknown
  assistant_output_items_json?: unknown
  has_reasoning_content?: boolean | null
}
