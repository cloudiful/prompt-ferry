import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { messages } from '@/i18n'
import { endpointOpenAiMessages } from '@/i18n/modules/endpoints.openai'
import { emptyUserForm } from '@/models/users'
import { CreateUserDialog } from '@/components/users/CreateUserDialog'
import { SettingsGeneralTab } from '@/components/settings/SettingsGeneralTab'
import { LocaleProvider } from '@/lib/i18n'
import { ThemeProvider } from '@/lib/theme'

const locales = ['zh-CN', 'en-US'] as const

const HOST_PAGE_KEYS = [
  'relayHost',
  'relayHostLogin',
  'relayHostTokenPlaceholder',
  'relayHostRoleTitle',
  'relayHostSettingsTitle',
  'relayHostAdminBind',
  'relayHostAdminBindHelp',
  'relayHostStatusTitle',
  'relayHostRelayReady',
  'relayHostWorker',
  'relayRoleIntegrated',
  'relayRoleIntegratedDesc',
  'relayRoleWorker',
  'relayRoleWorkerDesc',
  'relayRoleRelay',
  'relayRoleRelayDesc',
  'relayRoleCurrent',
  'relayRolePending',
  'relayRoleSelected',
  'relayUseRole',
  'relayRoleSaved',
  'relayRestartRequired',
  'relayRestartRequiredDesc',
  'relayRestartNow',
  'relayRestartRequested',
  'relayWorkerDisconnected',
  'relayWorkerDisconnectedDesc',
  'relayWorkerUnavailable',
  'relayStatusConnected',
  'relayStatusDisconnected',
] as const

function renderWithLocale(
  locale: (typeof locales)[number],
  children: React.ReactNode,
) {
  localStorage.setItem('prompt-ferry:locale', locale)
  return render(<LocaleProvider>{children}</LocaleProvider>)
}

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  document.documentElement.lang = ''
  Element.prototype.scrollIntoView = vi.fn()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
})

describe('locale catalog', () => {
  it('keeps the zh-CN and en-US key sets identical', () => {
    expect(Object.keys(messages['zh-CN']).sort()).toEqual(
      Object.keys(messages['en-US']).sort(),
    )
  })

  it('carries every host, role and restart key in both locales with text', () => {
    const missing = HOST_PAGE_KEYS.filter((key) => {
      const zh = messages['zh-CN'][key]
      const en = messages['en-US'][key]
      return (
        typeof zh !== 'string' ||
        zh.length === 0 ||
        typeof en !== 'string' ||
        en.length === 0
      )
    })
    expect(missing).toEqual([])
  })

  it('names the host controls as unaffected in the worker-state wording', () => {
    expect(messages['zh-CN'].relayWorkerDisconnectedDesc).toContain('角色')
    expect(messages['en-US'].relayWorkerDisconnectedDesc).toContain(
      'roles still work',
    )
    expect(messages['zh-CN'].relayWorkerUnavailable).toContain(
      '本机角色不受影响',
    )
    expect(messages['en-US'].relayWorkerUnavailable).toContain(
      'host role is untouched',
    )
  })

  it('keeps the retired copy keys absent from both locales', () => {
    for (const locale of locales) {
      for (const key of [
        'configArchiveExportHelp',
        'configArchiveImportHelp',
        'relayHostLoginDesc',
      ]) {
        expect(Object.hasOwn(messages[locale], key)).toBe(false)
      }
    }
  })

  it('keeps the request sequence label, the endpoint key nickname copy and the extracted OpenAI module table in both locales', () => {
    // The badge renders the prompt-ferry request counter, so the label must not
    // read as a conversation-scoped sequence or as an OpenCode turn count.
    const zh = messages['zh-CN']
    const en = messages['en-US']
    expect(en.conversationSeq).toMatch(/request sequence/i)
    expect(en.conversationSeq).not.toMatch(/turn/i)
    expect(en.conversationSeq).not.toMatch(/conversation/i)
    expect(zh.conversationSeq).toBe('请求序号')
    expect(zh.conversationSeq).not.toBe('会话序号')
    // The same key in both locales, so the UI label survives the locale switch.
    expect((en as Record<string, string>).conversationSeq).toBe(
      en.conversationSeq,
    )

    // Usage lists show the nickname, so the placeholder mirrors the
    // server-side positional default in both locales.
    for (const key of [
      'apiKeyName',
      'apiKeyNameDefault',
      'apiKeyNameDuplicate',
      'endpointApiKeysHint',
    ]) {
      for (const locale of locales) {
        const table = messages[locale] as Record<string, string>
        expect(table[key].length, `${locale}.${key}`).toBeGreaterThan(0)
      }
    }
    expect(
      (messages['zh-CN'] as Record<string, string>).apiKeyNameDefault,
    ).toBe('key {index}')
    expect(
      (messages['en-US'] as Record<string, string>).apiKeyNameDefault,
    ).toBe('key {index}')

    // The extracted OpenAI module keeps flat keys and zh/en parity, and every
    // moved key stays reachable from the merged locale object.
    const zhKeys = Object.keys(endpointOpenAiMessages['zh-CN']).sort()
    const enKeys = Object.keys(endpointOpenAiMessages['en-US']).sort()
    expect(enKeys).toEqual(zhKeys)
    for (const locale of locales) {
      const merged = messages[locale] as Record<string, string>
      const module = endpointOpenAiMessages[locale] as Record<string, string>
      for (const key of zhKeys) {
        expect(merged[key], `${locale}.${key}`).toBe(module[key])
      }
    }
  })

  it('keeps the user-facing copy off the backend field names', () => {
    for (const locale of locales) {
      expect(messages[locale].ownerUser).not.toContain('owner_user_id')
      expect(messages[locale].userLoginPlaceholder).not.toContain('login_name')
    }
    expect(messages['zh-CN'].ownerUser).toBe('选择用户')
    expect(messages['en-US'].ownerUser).toBe('Select user')
    expect(messages['zh-CN'].providerGeneric).toBe('通用')
    expect(messages['en-US'].providerGeneric).toBe('Generic')
  })

  it('states the MCP connection consequence instead of the mechanism', () => {
    expect(messages['zh-CN'].bearerTokenHint).not.toContain('最少使用')
    expect(messages['en-US'].bearerTokenHint).not.toContain('least-used')
    for (const locale of locales) {
      expect(messages[locale].lifecyclePolicyHint).not.toContain(
        'server/discover',
      )
    }
  })

  it('names one display-time-zone consequence in both locales', () => {
    expect(messages['zh-CN'].displayTimeZone).toBe('显示时区')
    expect(messages['en-US'].displayTimeZone).toBe('Display time zone')
    expect(messages['zh-CN'].displayTimeZoneEffect).toBe(
      '仅影响时间显示，不改变统计周期。',
    )
    expect(messages['en-US'].displayTimeZoneEffect).toBe(
      'Affects how times are displayed only; it does not change the reporting period.',
    )
  })

  it('keeps the irreversible import warning in both locales', () => {
    expect(messages['zh-CN'].configArchiveImportConfirmBody).toContain(
      '无法撤销',
    )
    expect(messages['en-US'].configArchiveImportConfirmBody).toContain(
      'cannot be undone',
    )
  })
})

describe('user create dialog copy', () => {
  it('labels the login, display and initial-password fields in English', () => {
    renderWithLocale(
      'en-US',
      <CreateUserDialog
        form={emptyUserForm()}
        onChange={() => {}}
        onOpenChange={() => {}}
        onSubmit={() => {}}
        open
        pending={false}
      />,
    )

    expect(screen.getByLabelText('Login name')).toBeTruthy()
    expect(screen.getByLabelText('Display name')).toBeTruthy()
    expect(screen.getByLabelText('Initial password')).toBeTruthy()
    expect(
      screen.getByLabelText('Login name').getAttribute('placeholder'),
    ).toBe(messages['en-US'].userLoginPlaceholder)
  })

  it('labels the same fields in Chinese with the localized placeholder', () => {
    renderWithLocale(
      'zh-CN',
      <CreateUserDialog
        form={emptyUserForm()}
        onChange={() => {}}
        onOpenChange={() => {}}
        onSubmit={() => {}}
        open
        pending={false}
      />,
    )

    expect(screen.getByLabelText(messages['zh-CN'].loginName)).toBeTruthy()
    expect(screen.getByLabelText(messages['zh-CN'].displayName)).toBeTruthy()
    expect(
      screen.getByLabelText(messages['zh-CN'].userInitialPassword),
    ).toBeTruthy()
    expect(
      screen
        .getByLabelText(messages['zh-CN'].loginName)
        .getAttribute('placeholder'),
    ).toBe(messages['zh-CN'].userLoginPlaceholder)
  })
})

describe('display time zone copy', () => {
  it('shows the title and exactly one consequence line in English', () => {
    localStorage.setItem('prompt-ferry:locale', 'en-US')
    render(
      <LocaleProvider>
        <ThemeProvider>
          <SettingsGeneralTab />
        </ThemeProvider>
      </LocaleProvider>,
    )

    expect(screen.getAllByText('Display time zone').length).toBeGreaterThan(0)
    expect(
      screen.getAllByText(
        'Affects how times are displayed only; it does not change the reporting period.',
      ).length,
    ).toBe(1)
  })

  it('shows the title and exactly one consequence line in Chinese', () => {
    localStorage.setItem('prompt-ferry:locale', 'zh-CN')
    render(
      <LocaleProvider>
        <ThemeProvider>
          <SettingsGeneralTab />
        </ThemeProvider>
      </LocaleProvider>,
    )

    expect(screen.getAllByText('显示时区').length).toBeGreaterThan(0)
    expect(screen.getAllByText('仅影响时间显示，不改变统计周期。').length).toBe(
      1,
    )
  })
})
