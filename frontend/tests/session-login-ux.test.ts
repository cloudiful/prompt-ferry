import { expect, test } from 'bun:test'

import { authLogin, authMe, freshSessions } from './session-login.fixtures'
import { i18n } from '../src/i18n/plugin'

const unauthorized = {
  error: { code: 'unauthorized', message: 'invalid login' },
}

test('an anonymous session probe is not rendered as a load failure', async () => {
  const { worker } = freshSessions()
  authMe.mockRejectedValueOnce(unauthorized)

  expect(await worker.bootstrapAuth()).toBe(false)
  expect(worker.me).toBeNull()
  expect(worker.error).toBe('')
  expect(worker.bootstrapped).toBe(true)
  expect(worker.busy).toBe(false)
})

test('real session probe failures stay visible', async () => {
  const { worker } = freshSessions()
  authMe.mockRejectedValueOnce({
    error: { code: 'internal', message: 'session store unavailable' },
  })

  await worker.bootstrapAuth()

  expect(worker.me).toBeNull()
  expect(worker.error).toContain('session store unavailable')
})

test('network failures during bootstrap are not swallowed', async () => {
  const { worker } = freshSessions()
  authMe.mockRejectedValueOnce(new Error('Failed to fetch'))

  await worker.bootstrapAuth()

  expect(worker.error).toBe('Failed to fetch')
})

test('bad credentials show a localized error and release the gate', async () => {
  const { worker } = freshSessions()
  authLogin.mockRejectedValueOnce(unauthorized)

  await expect(
    worker.login({ login_name: 'admin', password: 'bad-secret' }),
  ).rejects.toBeDefined()

  expect(worker.error).toBe(i18n.global.t('invalidCredentials'))
  expect(worker.busy).toBe(false)
})

test('the password visibility control names a password, not a secret', async () => {
  const login = await Bun.file(
    new URL('../src/pages/LoginPage.vue', import.meta.url),
  ).text()

  expect(login).toContain("t('showPassword')")
  expect(login).toContain("t('hidePassword')")
  expect(login).not.toContain("t('showSecret')")
  expect(login).not.toContain("t('hideSecret')")
})
