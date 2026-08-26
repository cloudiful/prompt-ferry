import type {
  ClientKeyFacet,
  RequestRecordFacets,
  RequestRecordFullResponse,
  RequestRecordState,
} from '../generated/admin-api'
import type {
  ConversationEndpointOverrideView,
  Option,
  RequestRecordDetailView,
  RequestRecordRowView,
  SessionRouteOptionsView,
} from '../models'

export type UsageFacetOptionsView = {
  request_user_options: Option[]
  request_model_options: Option[]
  request_client_key_options: Option<number>[]
  request_state_options: Option<RequestRecordState>[]
  request_redaction_options: Option<boolean>[]
}

export type UsageDetailWorkspaceView = {
  record: RequestRecordDetailView | null
  detail_loading: boolean
  request_full: RequestRecordFullResponse | null
  request_full_loading: boolean
  session_route_options: SessionRouteOptionsView | null
  session_route_options_loading: boolean
  conversation_override: ConversationEndpointOverrideView | null
  override_saving: boolean
  affinity_resetting: boolean
}

export type UsageWorkspaceView = {
  busy: boolean
  records_loading: boolean
  records: RequestRecordRowView[]
  first: number
  rows_per_page: number
  total: number
  sort_field: string
  sort_order: -1 | 0 | 1
  facets: UsageFacetOptionsView
  detail: UsageDetailWorkspaceView
}

type UsageStateLabels = Record<RequestRecordState, string>
type UsageWorkspaceRecordsInput = {
  first: number
  items: RequestRecordRowView[]
  loading: boolean
  rowsPerPage: number
  sortField: string
  sortOrder: -1 | 0 | 1
  total: number
}
type UsageWorkspaceDetailInput = {
  affinityResetting: boolean
  conversationOverride: ConversationEndpointOverrideView | null
  detailLoading: boolean
  detailRecord: RequestRecordDetailView | null
  overrideSaving: boolean
  requestFull: RequestRecordFullResponse | null
  requestFullLoading: boolean
  routeOptionsLoading: boolean
  sessionRouteOptions: SessionRouteOptionsView | null
}

export function createUsageStateOptions(
  labels: UsageStateLabels,
): Option<RequestRecordState>[] {
  return [
    { label: labels.received, value: 'received' },
    { label: labels.awaiting_approval, value: 'awaiting_approval' },
    { label: labels.upstream_processing, value: 'upstream_processing' },
    { label: labels.completed, value: 'completed' },
    { label: labels.failed, value: 'failed' },
    { label: labels.aborted, value: 'aborted' },
  ]
}

export function createUsageFacetOptionsView(
  facets: RequestRecordFacets,
  requestStateOptions: Option<RequestRecordState>[],
): UsageFacetOptionsView {
  return {
    request_user_options: facets.users.map((value) => ({
      label: value,
      value,
    })),
    request_model_options: facets.models.map((value) => ({
      label: value,
      value,
    })),
    request_client_key_options: createUsageClientKeyOptions(
      facets.client_keys ?? [],
    ),
    request_state_options: requestStateOptions,
    request_redaction_options: [
      { label: 'Yes', value: true },
      { label: 'No', value: false },
    ],
  }
}

function createUsageClientKeyOptions(
  facets: ClientKeyFacet[],
): Option<number>[] {
  return facets.map((facet) => ({
    label: facet.user_login_name
      ? `${facet.label} (${facet.user_login_name})`
      : facet.label,
    value: facet.key_id,
  }))
}

export function createUsageWorkspaceView(options: {
  busy: boolean
  facets: UsageFacetOptionsView
  detail: UsageWorkspaceDetailInput
  records: UsageWorkspaceRecordsInput
}): UsageWorkspaceView {
  return {
    busy: options.busy,
    records_loading: options.records.loading,
    records: options.records.items,
    first: options.records.first,
    rows_per_page: options.records.rowsPerPage,
    total: options.records.total,
    sort_field: options.records.sortField,
    sort_order: options.records.sortOrder,
    facets: options.facets,
    detail: {
      affinity_resetting: options.detail.affinityResetting,
      record: options.detail.detailRecord,
      detail_loading: options.detail.detailLoading,
      request_full: options.detail.requestFull,
      request_full_loading: options.detail.requestFullLoading,
      session_route_options: options.detail.sessionRouteOptions,
      session_route_options_loading: options.detail.routeOptionsLoading,
      conversation_override: options.detail.conversationOverride,
      override_saving: options.detail.overrideSaving,
    },
  }
}
