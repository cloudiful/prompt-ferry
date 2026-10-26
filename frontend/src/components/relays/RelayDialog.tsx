import type {
  BridgeEncryptionMode,
  ManagedRelay,
  TlsMode,
} from '@/generated/admin-api'
import type { RelayForm, RelaySecretAction } from '@/admin-mappers/forms/relay'
import type { MessageKey } from '@/i18n'
import { formatDateTime } from '@/format/datetime-core'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'

const TLS_OPTIONS: Array<{ label: MessageKey; value: TlsMode }> = [
  { label: 'relayTlsOff', value: 'off' },
  { label: 'relayTlsServer', value: 'server' },
  { label: 'relayTlsMtls', value: 'mtls' },
]

const BRIDGE_OPTIONS: Array<{
  label: MessageKey
  value: BridgeEncryptionMode
}> = [
  { label: 'relayBridgeOff', value: 'off' },
  { label: 'relayBridgeRequired', value: 'required' },
]

const SECRET_ACTION_OPTIONS: Array<{
  label: MessageKey
  value: RelaySecretAction
}> = [
  { label: 'relaySecretKeep', value: 'keep' },
  { label: 'relaySecretReplace', value: 'replace' },
  { label: 'relaySecretClear', value: 'clear' },
]

type RelaySecretFieldProps = {
  action: RelaySecretAction
  actionField: keyof RelayForm
  disabled: boolean
  field: keyof RelayForm
  label: string
  onChange: (patch: Partial<RelayForm>) => void
  relayId: string
  rows: number
  value: string
}

function RelaySecretField({
  action,
  actionField,
  disabled,
  field,
  label,
  onChange,
  relayId,
  rows,
  value,
}: RelaySecretFieldProps) {
  const { t } = useLocale()
  const textareaId = `relay-${String(field).replaceAll('_', '-')}`

  return (
    <Card className="min-w-0 gap-3 py-4">
      <CardHeader className="px-4">
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <CardTitle className="text-sm">{label}</CardTitle>
          {relayId && (
            <Select
              disabled={disabled}
              onValueChange={(next) =>
                onChange({ [actionField]: next as RelaySecretAction })
              }
              value={action}
            >
              <SelectTrigger aria-label={`${label} action`} className="w-32">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SECRET_ACTION_OPTIONS.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {t(option.label)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </div>
      </CardHeader>
      <CardContent className="px-4">
        <Label className="sr-only" htmlFor={textareaId}>
          {label}
        </Label>
        <Textarea
          autoComplete="off"
          disabled={disabled}
          id={textareaId}
          onChange={(event) => onChange({ [field]: event.currentTarget.value })}
          rows={rows}
          value={value}
        />
      </CardContent>
    </Card>
  )
}

type RelayDialogProps = {
  currentRelay: ManagedRelay | null
  form: RelayForm
  header: string
  onChange: (patch: Partial<RelayForm>) => void
  onOpenChange: (open: boolean) => void
  onSave: () => void
  open: boolean
  pending: boolean
  savePending: boolean
}

export function RelayDialog({
  currentRelay,
  form,
  header,
  onChange,
  onOpenChange,
  onSave,
  open,
  pending,
  savePending,
}: RelayDialogProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-h-[90vh] w-full overflow-y-auto sm:max-w-5xl"
        onEscapeKeyDown={(event) => {
          if (pending) event.preventDefault()
        }}
        onInteractOutside={(event) => {
          if (pending) event.preventDefault()
        }}
        showCloseButton={!pending}
      >
        <DialogHeader>
          <DialogTitle>{header}</DialogTitle>
        </DialogHeader>
        <fieldset className="grid min-w-0 gap-4" disabled={pending}>
          <div className="grid min-w-0 gap-4 sm:grid-cols-2">
            <div className="grid min-w-0 gap-2">
              <Label htmlFor="relay-name">{t('name')}</Label>
              <Input
                autoComplete="off"
                id="relay-name"
                onChange={(event) =>
                  onChange({ name: event.currentTarget.value })
                }
                placeholder={t('relayNameHint')}
                value={form.name}
              />
            </div>
            <div className="grid min-w-0 gap-2">
              <Label htmlFor="relay-url">{t('relayUrl')}</Label>
              <Input
                autoComplete="url"
                id="relay-url"
                onChange={(event) =>
                  onChange({ relay_url: event.currentTarget.value })
                }
                placeholder="wss://relay.example.com/ws/worker"
                value={form.relay_url}
              />
            </div>
            <div className="grid min-w-0 gap-2">
              <Label htmlFor="relay-tls-mode">{t('relayTlsMode')}</Label>
              <Select
                disabled={pending}
                onValueChange={(value) =>
                  onChange({ tls_mode: value as TlsMode })
                }
                value={form.tls_mode}
              >
                <SelectTrigger
                  aria-label={t('relayTlsMode')}
                  id="relay-tls-mode"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TLS_OPTIONS.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {t(option.label)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid min-w-0 gap-2">
              <Label htmlFor="relay-bridge-mode">{t('relayBridgeMode')}</Label>
              <Select
                disabled={pending}
                onValueChange={(value) =>
                  onChange({
                    bridge_encryption_mode: value as BridgeEncryptionMode,
                  })
                }
                value={form.bridge_encryption_mode}
              >
                <SelectTrigger
                  aria-label={t('relayBridgeMode')}
                  id="relay-bridge-mode"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {BRIDGE_OPTIONS.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {t(option.label)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Checkbox
              checked={form.enabled}
              disabled={pending}
              id="relay-enabled"
              onCheckedChange={(checked) =>
                onChange({ enabled: checked === true })
              }
            />
            <Label htmlFor="relay-enabled">{t('relayEnabled')}</Label>
          </div>

          <div className="grid min-w-0 gap-4 md:grid-cols-2">
            <RelaySecretField
              action={form.relay_ca_action}
              actionField="relay_ca_action"
              disabled={pending}
              field="relay_ca_pem"
              label={t('relayCaPem')}
              onChange={onChange}
              relayId={form.relay_id}
              rows={5}
              value={form.relay_ca_pem}
            />
            {form.bridge_encryption_mode === 'required' && (
              <RelaySecretField
                action={form.bridge_key_action}
                actionField="bridge_key_action"
                disabled={pending}
                field="bridge_encryption_key"
                label={t('relayBridgeKey')}
                onChange={onChange}
                relayId={form.relay_id}
                rows={5}
                value={form.bridge_encryption_key}
              />
            )}
          </div>

          {form.tls_mode === 'mtls' && (
            <div className="grid min-w-0 gap-4 md:grid-cols-2">
              <RelaySecretField
                action={form.client_cert_action}
                actionField="client_cert_action"
                disabled={pending}
                field="client_cert_pem"
                label={t('relayClientCertPem')}
                onChange={onChange}
                relayId={form.relay_id}
                rows={6}
                value={form.client_cert_pem}
              />
              <RelaySecretField
                action={form.client_key_action}
                actionField="client_key_action"
                disabled={pending}
                field="client_key_pem"
                label={t('relayClientKeyPem')}
                onChange={onChange}
                relayId={form.relay_id}
                rows={6}
                value={form.client_key_pem}
              />
            </div>
          )}

          {form.relay_id && (
            <div className="grid gap-1 text-xs text-muted-foreground">
              <span>
                {t('relayLastConnectedAt')}:{' '}
                {formatDateTime(
                  currentRelay?.last_connected_at,
                  locale,
                  undefined,
                  timeZone.effective,
                )}
              </span>
              <span>
                {t('relayLastDisconnectedAt')}:{' '}
                {formatDateTime(
                  currentRelay?.last_disconnected_at,
                  locale,
                  undefined,
                  timeZone.effective,
                )}
              </span>
            </div>
          )}
        </fieldset>
        <DialogFooter>
          <Button
            disabled={pending}
            onClick={() => onOpenChange(false)}
            type="button"
            variant="ghost"
          >
            {t('cancel')}
          </Button>
          <Button disabled={pending} onClick={onSave} type="button">
            {savePending && (
              <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
            )}
            <span>{t('save')}</span>
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
