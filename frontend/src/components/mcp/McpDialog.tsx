import { useEffect, useId, useMemo, useState, type ReactNode } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import type { FormValidateOrFn } from '@tanstack/form-core'
import { ArrowLeft, Globe, Info, LoaderCircle } from 'lucide-react'
import type {
  McpCatalogResponse,
  McpProviderDescriptor,
  McpServer,
  User,
} from '@/generated/admin-api'
import type { McpForm } from '@/models'
import { McpBearerTokensEditor } from '@/components/mcp/McpBearerTokensEditor'
import { McpCatalogMultiSelect } from '@/components/mcp/McpCatalogMultiSelect'
import { McpEnvironmentEditor } from '@/components/mcp/McpEnvironmentEditor'
import { McpOAuthPanel } from '@/components/mcp/McpOAuthPanel'
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
  header: string
  initialForm: McpForm
  isAdmin: boolean
  onOAuthConnect: (server: McpServer) => void
  onOAuthDisconnect: (server: McpServer) => void
  oauthPending: boolean
  oauthServer: McpServer | null
  onOpenChange: (open: boolean) => void
  onRefreshCatalog: () => void
  onSave: (form: McpForm) => void
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
  header,
  initialForm,
  isAdmin,
  onOAuthConnect,
  onOAuthDisconnect,
  oauthPending,
  oauthServer,
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
  const form = useForm<
    McpForm,
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    FormValidateOrFn<McpForm>,
    undefined,
    undefined,
    undefined,
    undefined,
    never
  >({
    defaultValues: initialForm,
    onSubmit: async ({ value }) => {
      onSave(value)
    },
    validators: {
      onSubmit: ({ value }) =>
        isProxyUrlValid(value.proxy_url) ? undefined : t('proxyAddress'),
    },
  })
  const values = useStore(form.store, (state) => state.values)
  const submitAttempts = useStore(
    form.store,
    (state) => state.submissionAttempts,
  )
  const submitError = useStore(form.store, (state) =>
    state.submissionAttempts > 0 && typeof state.errorMap.onSubmit === 'string'
      ? state.errorMap.onSubmit
      : undefined,
  )
  const selectedProvider = providers.find(
    (provider) => provider.id === values.provider_kind,
  )
  const hostedPreset = Boolean(
    values.transport === 'http' &&
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
  const toolLabel =
    values.tool_filter_mode === 'whitelist'
      ? t('allowedTools')
      : t('disabledTools')
  const toolPlaceholder =
    values.tool_filter_mode === 'whitelist'
      ? t('allowedToolsPlaceholder')
      : t('disabledToolsPlaceholder')
  const proxyValid =
    submitAttempts === 0
      ? isProxyUrlValid(values.proxy_url)
      : typeof form.store.state.errorMap.onSubmit === 'undefined'
  const sourceBoundAdmin = isAdmin && values.source_endpoint_id !== null

  useEffect(() => {
    setView('main')
  }, [open])

  useEffect(() => {
    if (
      selectedProvider?.default_url != null &&
      selectedProvider.auth === 'bearer'
    ) {
      form.setFieldValue('url', selectedProvider.default_url)
      form.setFieldValue('auth_mode', 'bearer')
    }
  }, [
    selectedProvider?.auth,
    selectedProvider?.default_url,
    values.provider_kind,
  ])

  function setTransport(transport: McpForm['transport']): void {
    form.setFieldValue('transport', transport)
    if (transport !== 'builtin_minimax' && values.source_endpoint_id) {
      form.setFieldValue('source_endpoint_id', null)
    }
  }

  function setProtocolVersion(value: string): void {
    form.setFieldValue(
      'lifecycle_manual_protocol_version',
      value === MCP_PROTOCOL_VERSION_AUTO ? null : value,
    )
  }

  function updateProxy(value: { proxyUrl: string; hasSaved: boolean }): void {
    form.setFieldValue('proxy_url', value.proxyUrl)
    form.setFieldValue('has_saved_proxy_url', value.hasSaved)
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
          noValidate
          id="mcp-server-form"
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            if (!savePending) void form.handleSubmit()
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
                    hasSaved={values.has_saved_proxy_url}
                    onChange={updateProxy}
                    proxyUrl={values.proxy_url}
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
                      <form.Field name="scope">
                        {(field) => (
                          <Select
                            onValueChange={(scope) => {
                              const next = scope === 'user' ? 'user' : 'admin'
                              field.handleChange(next)
                              form.setFieldValue(
                                'owner_user_id',
                                next === 'user' ? values.owner_user_id : null,
                              )
                            }}
                            value={field.state.value}
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
                              <SelectItem value="user">
                                {t('scopeUser')}
                              </SelectItem>
                            </SelectContent>
                          </Select>
                        )}
                      </form.Field>
                    </Field>
                  ) : null}
                  <Field label={t('name')}>
                    <form.Field name="name">
                      {(field) => (
                        <Input
                          aria-label={t('name')}
                          autoComplete="off"
                          id={ids.name}
                          onChange={(event) =>
                            field.handleChange(event.currentTarget.value)
                          }
                          placeholder={t('name')}
                          value={field.state.value}
                        />
                      )}
                    </form.Field>
                  </Field>
                  <Field label={t('aggregateNamingModePassthroughPreferred')}>
                    <form.Field name="aggregate_naming_mode">
                      {(field) => (
                        <Select
                          onValueChange={(value) =>
                            field.handleChange(
                              value === 'qualified_only'
                                ? 'qualified_only'
                                : 'passthrough_preferred',
                            )
                          }
                          value={field.state.value}
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
                      )}
                    </form.Field>
                  </Field>
                </div>
                {isAdmin && values.scope === 'user' ? (
                  <Field label={t('owner')}>
                    <form.Field name="owner_user_id">
                      {(field) => (
                        <Select
                          onValueChange={(value) =>
                            field.handleChange(value ? Number(value) : null)
                          }
                          value={
                            field.state.value == null
                              ? undefined
                              : String(field.state.value)
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
                      )}
                    </form.Field>
                  </Field>
                ) : null}

                <div className="grid min-w-0 gap-3 md:grid-cols-[minmax(0,28rem)_minmax(0,1fr)]">
                  <Field label={t('transport')}>
                    <form.Field name="transport">
                      {(field) => (
                        <Select
                          onValueChange={(value) =>
                            setTransport(value as McpForm['transport'])
                          }
                          value={field.state.value}
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
                            {values.source_endpoint_id ? (
                              <SelectItem value="builtin_minimax">
                                {t('minimaxManaged')}
                              </SelectItem>
                            ) : null}
                          </SelectContent>
                        </Select>
                      )}
                    </form.Field>
                  </Field>
                  {values.transport === 'http' && !hostedPreset ? (
                    <Field label={t('url')}>
                      <form.Field name="url">
                        {(field) => (
                          <Input
                            aria-label={t('endpoint')}
                            autoComplete="url"
                            id={ids.url}
                            onChange={(event) =>
                              field.handleChange(event.currentTarget.value)
                            }
                            placeholder="http://127.0.0.1:3000/mcp"
                            value={field.state.value}
                          />
                        )}
                      </form.Field>
                    </Field>
                  ) : values.transport === 'stdio' ? (
                    <div className="flex min-w-0 items-center gap-1">
                      <form.Field name="command_argv_text">
                        {(field) => (
                          <Input
                            aria-label={t('stdioCommandHint')}
                            autoComplete="off"
                            id={ids.command}
                            onChange={(event) =>
                              field.handleChange(event.currentTarget.value)
                            }
                            placeholder='["uvx", "minimax-coding-plan-mcp", "-y"]'
                            value={field.state.value}
                          />
                        )}
                      </form.Field>
                      <InfoHint text={t('stdioCommandHint')} />
                    </div>
                  ) : values.transport === 'builtin_minimax' ? (
                    <div className="flex min-w-0 items-center gap-2 rounded-md border bg-muted p-3 text-sm text-muted-foreground">
                      <Info aria-hidden="true" className="size-4 shrink-0" />
                      <span>{t('minimaxManagedHint')}</span>
                    </div>
                  ) : null}
                </div>

                {values.transport === 'http' ? (
                  <>
                    <Field
                      hint={t('providerDefaultUrlHint')}
                      label={t('providerKind')}
                    >
                      <form.Field name="provider_kind">
                        {(field) => (
                          <Select
                            onValueChange={field.handleChange}
                            value={field.state.value || 'generic'}
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
                                <SelectItem
                                  key={provider.id}
                                  value={provider.id}
                                >
                                  {provider.id === 'generic'
                                    ? t('providerGeneric')
                                    : provider.display_name}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        )}
                      </form.Field>
                    </Field>
                    {!hostedPreset ? (
                      <Field label={t('authMode')}>
                        <form.Field name="auth_mode">
                          {(field) => (
                            <Select
                              onValueChange={(value) =>
                                field.handleChange(
                                  value as McpForm['auth_mode'],
                                )
                              }
                              value={field.state.value}
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
                                {values.transport === 'http' &&
                                values.scope === 'user' &&
                                !hostedPreset ? (
                                  <SelectItem value="oauth">
                                    {t('authModeOAuth')}
                                  </SelectItem>
                                ) : null}
                              </SelectContent>
                            </Select>
                          )}
                        </form.Field>
                      </Field>
                    ) : null}
                    {values.auth_mode === 'oauth' && oauthServer ? (
                      <McpOAuthPanel
                        onConnect={onOAuthConnect}
                        onDisconnect={onOAuthDisconnect}
                        pending={oauthPending}
                        server={oauthServer}
                      />
                    ) : null}
                    {values.auth_mode === 'oauth' && !oauthServer ? (
                      <p className="text-xs text-muted-foreground">
                        {t('oauthHint')}
                      </p>
                    ) : null}
                    {values.auth_mode === 'bearer' ? (
                      <form.Field name="bearer_tokens">
                        {(field) => (
                          <McpBearerTokensEditor
                            disabled={savePending}
                            onChange={field.handleChange}
                            tokens={field.state.value}
                          />
                        )}
                      </form.Field>
                    ) : null}
                    {values.auth_mode === 'basic' && !hostedPreset ? (
                      <div className="grid min-w-0 gap-3 md:grid-cols-2">
                        <Field label={t('authModeBasic')}>
                          <form.Field name="basic_username">
                            {(field) => (
                              <Input
                                aria-label={t('basicUsernamePlaceholder')}
                                autoComplete="username"
                                onChange={(event) =>
                                  field.handleChange(event.currentTarget.value)
                                }
                                placeholder={t('basicUsernamePlaceholder')}
                                value={field.state.value}
                              />
                            )}
                          </form.Field>
                        </Field>
                        <Field label={t('basicPasswordPlaceholder')}>
                          <form.Field name="basic_password">
                            {(field) => (
                              <Input
                                aria-label={t('basicPasswordPlaceholder')}
                                autoComplete="new-password"
                                onChange={(event) =>
                                  field.handleChange(event.currentTarget.value)
                                }
                                placeholder={
                                  values.has_basic_password
                                    ? t('savedSecret')
                                    : t('basicPasswordPlaceholder')
                                }
                                type="password"
                                value={field.state.value}
                              />
                            )}
                          </form.Field>
                        </Field>
                      </div>
                    ) : null}
                    <div className="flex min-w-0 flex-wrap items-center gap-2 text-xs text-muted-foreground">
                      <span>{t('proxyUrl')}</span>
                      <InfoHint text={t('mcpProxyUrlHint')} />
                      {values.has_saved_proxy_url ? (
                        <span className="rounded-full border px-2 py-0.5">
                          {t('saved')}
                        </span>
                      ) : null}
                      <Button
                        aria-label={t('proxyUrl')}
                        aria-pressed={
                          values.proxy_url.trim() !== '' ||
                          values.has_saved_proxy_url
                        }
                        onClick={() => setView('proxy')}
                        size="icon-sm"
                        type="button"
                        variant={
                          values.proxy_url.trim() !== '' ||
                          values.has_saved_proxy_url
                            ? 'default'
                            : 'outline'
                        }
                      >
                        <Globe aria-hidden="true" />
                      </Button>
                    </div>
                  </>
                ) : null}

                {values.transport === 'stdio' ? (
                  <form.Field name="environment_variables">
                    {(field) => (
                      <McpEnvironmentEditor
                        disabled={savePending}
                        onChange={field.handleChange}
                        variables={field.state.value}
                      />
                    )}
                  </form.Field>
                ) : null}

                <section className="grid min-w-0 gap-3 rounded-md border bg-muted p-3">
                  <div className="grid min-w-0 gap-3 md:grid-cols-3">
                    <Field label={t('toolFilterMode')}>
                      <form.Field name="tool_filter_mode">
                        {(field) => (
                          <Select
                            onValueChange={(value) =>
                              field.handleChange(
                                value as McpForm['tool_filter_mode'],
                              )
                            }
                            value={field.state.value}
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
                        )}
                      </form.Field>
                    </Field>
                    <Field
                      hint={t('lifecyclePolicyHint')}
                      label={t('lifecyclePolicy')}
                    >
                      <form.Field name="lifecycle_policy">
                        {(field) => (
                          <Select
                            onValueChange={(value) =>
                              field.handleChange(
                                value as McpForm['lifecycle_policy'],
                              )
                            }
                            value={field.state.value}
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
                        )}
                      </form.Field>
                    </Field>
                    <Field
                      hint={t('lifecycleManualProtocolVersionHint')}
                      label={t('lifecycleManualProtocolVersion')}
                    >
                      <form.Field name="lifecycle_manual_protocol_version">
                        {(field) => (
                          <Select
                            onValueChange={setProtocolVersion}
                            value={
                              field.state.value ?? MCP_PROTOCOL_VERSION_AUTO
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
                        )}
                      </form.Field>
                    </Field>
                  </div>
                  <div className="grid min-w-0 gap-3 md:grid-cols-2">
                    <Field label={toolLabel}>
                      {values.tool_filter_mode === 'whitelist' ? (
                        <form.Field name="allowed_tools">
                          {(field) => (
                            <McpCatalogMultiSelect
                              disabled={savePending}
                              items={catalog.tools}
                              loading={catalogLoading}
                              onChange={field.handleChange}
                              placeholder={toolPlaceholder}
                              value={field.state.value}
                            />
                          )}
                        </form.Field>
                      ) : (
                        <form.Field name="disabled_tools">
                          {(field) => (
                            <McpCatalogMultiSelect
                              disabled={savePending}
                              items={catalog.tools}
                              loading={catalogLoading}
                              onChange={field.handleChange}
                              placeholder={toolPlaceholder}
                              value={field.state.value}
                            />
                          )}
                        </form.Field>
                      )}
                    </Field>
                    <Field
                      hint={t('disabledResourcesHint')}
                      label={t('disabledResources')}
                    >
                      <form.Field name="disabled_resources">
                        {(field) => (
                          <McpCatalogMultiSelect
                            disabled={savePending}
                            items={catalog.resources}
                            loading={catalogLoading}
                            onChange={field.handleChange}
                            placeholder={t('disabledResourcesPlaceholder')}
                            value={field.state.value}
                          />
                        )}
                      </form.Field>
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
            {submitError && view !== 'proxy' ? (
              <p className="m-0 text-xs text-destructive" role="alert">
                {submitError}
              </p>
            ) : null}
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
