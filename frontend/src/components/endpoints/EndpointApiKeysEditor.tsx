import { Plus, Trash2 } from 'lucide-react'
import { duplicateEndpointApiKeyLabelIndexes } from '@/admin-mappers/forms/endpoint'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import type { EndpointApiKeyForm } from '@/models'
import type { EndpointFormApi } from '@/models/endpoints/form-api'

type EndpointApiKeysEditorProps = {
  form: EndpointFormApi
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

export function EndpointApiKeysEditor({ form }: EndpointApiKeysEditorProps) {
  const { t } = useLocale()

  return (
    <section aria-label={t('apiKeys')} className="grid min-w-0 gap-2">
      <form.Field name="api_keys" mode="array">
        {(keysField) => {
          const keys = keysField.state.value
          const duplicateIndexes = new Set(
            duplicateEndpointApiKeyLabelIndexes(keys),
          )

          function canDelete(index: number): boolean {
            return (
              keys.length > 1 &&
              keys.some((key, keyIndex) => keyIndex !== index && key.enabled)
            )
          }

          function canDisable(index: number): boolean {
            const key = keys[index]
            return (
              !key?.enabled ||
              keys.some(
                (other, otherIndex) => otherIndex !== index && other.enabled,
              )
            )
          }

          return (
            <>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="min-w-0">
                  <div className="font-medium">{t('apiKeys')}</div>
                  <p className="text-xs text-muted-foreground">
                    {t('endpointApiKeysHint')}
                  </p>
                </div>
                <label className="inline-flex min-h-8 items-center gap-2 text-xs">
                  <span>{t('endpointKeyLbEnabled')}</span>
                  <form.Field name="key_lb_enabled">
                    {(field) => (
                      <Switch
                        aria-label={t('endpointKeyLbEnabled')}
                        checked={field.state.value}
                        onCheckedChange={(checked) =>
                          field.handleChange(checked === true)
                        }
                      />
                    )}
                  </form.Field>
                </label>
              </div>
              <div className="grid gap-3">
                {keys.map((key, index) => (
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
                      <form.Field name={`api_keys[${index}].key_label`}>
                        {(field) => (
                          <Input
                            aria-label={`${t('apiKeyName')} ${index + 1}`}
                            autoComplete="off"
                            id={`endpoint-key-name-${index}`}
                            onChange={(event) =>
                              field.handleChange(event.currentTarget.value)
                            }
                            placeholder={t('apiKeyNameDefault', {
                              index: index + 1,
                            })}
                            value={field.state.value}
                          />
                        )}
                      </form.Field>
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
                        <form.Field name={`api_keys[${index}].api_key`}>
                          {(field) => (
                            <Input
                              aria-label={`${t('apiKey')} ${index + 1}`}
                              autoComplete="new-password"
                              id={`endpoint-api-key-${index}`}
                              onChange={(event) =>
                                field.handleChange(event.currentTarget.value)
                              }
                              placeholder={
                                key.has_saved_key
                                  ? t('apiKeyOptionalOnEdit')
                                  : t('apiKey')
                              }
                              type="password"
                              value={field.state.value}
                            />
                          )}
                        </form.Field>
                      </div>
                    </div>
                    <label className="inline-flex min-h-9 items-center justify-center gap-2 self-end text-xs">
                      <span>{t('status')}</span>
                      <form.Field name={`api_keys[${index}].enabled`}>
                        {(field) => (
                          <Switch
                            aria-label={`${t('status')} ${key.key_label.trim() || t('apiKeyNameDefault', { index: index + 1 })}`}
                            checked={field.state.value}
                            disabled={!canDisable(index)}
                            onCheckedChange={(checked) =>
                              field.handleChange(checked === true)
                            }
                          />
                        )}
                      </form.Field>
                    </label>
                    <Button
                      aria-label={`${t('delete')} ${key.key_label.trim() || t('apiKeyNameDefault', { index: index + 1 })}`}
                      className="justify-self-end self-end"
                      disabled={!canDelete(index)}
                      onClick={() => keysField.removeValue(index)}
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
                onClick={() => keysField.pushValue(newApiKey())}
                size="sm"
                type="button"
                variant="outline"
              >
                <Plus aria-hidden="true" />
                {t('addApiKey')}
              </Button>
            </>
          )
        }}
      </form.Field>
    </section>
  )
}
