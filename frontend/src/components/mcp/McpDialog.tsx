import { useEffect, useId, useMemo, useState, type ReactNode } from 'react'
import { ArrowLeft, Globe, Info, LoaderCircle } from 'lucide-react'
import type {
  McpCatalogResponse,
  McpProviderDescriptor,
  User,
} from '@/generated/admin-api'
import type { McpForm } from '@/models'
import { McpBearerTokensEditor } from '@/components/mcp/McpBearerTokensEditor'
import { McpCatalogMultiSelect } from '@/components/mcp/McpCatalogMultiSelect'
import { McpEnvironmentEditor } from '@/components/mcp/McpEnvironmentEditor'
import { ProxySettingsFields } from '@/components/shared/ProxySettingsFields'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
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
import { useLocale } from '@/lib/i18n'
import { MCP_PROTOCOL_VERSION_AUTO, MCP_PROTOCOL_VERSIONS } from '@/lib/mcp'
import { isProxyUrlValid } from '@/components/shared/ProxySettingsFields'

type McpDialogProps = {
  catalog: McpCatalogResponse
  catalogError: boolean
  catalogLoading: boolean
  form: McpForm
  header: string
  isAdmin: boolean
  onChange: (form: McpForm) => void
  onOpenChange: (open: boolean) => void
  onRefreshCatalog: () => void
  onSave: () => void
  open: boolean
  providers: McpProviderDescriptor[]
  savePending: boolean
  users: User[]
}

type DialogView = 'main' | 'proxy'

export function McpDialog({
  catalog,
  catalogError,
  catalogLoading,
  form,
  header,
  isAdmin,
  onChange,
  onOpenChange,
  onRefreshCatalog,
  onSave,
  open,
  providers,
  savePending,
  users,
}: McpDialogProps) {
  const { t } = useLocale()
  const idPrefix = useId()
  const [view, setView] = useState<DialogView>('main')
  const selectedProvider = providers.find(
    (provider) => provider.id === form.provider_kind,
  )
  const hostedPreset = Boolean(
    form.transport === 'http' &&
    selectedProvider?.default_url != null &&
    selectedProvider.auth === 'bearer',
  )
  const httpProviderOptions = useMemo(
    () =>
      providers.filter(
        (provider) => provider.id === 'generic' || provider.default_url,
      ),
    [providers],
  )
  const hasCatalogPreview =
    catalog.tools.length > 0 ||
    catalog.resources.length > 0 ||
    catalog.prompts.length > 0
  const selectedTools =
    form.tool_filter_mode === 'whitelist'
      ? form.allowed_tools
      : form.disabled_tools
  const toolLabel =
    form.tool_filter_mode === 'whitelist'
      ? t('allowedTools')
      : t('disabledTools')
  const toolPlaceholder =
    form.tool_filter_mode === 'whitelist'
      ? t('allowedToolsPlaceholder')
      : t('disabledToolsPlaceholder')
  const proxyValid = isProxyUrlValid(form.proxy_url)
  const sourceBoundAdmin = isAdmin && form.source_endpoint_id !== null

  useEffect(() => {
    setView('main')
  }, [open])

  useEffect(() => {
    if (
      selectedProvider?.default_url != null &&
      selectedProvider.auth === 'bearer'
    ) {
      onChange({
        ...form,
        url: selectedProvider.default_url,
        auth_mode: 'bearer',
      })
    }
  }, [
    form.provider_kind,
    onChange,
    selectedProvider?.auth,
    selectedProvider?.default_url,
  ])

  function update(patch: Partial<McpForm>): void {
    onChange({ ...form, ...patch })
  }

  function setSelectedTools(values: string[]): void {
    update(
      form.tool_filter_mode === 'whitelist'
        ? { allowed_tools: values }
        : { disabled_tools: values },
    )
  }

  function setTransport(transport: McpForm['transport']): void {
    update({
      transport,
      ...(transport !== 'builtin_minimax' && form.source_endpoint_id
        ? { source_endpoint_id: null }
        : {}),
    })
  }

  function setProvider(providerKind: string): void {
    update({ provider_kind: providerKind })
  }

  function setProtocolVersion(value: string): void {
    update({
      lifecycle_manual_protocol_version:
        value === MCP_PROTOCOL_VERSION_AUTO ? null : value,
    })
  }

  function updateProxy(value: { proxyUrl: string; hasSaved: boolean }): void {
    update({ proxy_url: value.proxyUrl, has_saved_proxy_url: value.hasSaved })
  }

  function close(): void {
    if (savePending) return
    onOpenChange(false)
  }

  const ids = {
    auth: `${idPrefix}-auth`,
    aggregate: `${idPrefix}-aggregate`,
    lifecyclePolicy: `${idPrefix}-lifecycle-policy`,
    protocolVersion: `${idPrefix}-protocol-version`,
    name: `${idPrefix}-name`,
    owner: `${idPrefix}-owner`,
    provider: `${idPrefix}-provider`,
    scope: `${idPrefix}-scope`,
    toolFilter: `${idPrefix}-tool-filter`,
    transport: `${idPrefix}-transport`,
    url: `${idPrefix}-url`,
    command: `${idPrefix}-command`,
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-h-[min(94vh,58rem)] overflow-y-auto sm:max-w-[min(calc(100vw_-_2rem),56rem)]"
        onEscapeKeyDown={(event) => {
          if (savePending) event.preventDefault()
        }}
        onInteractOutside={(event) => {
          if (savePending) event.preventDefault()
        }}
        showCloseButton={!savePending}
      >
        <DialogHeader>
          <DialogTitle>{header}</DialogTitle>
        </DialogHeader>
        <form
          className="grid min-w-0 gap-4"
          id="mcp-server-form"
          onSubmit={(event) => {
            event.preventDefault()
            if (!savePending && proxyValid) onSave()
          }}
        >
          <fieldset className="grid min-w-0 gap-3" disabled={savePending}>
            {view === 'proxy' ? (
              <>
                <div className="flex min-w-0 items-center gap-2">
                  <Button
                    aria-label={t('cancel')}
                    onClick={() => setView('main')}
                    size="icon-sm"
                    type="button"
                    variant="ghost"
                  >
                    <ArrowLeft aria-hidden="true" />
                  </Button>
                  <h3 className="font-medium">{t('proxySettings')}</h3>
                </div>
                <div className="grid min-w-0 gap-2 rounded-md border bg-muted p-3">
                  <div className="flex items-center gap-2">
                    <Label>{t('proxyUrl')}</Label>
                    <InfoHint text={t('mcpProxyUrlHint')} />
                  </div>
                  <ProxySettingsFields
                    hasSaved={form.has_saved_proxy_url}
                    onChange={updateProxy}
                    proxyUrl={form.proxy_url}
                  />
                  {!proxyValid ? (
                    <p className="text-xs text-destructive" role="alert">
                      {t('proxyAddress')}
                    </p>
                  ) : null}
                </div>
              </>
            ) : (
              <>
                <div
                  className={
                    isAdmin
                      ? 'grid min-w-0 gap-3 md:grid-cols-[9rem_minmax(0,1fr)_minmax(0,1fr)]'
                      : 'grid min-w-0 gap-3 md:grid-cols-2'
                  }
                >
                  {isAdmin ? (
                    <Field label={t('scope')}>
                      <Select
                        onValueChange={(scope) =>
                          update({
                            scope: scope === 'user' ? 'user' : 'admin',
                            owner_user_id:
                              scope === 'user' ? form.owner_user_id : null,
                          })
                        }
                        value={form.scope}
                      >
                        <SelectTrigger
                          aria-label={t('scope')}
                          className="w-full"
                          id={ids.scope}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="admin">
                            {t('scopeAdmin')}
                          </SelectItem>
                          <SelectItem value="user">{t('scopeUser')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  ) : null}
                  <Field label={t('name')}>
                    <Input
                      aria-label={t('name')}
                      autoComplete="off"
                      id={ids.name}
                      onChange={(event) =>
                        update({ name: event.currentTarget.value })
                      }
                      placeholder={t('name')}
                      value={form.name}
                    />
                  </Field>
                  <Field label={t('aggregateNamingModePassthroughPreferred')}>
                    <Select
                      onValueChange={(value) =>
                        update({
                          aggregate_naming_mode:
                            value === 'qualified_only'
                              ? 'qualified_only'
                              : 'passthrough_preferred',
                        })
                      }
                      value={form.aggregate_naming_mode}
                    >
                      <SelectTrigger
                        aria-label={t(
                          'aggregateNamingModePassthroughPreferred',
                        )}
                        className="w-full"
                        disabled={sourceBoundAdmin}
                        id={ids.aggregate}
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="passthrough_preferred">
                          {t('aggregateNamingModePassthroughPreferred')}
                        </SelectItem>
                        <SelectItem value="qualified_only">
                          {t('aggregateNamingModeQualifiedOnly')}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </Field>
                </div>
                {isAdmin && form.scope === 'user' ? (
                  <Field label={t('owner')}>
                    <Select
                      onValueChange={(value) =>
                        update({ owner_user_id: value ? Number(value) : null })
                      }
                      value={
                        form.owner_user_id == null
                          ? undefined
                          : String(form.owner_user_id)
                      }
                    >
                      <SelectTrigger
                        aria-label={t('ownerUser')}
                        className="w-full sm:max-w-md"
                        id={ids.owner}
                      >
                        <SelectValue placeholder={t('ownerUser')} />
                      </SelectTrigger>
                      <SelectContent>
                        {users.map((user) => (
                          <SelectItem
                            key={user.user_id}
                            value={String(user.user_id)}
                          >
                            {user.login_name} — {user.display_name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </Field>
                ) : null}

                <div className="grid min-w-0 gap-3 md:grid-cols-[minmax(0,28rem)_minmax(0,1fr)]">
                  <Field label={t('transport')}>
                    <Select
                      onValueChange={(value) =>
                        setTransport(value as McpForm['transport'])
                      }
                      value={form.transport}
                    >
                      <SelectTrigger
                        aria-label={t('transport')}
                        className="w-full sm:max-w-md"
                        id={ids.transport}
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="http">HTTP MCP</SelectItem>
                        <SelectItem value="stdio">stdio</SelectItem>
                        {form.source_endpoint_id ? (
                          <SelectItem value="builtin_minimax">
                            {t('minimaxManaged')}
                          </SelectItem>
                        ) : null}
                      </SelectContent>
                    </Select>
                  </Field>
                  {form.transport === 'http' && !hostedPreset ? (
                    <Field label={t('url')}>
                      <Input
                        aria-label={t('endpoint')}
                        autoComplete="url"
                        id={ids.url}
                        onChange={(event) =>
                          update({ url: event.currentTarget.value })
                        }
                        placeholder="http://127.0.0.1:3000/mcp"
                        value={form.url}
                      />
                    </Field>
                  ) : form.transport === 'stdio' ? (
                    <div className="flex min-w-0 items-center gap-1">
                      <Input
                        aria-label={t('stdioCommandHint')}
                        autoComplete="off"
                        id={ids.command}
                        onChange={(event) =>
                          update({
                            command_argv_text: event.currentTarget.value,
                          })
                        }
                        placeholder='["uvx", "minimax-coding-plan-mcp", "-y"]'
                        value={form.command_argv_text}
                      />
                      <InfoHint text={t('stdioCommandHint')} />
                    </div>
                  ) : form.transport === 'builtin_minimax' ? (
                    <div className="flex min-w-0 items-center gap-2 rounded-md border bg-muted p-3 text-sm text-muted-foreground">
                      <Info aria-hidden="true" className="size-4 shrink-0" />
                      <span>{t('minimaxManagedHint')}</span>
                    </div>
                  ) : null}
                </div>

                {form.transport === 'http' ? (
                  <>
                    <Field
                      hint={t('providerDefaultUrlHint')}
                      label={t('providerKind')}
                    >
                      <Select
                        onValueChange={setProvider}
                        value={form.provider_kind || 'generic'}
                      >
                        <SelectTrigger
                          aria-label={t('providerKind')}
                          className="w-full sm:max-w-md"
                          id={ids.provider}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {httpProviderOptions.map((provider) => (
                            <SelectItem key={provider.id} value={provider.id}>
                              {provider.id === 'generic'
                                ? t('providerGeneric')
                                : provider.display_name}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </Field>
                    {!hostedPreset ? (
                      <Field label={t('authMode')}>
                        <Select
                          onValueChange={(value) =>
                            update({ auth_mode: value as McpForm['auth_mode'] })
                          }
                          value={form.auth_mode}
                        >
                          <SelectTrigger
                            aria-label={t('authMode')}
                            className="w-full"
                            id={ids.auth}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="none">
                              {t('authModeNone')}
                            </SelectItem>
                            <SelectItem value="bearer">
                              {t('authModeBearer')}
                            </SelectItem>
                            <SelectItem value="basic">
                              {t('authModeBasic')}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                    ) : null}
                    {form.auth_mode === 'bearer' ? (
                      <McpBearerTokensEditor
                        disabled={savePending}
                        onChange={(bearer_tokens) => update({ bearer_tokens })}
                        tokens={form.bearer_tokens}
                      />
                    ) : null}
                    {form.auth_mode === 'basic' && !hostedPreset ? (
                      <div className="grid min-w-0 gap-3 md:grid-cols-2">
                        <Field label={t('authModeBasic')}>
                          <Input
                            aria-label={t('basicUsernamePlaceholder')}
                            autoComplete="username"
                            onChange={(event) =>
                              update({
                                basic_username: event.currentTarget.value,
                              })
                            }
                            placeholder={t('basicUsernamePlaceholder')}
                            value={form.basic_username}
                          />
                        </Field>
                        <Field label={t('basicPasswordPlaceholder')}>
                          <Input
                            aria-label={t('basicPasswordPlaceholder')}
                            autoComplete="new-password"
                            onChange={(event) =>
                              update({
                                basic_password: event.currentTarget.value,
                              })
                            }
                            placeholder={
                              form.has_basic_password
                                ? t('savedSecret')
                                : t('basicPasswordPlaceholder')
                            }
                            type="password"
                            value={form.basic_password}
                          />
                        </Field>
                      </div>
                    ) : null}
                    <div className="flex min-w-0 flex-wrap items-center gap-2 text-xs text-muted-foreground">
                      <span>{t('proxyUrl')}</span>
                      <InfoHint text={t('mcpProxyUrlHint')} />
                      {form.has_saved_proxy_url ? (
                        <span className="rounded-full border px-2 py-0.5">
                          {t('saved')}
                        </span>
                      ) : null}
                      <Button
                        aria-label={t('proxyUrl')}
                        aria-pressed={
                          form.proxy_url.trim() !== '' ||
                          form.has_saved_proxy_url
                        }
                        onClick={() => setView('proxy')}
                        size="icon-sm"
                        type="button"
                        variant={
                          form.proxy_url.trim() !== '' ||
                          form.has_saved_proxy_url
                            ? 'default'
                            : 'outline'
                        }
                      >
                        <Globe aria-hidden="true" />
                      </Button>
                    </div>
                  </>
                ) : null}

                {form.transport === 'stdio' ? (
                  <McpEnvironmentEditor
                    disabled={savePending}
                    onChange={(environment_variables) =>
                      update({ environment_variables })
                    }
                    variables={form.environment_variables}
                  />
                ) : null}

                <section className="grid min-w-0 gap-3 rounded-md border bg-muted p-3">
                  <div className="grid min-w-0 gap-3 md:grid-cols-3">
                    <Field label={t('toolFilterMode')}>
                      <Select
                        onValueChange={(value) =>
                          update({
                            tool_filter_mode:
                              value as McpForm['tool_filter_mode'],
                          })
                        }
                        value={form.tool_filter_mode}
                      >
                        <SelectTrigger
                          aria-label={t('toolFilterMode')}
                          className="w-full"
                          id={ids.toolFilter}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="blacklist">
                            {t('toolFilterModeBlacklist')}
                          </SelectItem>
                          <SelectItem value="whitelist">
                            {t('toolFilterModeWhitelist')}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field
                      hint={t('lifecyclePolicyHint')}
                      label={t('lifecyclePolicy')}
                    >
                      <Select
                        onValueChange={(value) =>
                          update({
                            lifecycle_policy:
                              value as McpForm['lifecycle_policy'],
                          })
                        }
                        value={form.lifecycle_policy}
                      >
                        <SelectTrigger
                          aria-label={t('lifecyclePolicy')}
                          className="w-full"
                          id={ids.lifecyclePolicy}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="auto">
                            {t('lifecyclePolicyAuto')}
                          </SelectItem>
                          <SelectItem value="legacy_initialize">
                            {t('lifecyclePolicyLegacy')}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field
                      hint={t('lifecycleManualProtocolVersionHint')}
                      label={t('lifecycleManualProtocolVersion')}
                    >
                      <Select
                        onValueChange={setProtocolVersion}
                        value={
                          form.lifecycle_manual_protocol_version ??
                          MCP_PROTOCOL_VERSION_AUTO
                        }
                      >
                        <SelectTrigger
                          aria-label={t('lifecycleManualProtocolVersion')}
                          className="w-full font-mono"
                          id={ids.protocolVersion}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value={MCP_PROTOCOL_VERSION_AUTO}>
                            {t('lifecycleManualProtocolVersionAuto')}
                          </SelectItem>
                          {MCP_PROTOCOL_VERSIONS.map((version) => (
                            <SelectItem key={version} value={version}>
                              {version}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </Field>
                  </div>
                  <div className="grid min-w-0 gap-3 md:grid-cols-2">
                    <Field label={toolLabel}>
                      <McpCatalogMultiSelect
                        disabled={savePending}
                        items={catalog.tools}
                        loading={catalogLoading}
                        onChange={setSelectedTools}
                        placeholder={toolPlaceholder}
                        value={selectedTools}
                      />
                    </Field>
                    <Field
                      hint={t('disabledResourcesHint')}
                      label={t('disabledResources')}
                    >
                      <McpCatalogMultiSelect
                        disabled={savePending}
                        items={catalog.resources}
                        loading={catalogLoading}
                        onChange={(disabled_resources) =>
                          update({ disabled_resources })
                        }
                        placeholder={t('disabledResourcesPlaceholder')}
                        value={form.disabled_resources}
                      />
                    </Field>
                  </div>
                  {catalogError ? (
                    <Alert
                      className="flex flex-wrap items-center justify-between gap-2"
                      role="alert"
                    >
                      <span>{t('error')}</span>
                      <Button
                        onClick={onRefreshCatalog}
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        {t('retry')}
                      </Button>
                    </Alert>
                  ) : null}
                  {catalogLoading ? (
                    <div
                      aria-label={t('loadingTools')}
                      className="flex items-center gap-2 text-xs text-muted-foreground"
                      role="status"
                    >
                      <LoaderCircle
                        aria-hidden="true"
                        className="size-4 animate-spin"
                      />
                      {t('loadingTools')}
                    </div>
                  ) : null}
                  {hasCatalogPreview ? (
                    <div className="grid min-w-0 gap-3 rounded-md border bg-background p-3">
                      <h3 className="text-xs font-medium">
                        {t('aggregatePreview')}
                      </h3>
                      <CatalogPreview
                        items={catalog.tools}
                        label={t('tools')}
                      />
                      <CatalogPreview
                        items={catalog.resources}
                        label={t('resources')}
                      />
                      <CatalogPreview
                        items={catalog.prompts}
                        label={t('prompts')}
                      />
                    </div>
                  ) : null}
                </section>
              </>
            )}
          </fieldset>
          <DialogFooter className="border-t pt-3">
            <Button
              disabled={savePending}
              onClick={close}
              type="button"
              variant="outline"
            >
              {t('cancel')}
            </Button>
            <Button disabled={savePending || !proxyValid} type="submit">
              {savePending ? (
                <LoaderCircle aria-hidden="true" className="animate-spin" />
              ) : null}
              {t('save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

type FieldProps = {
  children: ReactNode
  hint?: string
  label: string
}

function Field({ children, hint, label }: FieldProps) {
  return (
    <div className="grid min-w-0 gap-1.5">
      <div className="flex min-w-0 items-center gap-1 text-xs font-medium">
        <span>{label}</span>
        {hint ? <InfoHint text={hint} /> : null}
      </div>
      {children}
    </div>
  )
}

function InfoHint({ text }: { text: string }) {
  return (
    <span aria-label={text} className="inline-flex shrink-0" title={text}>
      <Info aria-hidden="true" className="size-3.5 text-muted-foreground" />
    </span>
  )
}

function CatalogPreview({
  items,
  label,
}: {
  items: McpCatalogResponse['tools']
  label: string
}) {
  if (items.length === 0) return null
  return (
    <section className="grid min-w-0 gap-1">
      <h4 className="text-xs font-medium">{label}</h4>
      <ul className="grid min-w-0 gap-1">
        {items.map((item) => (
          <li className="grid min-w-0 gap-0.5 text-xs" key={item.name}>
            <span className="min-w-0 truncate font-mono" title={item.name}>
              {item.name}
            </span>
            <span className="break-words font-mono text-muted-foreground">
              → {item.aggregate_names.join(', ')}
            </span>
          </li>
        ))}
      </ul>
    </section>
  )
}
