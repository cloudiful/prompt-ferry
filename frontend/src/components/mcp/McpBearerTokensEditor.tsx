import { Plus, Trash2 } from 'lucide-react'
import type { McpBearerTokenForm } from '@/models'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import { canDisableMcpBearerToken, canRemoveMcpBearerToken } from '@/models/mcp'

type McpBearerTokensEditorProps = {
  disabled?: boolean
  onChange: (tokens: McpBearerTokenForm[]) => void
  tokens: McpBearerTokenForm[]
}

export function McpBearerTokensEditor({
  disabled = false,
  onChange,
  tokens,
}: McpBearerTokensEditorProps) {
  const { t } = useLocale()

  function update(index: number, patch: Partial<McpBearerTokenForm>): void {
    onChange(
      tokens.map((token, row) =>
        row === index ? { ...token, ...patch } : token,
      ),
    )
  }

  function add(): void {
    onChange([...tokens, { token: '', enabled: true }])
  }

  function remove(index: number): void {
    if (!canRemoveMcpBearerToken(tokens, index)) return
    onChange(tokens.filter((_, row) => row !== index))
  }

  return (
    <section aria-label={t('bearerToken')} className="grid min-w-0 gap-2">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <h3 className="text-xs font-medium">{t('bearerToken')}</h3>
          <span className="text-xs text-muted-foreground">
            {t('bearerTokenHint')}
          </span>
        </div>
        <Button
          disabled={disabled}
          onClick={add}
          size="sm"
          type="button"
          variant="outline"
        >
          <Plus aria-hidden="true" />
          {t('addBearerToken')}
        </Button>
      </div>
      <div className="grid min-w-0 gap-2">
        {tokens.map((token, index) => (
          <div
            className="grid min-w-0 gap-2 rounded-md border p-2 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-center"
            key={`bearer-${index}`}
          >
            <Input
              aria-label={`${t('bearerToken')} ${index + 1}`}
              autoComplete="off"
              disabled={disabled}
              onChange={(event) =>
                update(index, { token: event.currentTarget.value })
              }
              placeholder={t('bearerTokenPlaceholder')}
              type="password"
              value={token.token}
            />
            <Switch
              aria-label={`${t('status')} ${t('bearerToken')} ${index + 1}`}
              checked={token.enabled}
              disabled={disabled || !canDisableMcpBearerToken(tokens, index)}
              onCheckedChange={(enabled) => update(index, { enabled })}
            />
            <Button
              aria-label={`${t('delete')} ${t('bearerToken')} ${index + 1}`}
              disabled={disabled || !canRemoveMcpBearerToken(tokens, index)}
              onClick={() => remove(index)}
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              <Trash2 aria-hidden="true" />
            </Button>
          </div>
        ))}
      </div>
    </section>
  )
}
