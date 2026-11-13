import { LoaderCircle, RefreshCw, Save, Sparkles } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { LlmReviewSettingsCard } from '@/components/settings/LlmReviewSettingsCard'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSettingsReview } from '@/lib/settings-review'

/**
 * The admin review settings: the LLM review configuration.
 *
 * The tab is saved by hand and never autosaves. Save is unavailable until the
 * settings have actually been read, because the stored key and webhook token are
 * echoed by that read and an empty default form would clear them.
 */
export function SettingsReviewTab() {
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const settings = useSettingsReview({
    onSaved: () => notifySuccess(t('llmReviewSaved')),
  })
  const { form, values } = settings

  const savePending = settings.pending
  const saveOrigin = settings.pendingOrigin
  const disabled = !settings.loaded || savePending

  const refresh = () => {
    void settings.refresh().catch(notifyApiError)
  }

  if (settings.loadError && !settings.loaded) {
    return (
      <>
        <HeaderActions>
          <Button
            aria-label={t('refresh')}
            disabled={settings.refreshBlocked}
            id="settings-review-refresh"
            onClick={refresh}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </HeaderActions>
        <div className="grid min-w-0 gap-2 rounded-lg border bg-card px-3 py-2.5">
          <p className="m-0 text-sm" role="alert">
            {t('error')}
          </p>
          <div>
            <Button onClick={refresh} size="sm" type="button" variant="outline">
              {t('retry')}
            </Button>
          </div>
        </div>
      </>
    )
  }

  return (
    <>
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          disabled={settings.refreshBlocked}
          id="settings-review-refresh"
          onClick={refresh}
          size="sm"
          type="button"
          variant="outline"
        >
          <RefreshCw aria-hidden="true" />
          {t('refresh')}
        </Button>
      </HeaderActions>
      {settings.loadError ? (
        <p
          className="m-0 rounded-lg border bg-card px-3 py-2.5 text-sm"
          role="alert"
        >
          {t('error')}
        </p>
      ) : null}
      <div className="grid min-w-0 gap-2">
        <SettingsCard icon={Sparkles} title={t('llmReview')}>
          <form
            className="grid min-w-0 gap-3"
            noValidate
            onSubmit={(event) => {
              event.preventDefault()
              event.stopPropagation()
              if (disabled) return
              void form.handleSubmit().catch(notifyApiError)
            }}
          >
            <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
              <div className="flex min-w-0 items-center gap-2">
                <Switch
                  aria-label={t('llmReview')}
                  checked={values.enabled}
                  disabled={disabled}
                  id="settings-llm-review-enabled"
                  name="settings-llm-review-enabled"
                  onCheckedChange={(checked) =>
                    form.setFieldValue('enabled', checked === true)
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
                {settings.loading && !settings.loaded ? (
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
                  size="sm"
                  type="submit"
                >
                  <Save aria-hidden="true" />
                  {t('save')}
                </Button>
              </div>
            </div>
            <LlmReviewSettingsCard
              disabled={disabled}
              form={values}
              onChange={(field, value) =>
                form.setFieldValue(field as never, value as never)
              }
            />
          </form>
        </SettingsCard>
        <p className="m-0 px-1 text-[11px] leading-relaxed text-muted-foreground">
          {t('llmReviewHint')}
        </p>
      </div>
    </>
  )
}
