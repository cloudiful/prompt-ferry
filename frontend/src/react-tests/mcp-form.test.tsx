import { render, screen, fireEvent } from '@testing-library/react'
import { useState, useCallback } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  McpCatalogResponse,
  McpProviderDescriptor,
} from '@/generated/admin-api'
import type { McpForm } from '@/models'
import { createEmptyMcpForm } from '@/admin-mappers/forms/mcp'
import { MCP_PROTOCOL_VERSION_AUTO, MCP_PROTOCOL_VERSIONS } from '@/lib/mcp'
import { McpDialog } from '@/components/mcp/McpDialog'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'

const providers: McpProviderDescriptor[] = [
  {
    auth: 'none',
    default_url: null,
    display_name: 'Generic',
    id: 'generic',
    provider_balance_supported: false,
    unit: 'requests',
  },
  {
    auth: 'bearer',
    default_url: 'https://mcp.context7.com/mcp',
    display_name: 'Context7',
    id: 'context7',
    provider_balance_supported: false,
    unit: 'requests',
  },
]

const catalog: McpCatalogResponse = { tools: [], resources: [], prompts: [] }

function renderDialog(initial: McpForm, locale: 'zh-CN' | 'en-US' = 'en-US') {
  const changes: McpForm[] = []
  function Harness() {
    const [form, setForm] = useState(initial)
    const onChange = useCallback((next: McpForm) => {
      changes.push(next)
      setForm(next)
    }, [])
    return (
      <LocaleProvider>
        <NotifierProvider>
          <McpDialog
            catalog={catalog}
            catalogError={false}
            catalogLoading={false}
            form={form}
            header="MCP"
            isAdmin
            onChange={onChange}
            onOpenChange={() => {}}
            onRefreshCatalog={() => {}}
            onSave={() => {}}
            open
            providers={providers}
            savePending={false}
            users={[]}
          />
        </NotifierProvider>
      </LocaleProvider>
    )
  }
  const view = render(<Harness />)
  return { view, changes }
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
})

describe('mcp form defaults', () => {
  it('starts with the complete empty defaults', () => {
    expect(createEmptyMcpForm()).toEqual({
      server_id: '',
      source_endpoint_id: null,
      scope: 'admin',
      owner_user_id: null,
      name: '',
      aggregate_naming_mode: 'passthrough_preferred',
      transport: 'http',
      provider_kind: 'generic',
      url: '',
      command_argv_text: '[]',
      auth_mode: 'none',
      bearer_tokens: [],
      basic_username: '',
      basic_password: '',
      has_basic_password: false,
      proxy_url: '',
      has_saved_proxy_url: false,
      http_headers_text: '{}',
      environment_variables: [],
      tool_filter_mode: 'blacklist',
      allowed_tools: [],
      disabled_tools: [],
      disabled_resources: [],
      enabled: true,
      timeout_ms: 30000,
      lifecycle_policy: 'auto',
      lifecycle_manual_protocol_version: null,
    })
  })
})

describe('mcp protocol version select', () => {
  it('mounts without an empty option and shows the automatic sentinel', () => {
    const { changes } = renderDialog(createEmptyMcpForm())

    expect(
      screen.getByRole('combobox', {
        name: 'Pinned protocol version (optional)',
      }).textContent,
    ).toContain('Automatic (negotiated)')

    fireEvent.click(
      screen.getByRole('combobox', {
        name: 'Pinned protocol version (optional)',
      }),
    )

    const options = screen.getAllByRole('option')
    expect(options.length).toBe(MCP_PROTOCOL_VERSIONS.length + 1)
    for (const option of options) {
      expect(option.textContent?.trim().length ?? 0).toBeGreaterThan(0)
    }
    for (const version of MCP_PROTOCOL_VERSIONS) {
      expect(
        options.some((option) => option.textContent?.trim() === version),
      ).toBe(true)
    }
    expect(changes).toEqual([])
  })

  it('round-trips an explicit version and maps the sentinel back to null', () => {
    const { changes } = renderDialog({
      ...createEmptyMcpForm(),
      lifecycle_manual_protocol_version: '2025-06-18',
      name: 'Fixture server',
      url: 'https://mcp.example.test/mcp',
    })

    fireEvent.click(
      screen.getByRole('combobox', {
        name: 'Pinned protocol version (optional)',
      }),
    )
    fireEvent.click(screen.getByRole('option', { name: '2025-11-25' }))

    expect(changes.at(-1)?.lifecycle_manual_protocol_version).toBe('2025-11-25')
    expect(changes.at(-1)?.name).toBe('Fixture server')
    expect(changes.at(-1)?.url).toBe('https://mcp.example.test/mcp')

    fireEvent.click(
      screen.getByRole('combobox', {
        name: 'Pinned protocol version (optional)',
      }),
    )
    fireEvent.click(
      screen.getByRole('option', { name: 'Automatic (negotiated)' }),
    )

    expect(changes.at(-1)?.lifecycle_manual_protocol_version).toBeNull()
    expect(changes.at(-1)?.name).toBe('Fixture server')
    expect(changes.at(-1)?.url).toBe('https://mcp.example.test/mcp')
    expect(MCP_PROTOCOL_VERSION_AUTO).toBe('auto')
  })

  it('shows the localized generic provider name in both locales', () => {
    const english = renderDialog(createEmptyMcpForm(), 'en-US')
    expect(
      screen.getByRole('combobox', { name: 'Provider' }).textContent,
    ).toContain('Generic')
    english.view.unmount()

    localStorage.setItem('prompt-ferry:locale', 'zh-CN')
    const chinese = renderDialog(createEmptyMcpForm(), 'zh-CN')
    expect(
      screen.getByRole('combobox', { name: '提供商' }).textContent,
    ).toContain('通用')
    chinese.view.unmount()
  })

  it('drops the redundant bearer sentence and widens the dialog', () => {
    renderDialog({
      ...createEmptyMcpForm(),
      name: 'Hosted Context7',
      provider_kind: 'context7',
      transport: 'http',
      url: 'https://mcp.context7.com/mcp',
      auth_mode: 'bearer',
    })
    expect(
      screen.getByRole('combobox', { name: 'Provider' }).textContent,
    ).toContain('Context7')
    expect(screen.queryByText(/always uses bearer token auth/i)).toBeNull()
    expect(screen.queryByText(/固定使用 Bearer 令牌认证/)).toBeNull()
    const dialog = screen.getByRole('dialog')
    expect(dialog.className).toContain(
      'sm:max-w-[min(calc(100vw_-_2rem),56rem)]',
    )
    expect(dialog.className).not.toContain('max-w-3xl')
  })

  it('keeps the proxy action beside its label and one select width', () => {
    renderDialog({ ...createEmptyMcpForm(), name: 'Aligned MCP' })

    const proxyAction = screen.getByRole('button', { name: 'Proxy URL' })
    const proxyLabel = screen.getByText('Proxy URL')
    expect(proxyAction.parentElement).toBe(proxyLabel.parentElement)
    expect(proxyAction.parentElement?.className).not.toContain(
      'justify-between',
    )
    expect(proxyAction.previousElementSibling).not.toBeNull()

    const transport = screen
      .getAllByRole('combobox')
      .find((element) => element.id.endsWith('-transport'))
    const provider = screen
      .getAllByRole('combobox')
      .find((element) => element.id.endsWith('-provider'))
    expect(transport).toBeDefined()
    expect(provider).toBeDefined()
    expect(provider?.className).toContain('sm:max-w-md')
    expect(transport?.className).toBe(provider?.className)
  })
})
