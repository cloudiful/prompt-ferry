import type { RequestRecordFullResponse } from '@/generated/admin-api'
import { Badge } from '@/components/ui/badge'
import { useLocale } from '@/lib/i18n'
import {
  usageReasoningEffortDisplay,
  type UsageDetailView,
} from '@/lib/usage-detail-model'

type UsageContextSectionProps = {
  detail: UsageDetailView
  requestFull: RequestRecordFullResponse | null
}

/**
 * The AI request context: model, upstream identity, client key, conversation
 * identity, session header and the reasoning effort when it is known.
 */
export function UsageContextSection({
  detail,
  requestFull,
}: UsageContextSectionProps) {
  const { t } = useLocale()
  const reasoning = usageReasoningEffortDisplay(detail, requestFull)

  return (
    <section
      className="grid gap-2 rounded border p-3"
      data-usage-context-section
    >
      <h2 className="text-sm font-semibold">{t('requestContext')}</h2>
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        <Field label={t('model')}>{detail.model_display}</Field>
        <Field label={t('upstream')}>{detail.target}</Field>
        <Field label={t('upstreamId')}>
          <span className="break-all">{detail.endpoint_id || '-'}</span>
        </Field>
        <Field label={t('upstreamKey')}>
          {detail.endpoint_key_label || detail.endpoint_key_id ? (
            <span className="grid gap-0.5">
              <span>{detail.endpoint_key_label || '-'}</span>
              {detail.endpoint_key_id && (
                <span className="break-all text-muted-foreground">
                  {detail.endpoint_key_id}
                </span>
              )}
            </span>
          ) : (
            '-'
          )}
        </Field>
        <Field label={t('clientKey')}>{detail.client_key_label || '-'}</Field>
        <Field label={t('conversationId')}>
          <span className="break-all">{detail.conversation_id || '-'}</span>
        </Field>
        <Field label={t('conversationSeq')}>
          {detail.conversation_seq != null ? (
            <span className="flex flex-wrap gap-1">
              <Badge>#{detail.conversation_seq}</Badge>
              {detail.is_first_turn && (
                <Badge variant="outline">{t('firstTurn')}</Badge>
              )}
            </span>
          ) : (
            '-'
          )}
        </Field>
        <Field label={t('conversationSource')}>
          {detail.conversation_source_label}
        </Field>
        <Field label={t('sessionHeaderId')}>
          <span className="flex flex-wrap items-center gap-1">
            <span className="break-all">{detail.session_header_id || '-'}</span>
            {detail.is_sub_session && (
              <Badge variant="outline">{t('subSession')}</Badge>
            )}
          </span>
        </Field>
        {detail.session_parent_id && (
          <Field label={t('sessionParentId')}>
            <span className="break-all">{detail.session_parent_id}</span>
          </Field>
        )}
        {reasoning && <Field label={t('reasoningEffort')}>{reasoning}</Field>}
      </div>
    </section>
  )
}

function Field({
  children,
  label,
}: {
  children: React.ReactNode
  label: string
}) {
  return (
    <div className="grid content-start gap-px">
      <span className="text-[0.72rem] text-muted-foreground">{label}</span>
      <strong className="text-[0.8rem] leading-[1.45] font-semibold">
        {children}
      </strong>
    </div>
  )
}
