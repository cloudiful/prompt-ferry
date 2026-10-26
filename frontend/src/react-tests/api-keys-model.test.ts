import { describe, expect, it } from 'vitest'
import type { ClientKey, User } from '@/generated/admin-api'
import {
  CLIENT_KEY_PREFIX,
  SELF_USER_ID,
  clientKeyActionKey,
  clientKeyCreateKey,
  createApiKeysWorkspaceView,
} from '@/models/api-keys'
import {
  normalizePageSize,
  pageChange,
  pageSizeStorageKey,
  STANDARD_PAGE_SIZE_OPTIONS,
} from '@/models/pagination'

const key: ClientKey = {
  created_at: '2026-10-23T12:00:00.000Z',
  enabled: true,
  key_id: 'key-1',
  key_prefix: 'pf_live_',
  label: 'primary',
  last_used_at: null,
  secret: null,
  user_id: 1,
}

const user: User = {
  created_at: '2026-10-16T12:00:00.000Z',
  display_name: 'Fixture Operator',
  is_active: true,
  is_admin: false,
  login_name: 'operator',
  updated_at: '2026-10-23T12:00:00.000Z',
  user_id: 2,
}

describe('client-key model helpers', () => {
  it('keeps the self owner and exact shared write-key grammar', () => {
    expect(SELF_USER_ID).toBe(0)
    expect(CLIENT_KEY_PREFIX).toBe('clientkey')
    expect(clientKeyActionKey('uuid-1')).toBe('clientkey:uuid-1')
    expect(clientKeyCreateKey(0)).toBe('clientkey:create:0')
    expect(clientKeyCreateKey(2)).toBe('clientkey:create:2')
  })

  it('preserves the key view fields and exposes only actual returned secrets', () => {
    const view = createApiKeysWorkspaceView({
      keys: [key],
      selectedUser: user,
      visibleKeySecrets: { 'key-1': true },
    })

    expect(view.has_keys).toBe(true)
    expect(view.selected_user).toBe(user)
    expect(view.key_items).toEqual([
      {
        key,
        key_id: 'key-1',
        key_prefix: 'pf_live_',
        label: 'primary',
        enabled: true,
        secret: '',
        visible_secret: true,
      },
    ])
    expect(
      createApiKeysWorkspaceView({
        keys: [],
        selectedUser: null,
        visibleKeySecrets: {},
      }).has_keys,
    ).toBe(false)
  })
})

describe('pure pagination helpers', () => {
  it('normalizes persisted sizes and retains the shared page-size keys', () => {
    expect(STANDARD_PAGE_SIZE_OPTIONS).toEqual([10, 20, 50])
    expect(normalizePageSize('20', 10, STANDARD_PAGE_SIZE_OPTIONS)).toBe(20)
    expect(normalizePageSize('99', 10, STANDARD_PAGE_SIZE_OPTIONS)).toBe(10)
    expect(normalizePageSize(null, 10, STANDARD_PAGE_SIZE_OPTIONS)).toBe(10)
    expect(pageSizeStorageKey('api-keys')).toBe(
      'prompt-ferry:table-page-size:api-keys',
    )
  })

  it('converts one-based page changes into the legacy first/rows shape', () => {
    expect(pageChange(3, 10)).toEqual({ first: 20, page: 2, rows: 10 })
    expect(pageChange(0, 20)).toEqual({ first: 0, page: 0, rows: 20 })
  })
})
