import { Plus, Trash2 } from 'lucide-react'
import { duplicateEndpointApiKeyLabelIndexes } from '@/admin-mappers/forms/endpoint'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import type { EndpointApiKeyForm, EndpointForm } from '@/models'

type EndpointApiKeysEditorProps = {
  form: EndpointForm
  onChange: (update: (current: EndpointForm) => EndpointForm) => void
}

function newApiKey(): EndpointApiKeyForm {
  return {
    key_label: '',
    api_key: '',
    has_saved_key: false,
    enabled: true,
    key_id: '',
  }
}

export function EndpointApiKeysEditor({
  form,
  onChange,
}: EndpointApiKeysEditorProps) {
  const { t } = useLocale()
  const duplicateIndexes = new Set(
    duplicateEndpointApiKeyLabelIndexes(form.api_keys),
  )

  function updateKey(index: number, patch: Partial<EndpointApiKeyForm>): void {
    onChange((current) => ({
      ...current,
      api_keys: current.api_keys.map((key, keyIndex) =>
        keyIndex === index ? { ...key, ...patch } : key,
      ),
    }))
  }

  function addKey(): void {
    onChange((current) => ({
      ...current,
      api_keys: [...current.api_keys, newApiKey()],
    }))
  }

  function canDelete(index: number): boolean {
    return (
      form.api_keys.length > 1 &&
      form.api_keys.some((key, keyIndex) => keyIndex !== index && key.enabled)
    )
  }

  function canDisable(index: number): boolean {
    const key = form.api_keys[index]
    return (
      !key?.enabled ||
      form.api_keys.some(
        (other, otherIndex) => otherIndex !== index && other.enabled,
      )
    )
  }

  function removeKey(index: number): void {
    if (!canDelete(index)) return
    onChange((current) => ({
      ...current,
      api_keys: current.api_keys.filter((_, keyIndex) => keyIndex !== index),
    }))
  }

  return (
    <section aria-label={t('apiKeys')} className="grid min-w-0 gap-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="font-medium">{t('apiKeys')}</div>
          <p className="text-xs text-muted-foreground">
            {t('endpointApiKeysHint')}
          </p>
        </div>
        <label className="inline-flex min-h-8 items-center gap-2 text-xs">
          <span>{t('endpointKeyLbEnabled')}</span>
          <Switch
            aria-label={t('endpointKeyLbEnabled')}
            checked={form.key_lb_enabled}
            onCheckedChange={(checked) =>
              onChange((current) => ({ ...current, key_lb_enabled: checked }))
            }
          />
        </label>
      </div>
      <div className="grid gap-3">
        {form.api_keys.map((key, index) => (
          <div
            className="grid min-w-0 gap-2 rounded-md border p-3 lg:grid-cols-[minmax(8rem,0.8fr)_minmax(12rem,1.5fr)_auto_auto] lg:items-start"
            data-endpoint-key-row={index}
            key={key.key_id || `new-${index}`}
          >
            <div className="grid min-w-0 gap-1">
              <label
                className="text-xs font-medium"
                htmlFor={`endpoint-key-name-${index}`}
              >
                {t('apiKeyName')}
              </label>
              <Input
                aria-label={`${t('apiKeyName')} ${index + 1}`}
                autoComplete="off"
                id={`endpoint-key-name-${index}`}
                onChange={(event) =>
                  updateKey(index, { key_label: event.currentTarget.value })
                }
                placeholder={t('apiKeyNameDefault', { index: index + 1 })}
                value={key.key_label}
              />
              {duplicateIndexes.has(index) ? (
                <p className="text-xs text-destructive">
                  {t('apiKeyNameDuplicate')}
                </p>
              ) : null}
            </div>
            <div className="grid min-w-0 gap-1">
              <label
                className="text-xs font-medium"
                htmlFor={`endpoint-api-key-${index}`}
              >
                {t('apiKey')}
              </label>
              <div className="flex min-w-0 items-center gap-2">
                {key.has_saved_key ? (
                  <span className="shrink-0 rounded bg-muted px-2 py-1 text-xs text-muted-foreground">
                    {t('saved')}
                  </span>
                ) : null}
                <Input
                  aria-label={`${t('apiKey')} ${index + 1}`}
                  autoComplete="new-password"
                  id={`endpoint-api-key-${index}`}
                  onChange={(event) =>
                    updateKey(index, { api_key: event.currentTarget.value })
                  }
                  placeholder={
                    key.has_saved_key ? t('apiKeyOptionalOnEdit') : t('apiKey')
                  }
                  type="password"
                  value={key.api_key}
                />
              </div>
            </div>
            <label className="inline-flex min-h-9 items-center justify-center gap-2 self-end text-xs">
              <span>{t('status')}</span>
              <Switch
                aria-label={`${t('status')} ${key.key_label.trim() || t('apiKeyNameDefault', { index: index + 1 })}`}
                checked={key.enabled}
                disabled={!canDisable(index)}
                onCheckedChange={(checked) =>
                  updateKey(index, { enabled: checked })
                }
              />
            </label>
            <Button
              aria-label={`${t('delete')} ${key.key_label.trim() || t('apiKeyNameDefault', { index: index + 1 })}`}
              className="justify-self-end self-end"
              disabled={!canDelete(index)}
              onClick={() => removeKey(index)}
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              <Trash2 aria-hidden="true" />
            </Button>
          </div>
        ))}
      </div>
      <Button
        className="justify-self-start"
        onClick={addKey}
        size="sm"
        type="button"
        variant="outline"
      >
        <Plus aria-hidden="true" />
        {t('addApiKey')}
      </Button>
    </section>
  )
}
