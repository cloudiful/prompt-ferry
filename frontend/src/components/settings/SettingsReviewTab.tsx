import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { LoaderCircle, Save, Sparkles } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { LlmReviewSettingsCard } from '@/components/settings/LlmReviewSettingsCard'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import {
  disposeSettingsReview,
  setSettingsReviewNotifier,
  useSettingsReviewStore,
} from '@/lib/settings-review'
import { LLM_REVIEW_KEY } from '@/lib/settings-review-model'
import { useAsyncAction } from '@/lib/use-async-action'

/**
 * The admin review settings: the LLM review configuration.
 *
 * The tab is saved by hand and never autosaves. Save is unavailable until the
 * settings have actually been read, because the stored key and webhook token are
 * echoed by that read and an empty default form would clear them. The one piece
 * of guidance the operator needs is the storage-mode hint, which stays visible
 * below the card rather than inside a collapsed group.
 */
export function SettingsReviewTab() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError, notifyError, notifySuccess } = useNotifier()
  const { isPending, pendingOrigin } = useAsyncAction()

  const form = useSettingsReviewStore((state) => state.form)
  const loaded = useSettingsReviewStore((state) => state.loaded)
  const loading = useSettingsReviewStore((state) => state.loading)
  const loadError = useSettingsReviewStore((state) => state.loadError)
  const refresh = useSettingsReviewStore((state) => state.refresh)
  const setField = useSettingsReviewStore((state) => state.setField)
  const save = useSettingsReviewStore((state) => state.save)

  const savedMessage = useRef<() => void>(() => {})
  savedMessage.current = () => notifySuccess(t('llmReviewSaved'))

  useEffect(() => {
    setSettingsReviewNotifier({
      notifyApiError: (cause) => notifyApiError(cause),
      notifyHeaderFormat: () => notifyError(t('invalidReviewHeadersFormat')),
      notifySaved: () => savedMessage.current(),
    })
    return () => {
      setSettingsReviewNotifier(null)
      disposeSettingsReview()
    }
  }, [notifyApiError, notifyError, t])

  const savePending = isPending(LLM_REVIEW_KEY)
  const saveOrigin = pendingOrigin(LLM_REVIEW_KEY)
  const disabled = !loaded || savePending

  if (loadError && !loaded) {
    return (
      <div className="grid min-w-0 gap-2 rounded-lg border bg-card px-3 py-2.5">
        <p className="m-0 text-sm" role="alert">
          {t('error')}
        </p>
        <div>
          <Button
            onClick={() => void refresh(queryClient)}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('retry')}
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="grid min-w-0 gap-2">
      <SettingsCard icon={Sparkles} title={t('llmReview')}>
        <div className="grid min-w-0 gap-3">
          <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <Switch
                aria-label={t('llmReview')}
                checked={form.enabled}
                disabled={disabled}
                id="settings-llm-review-enabled"
                name="settings-llm-review-enabled"
                onCheckedChange={(checked) =>
                  setField('enabled', checked === true)
                }
              />
              <Label
                className="text-xs font-medium"
                htmlFor="settings-llm-review-enabled"
              >
                {t('llmReview')}
              </Label>
            </div>
            <div className="flex items-center gap-2">
              {loading && !loaded ? (
                <span
                  aria-label={t('loading')}
                  className="inline-flex"
                  role="status"
                >
                  <LoaderCircle
                    aria-hidden="true"
                    className="size-3.5 animate-spin text-muted-foreground"
                  />
                </span>
              ) : null}
              {saveOrigin === 'settings' ? (
                <span
                  aria-label={t('loading')}
                  className="inline-flex"
                  role="status"
                >
                  <LoaderCircle
                    aria-hidden="true"
                    className="size-3.5 animate-spin text-muted-foreground"
                  />
                </span>
              ) : null}
              <Button
                disabled={disabled}
                id="settings-llm-review-save"
                onClick={() => void save()}
                size="sm"
                type="button"
              >
                <Save aria-hidden="true" />
                {t('save')}
              </Button>
            </div>
          </div>
          <LlmReviewSettingsCard
            disabled={disabled}
            form={form}
            onChange={setField}
          />
        </div>
      </SettingsCard>
      <p className="m-0 px-1 text-[11px] leading-relaxed text-muted-foreground">
        {t('llmReviewHint')}
      </p>
    </div>
  )
}
