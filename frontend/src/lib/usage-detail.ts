export const USAGE_REQUEST_FULL_PAGE_SIZE = 10
export const USAGE_REQUEST_FULL_ORDER = 'desc'
export const USAGE_MAINTENANCE_KEY = 'usage:maintenance'

export function usageConversationActionKey(conversationId: string): string {
  return `conversation:${conversationId}`
}

export function usageRecordActionKey(recordId: number): string {
  return `record:${recordId}`
}

export function usageDetailActionKey(
  conversationId: string | null,
  recordId: number,
): string {
  return conversationId
    ? usageConversationActionKey(conversationId)
    : usageRecordActionKey(recordId)
}
