import { useForm } from '@tanstack/react-form'
import type {
  BridgeEncryptionMode,
  ManagedRelay,
  TlsMode,
} from '@/generated/admin-api'
import {
  createEmptyRelayForm,
  relayToForm,
  type RelayForm,
  type RelaySecretAction,
} from '@/admin-mappers/forms/relay'
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
  disabled: boolean
  label: string
  onActionChange: (action: RelaySecretAction) => void
  onValueChange: (value: string) => void
  relayId: string
  rows: number
  value: string
}

function RelaySecretField({
  action,
  disabled,
  label,
  onActionChange,
  onValueChange,
  relayId,
  rows,
  value,
}: RelaySecretFieldProps) {
  const { t } = useLocale()
  const textareaId = `relay-${label.toLowerCase().replaceAll(' ', '-')}`

  return (
    <Card className="min-w-0 gap-3 py-4">
      <CardHeader className="px-4">
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <CardTitle className="text-sm">{label}</CardTitle>
          {relayId && (
            <Select
              disabled={disabled}
              onValueChange={(next) =>
                onActionChange(next as RelaySecretAction)
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
          onChange={(event) => onValueChange(event.currentTarget.value)}
          rows={rows}
          value={value}
        />
      </CardContent>
    </Card>
  )
}

type RelayDialogProps = {
  currentRelay: ManagedRelay | null
  header: string
  onOpenChange: (open: boolean) => void
  onSave: (form: RelayForm) => void
  open: boolean
  pending: boolean
  relay: ManagedRelay | null
  savePending: boolean
}

export function RelayDialog({
  currentRelay,
  header,
  onOpenChange,
  onSave,
  open,
  pending,
  relay,
  savePending,
}: RelayDialogProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const relayId = relay?.relay_id ?? ''

  const form = useForm({
    defaultValues: relay ? relayToForm(relay) : createEmptyRelayForm(),
    onSubmit: async ({ value }) => {
      onSave(value)
    },
  })

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
        <form
          className="grid min-w-0 gap-4"
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            void form.handleSubmit()
          }}
        >
          <fieldset className="grid min-w-0 gap-4" disabled={pending}>
            <div className="grid min-w-0 gap-4 sm:grid-cols-2">
              <div className="grid min-w-0 gap-2">
                <Label htmlFor="relay-name">{t('name')}</Label>
                <form.Field name="name">
                  {(field) => (
                    <Input
                      autoComplete="off"
                      id="relay-name"
                      onChange={(event) =>
                        field.handleChange(event.currentTarget.value)
                      }
                      placeholder={t('relayNameHint')}
                      value={field.state.value}
                    />
                  )}
                </form.Field>
              </div>
              <div className="grid min-w-0 gap-2">
                <Label htmlFor="relay-url">{t('relayUrl')}</Label>
                <form.Field name="relay_url">
                  {(field) => (
                    <Input
                      autoComplete="url"
                      id="relay-url"
                      onChange={(event) =>
                        field.handleChange(event.currentTarget.value)
                      }
                      placeholder="wss://relay.example.com/ws/worker"
                      value={field.state.value}
                    />
                  )}
                </form.Field>
              </div>
              <div className="grid min-w-0 gap-2">
                <Label htmlFor="relay-tls-mode">{t('relayTlsMode')}</Label>
                <form.Field name="tls_mode">
                  {(field) => (
                    <Select
                      disabled={pending}
                      onValueChange={(value) =>
                        field.handleChange(value as TlsMode)
                      }
                      value={field.state.value}
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
                  )}
                </form.Field>
              </div>
              <div className="grid min-w-0 gap-2">
                <Label htmlFor="relay-bridge-mode">
                  {t('relayBridgeMode')}
                </Label>
                <form.Field name="bridge_encryption_mode">
                  {(field) => (
                    <Select
                      disabled={pending}
                      onValueChange={(value) =>
                        field.handleChange(value as BridgeEncryptionMode)
                      }
                      value={field.state.value}
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
                  )}
                </form.Field>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <form.Field name="enabled">
                {(field) => (
                  <Checkbox
                    checked={field.state.value}
                    disabled={pending}
                    id="relay-enabled"
                    onCheckedChange={(checked) =>
                      field.handleChange(checked === true)
                    }
                  />
                )}
              </form.Field>
              <Label htmlFor="relay-enabled">{t('relayEnabled')}</Label>
            </div>

            <div className="grid min-w-0 gap-4 md:grid-cols-2">
              <form.Field name="relay_ca_pem">
                {(pemField) => (
                  <form.Field name="relay_ca_action">
                    {(actionField) => (
                      <RelaySecretField
                        action={actionField.state.value}
                        disabled={pending}
                        label={t('relayCaPem')}
                        onActionChange={(action) =>
                          actionField.handleChange(action)
                        }
                        onValueChange={(value) => pemField.handleChange(value)}
                        relayId={relayId}
                        rows={5}
                        value={pemField.state.value}
                      />
                    )}
                  </form.Field>
                )}
              </form.Field>
              <form.Subscribe
                selector={(state) => state.values.bridge_encryption_mode}
              >
                {(bridgeMode) =>
                  bridgeMode === 'required' ? (
                    <form.Field name="bridge_encryption_key">
                      {(keyField) => (
                        <form.Field name="bridge_key_action">
                          {(actionField) => (
                            <RelaySecretField
                              action={actionField.state.value}
                              disabled={pending}
                              label={t('relayBridgeKey')}
                              onActionChange={(action) =>
                                actionField.handleChange(action)
                              }
                              onValueChange={(value) =>
                                keyField.handleChange(value)
                              }
                              relayId={relayId}
                              rows={5}
                              value={keyField.state.value}
                            />
                          )}
                        </form.Field>
                      )}
                    </form.Field>
                  ) : null
                }
              </form.Subscribe>
            </div>

            <form.Subscribe selector={(state) => state.values.tls_mode}>
              {(tlsMode) =>
                tlsMode === 'mtls' ? (
                  <div className="grid min-w-0 gap-4 md:grid-cols-2">
                    <form.Field name="client_cert_pem">
                      {(certField) => (
                        <form.Field name="client_cert_action">
                          {(actionField) => (
                            <RelaySecretField
                              action={actionField.state.value}
                              disabled={pending}
                              label={t('relayClientCertPem')}
                              onActionChange={(action) =>
                                actionField.handleChange(action)
                              }
                              onValueChange={(value) =>
                                certField.handleChange(value)
                              }
                              relayId={relayId}
                              rows={6}
                              value={certField.state.value}
                            />
                          )}
                        </form.Field>
                      )}
                    </form.Field>
                    <form.Field name="client_key_pem">
                      {(keyField) => (
                        <form.Field name="client_key_action">
                          {(actionField) => (
                            <RelaySecretField
                              action={actionField.state.value}
                              disabled={pending}
                              label={t('relayClientKeyPem')}
                              onActionChange={(action) =>
                                actionField.handleChange(action)
                              }
                              onValueChange={(value) =>
                                keyField.handleChange(value)
                              }
                              relayId={relayId}
                              rows={6}
                              value={keyField.state.value}
                            />
                          )}
                        </form.Field>
                      )}
                    </form.Field>
                  </div>
                ) : null
              }
            </form.Subscribe>

            {relayId && (
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
            <Button disabled={pending} type="submit">
              {savePending && (
                <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
              )}
              <span>{t('save')}</span>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
