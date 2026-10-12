import { expect, test } from 'bun:test'
import { compileScript, compileTemplate, parse } from 'vue/compiler-sfc'
import {
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
  deferred,
  freshSessions,
  relayAuthLogin,
  relayAuthMe,
} from './session-login.fixtures'

async function settle(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
}

test('Worker login snapshots credentials and remains busy through bootstrap and bridge refresh', async () => {
  const { worker } = freshSessions()
  const loginRequest = deferred()
  const bootstrapRequest = deferred()
  const bridgeRequest = deferred()
  authLogin.mockImplementationOnce(() => loginRequest.promise)
  authMe.mockImplementationOnce(() => bootstrapRequest.promise)
  bridgeStatus.mockImplementationOnce(() => bridgeRequest.promise)
  const payload = { login_name: 'admin', password: 'first-secret' }

  const loggingIn = worker.login(payload)
  payload.password = 'changed-secret'
  await settle()
  expect(worker.busy).toBe(true)
  expect(authLogin).toHaveBeenCalledTimes(1)
  expect((authLogin.mock.calls[0]?.[0] as { body: unknown }).body).toEqual({
    login_name: 'admin',
    password: 'first-secret',
  })
  expect(
    await worker.login({ login_name: 'admin', password: 'second' }),
  ).toEqual({ executed: false })
  expect(authLogin).toHaveBeenCalledTimes(1)

  loginRequest.resolve({ data: undefined })
  await settle()
  expect(authMe).toHaveBeenCalledTimes(1)
  expect(bridgeStatus).not.toHaveBeenCalled()
  expect(worker.busy).toBe(true)

  bootstrapRequest.resolve({
    data: {
      display_name: 'Administrator',
      is_admin: true,
      login_name: 'admin',
      user_id: 1,
    },
  })
  await settle()
  expect(bridgeStatus).toHaveBeenCalledTimes(1)
  expect(worker.busy).toBe(true)

  bridgeRequest.resolve({
    data: {
      configured_relays: 0,
      connected_relays: 0,
      relays: [],
      snapshot_version: 2,
    },
  })
  await expect(loggingIn).resolves.toMatchObject({ executed: true })
  expect(worker.busy).toBe(false)
  expect(worker.me?.login_name).toBe('admin')
})

test('failed login retains its error, releases the gate, and allows retry', async () => {
  const { worker } = freshSessions()
  authLogin.mockRejectedValueOnce(new Error('invalid credentials'))

  await expect(
    worker.login({ login_name: 'admin', password: 'bad-secret' }),
  ).rejects.toThrow('invalid credentials')
  expect(worker.error).toBe('invalid credentials')
  expect(worker.busy).toBe(false)

  await expect(
    worker.login({ login_name: 'admin', password: 'good-secret' }),
  ).resolves.toMatchObject({ executed: true })
  expect(worker.error).toBe('')
  expect(worker.busy).toBe(false)
})

test('Worker logout shares the login gate until session cleanup completes', async () => {
  const { worker } = freshSessions()
  worker.me = {
    display_name: 'Administrator',
    is_admin: true,
    login_name: 'admin',
    user_id: 1,
  }
  const request = deferred()
  authLogout.mockImplementationOnce(() => request.promise)

  const loggingOut = worker.logout()
  await settle()
  expect(worker.busy).toBe(true)
  expect(await worker.logout()).toEqual({ executed: false })
  expect(
    await worker.login({ login_name: 'admin', password: 'secret' }),
  ).toEqual({ executed: false })
  expect(authLogin).not.toHaveBeenCalled()

  request.resolve({ data: undefined })
  await expect(loggingOut).resolves.toEqual({
    executed: true,
    value: undefined,
  })
  expect(worker.busy).toBe(false)
  expect(worker.me).toBeNull()
  expect(worker.bootstrapped).toBe(false)
})

test('Relay and Worker authentication claims remain independent', async () => {
  const { worker, relay } = freshSessions()
  const workerLogin = deferred()
  const relayLogin = deferred()
  authLogin.mockImplementationOnce(() => workerLogin.promise)
  relayAuthLogin.mockImplementationOnce(() => relayLogin.promise)

  const workerRequest = worker.login({
    login_name: 'admin',
    password: 'worker-secret',
  })
  const relayRequest = relay.login('relay-secret')
  await settle()
  expect(worker.busy).toBe(true)
  expect(relay.busy).toBe(true)
  expect(await relay.login('duplicate-relay-secret')).toEqual({
    executed: false,
  })
  expect(
    await worker.login({ login_name: 'admin', password: 'duplicate' }),
  ).toEqual({ executed: false })
  expect((relayAuthLogin.mock.calls[0]?.[0] as { body: unknown }).body).toEqual(
    {
      admin_token: 'relay-secret',
    },
  )

  workerLogin.resolve({ data: undefined })
  relayLogin.resolve({ data: undefined })
  await Promise.all([workerRequest, relayRequest])
  expect(worker.busy).toBe(false)
  expect(relay.busy).toBe(false)
  expect(relay.authenticated).toBe(true)
  expect(relayAuthMe).toHaveBeenCalledTimes(1)
})

test('a bootstrap read cannot release an active Relay login claim', async () => {
  const { relay } = freshSessions()
  const loginRequest = deferred()
  const meRequest = deferred()
  relayAuthLogin.mockImplementationOnce(() => loginRequest.promise)
  relayAuthMe.mockImplementationOnce(() => meRequest.promise)

  const loggingIn = relay.login('relay-secret')
  await settle()
  expect(relay.busy).toBe(true)
  loginRequest.resolve({ data: undefined })
  await settle()
  expect(relay.busy).toBe(true)

  meRequest.resolve({ data: { authenticated: true } })
  await expect(loggingIn).resolves.toMatchObject({ executed: true })
  expect(relay.busy).toBe(false)
})

test('login and logout views bind completion and pending feedback to guarded outcomes', async () => {
  const paths = [
    '../src/pages/LoginPage.vue',
    '../src/pages/RelayHostPage.vue',
    '../src/layouts/AppShell.vue',
  ]
  const sources = await Promise.all(
    paths.map((path) => Bun.file(new URL(path, import.meta.url)).text()),
  )
  const login = parse(sources[0]!, { filename: 'LoginPage.vue' }).descriptor
  const relay = parse(sources[1]!, { filename: 'RelayHostPage.vue' }).descriptor
  const shell = parse(sources[2]!, { filename: 'AppShell.vue' }).descriptor
  const loginScript = compileScript(login, { id: 'login-page' }).content
  const relayScript = compileScript(relay, { id: 'relay-host-page' }).content
  const shellScript = compileScript(shell, { id: 'app-shell' }).content
  const loginTemplate = compileTemplate({
    source: login.template!.content,
    filename: 'LoginPage.vue',
    id: 'login-page',
  }).code
  const relayTemplate = compileTemplate({
    source: relay.template!.content,
    filename: 'RelayHostPage.vue',
    id: 'relay-host-page',
  }).code
  const shellTemplate = compileTemplate({
    source: shell.template!.content,
    filename: 'AppShell.vue',
    id: 'app-shell',
  }).code

  expect(loginScript).toContain('if (!result.executed) return')
  expect(loginScript).toContain('router.replace(redirect)')
  expect(loginTemplate).toContain('disabled: _ctx.busy')
  expect(loginTemplate).toContain('loading: _ctx.busy')
  expect(relayScript).toContain('if (!result.executed) return')
  expect(relayScript).toContain('tokenInput.value.trim() === token')
  expect(relayTemplate).toContain('relaySession.busy')
  expect(shellScript).toContain('if (result.executed)')
  expect(shellTemplate).toContain('session.busy')
  expect(shellTemplate).toContain('relaySession.busy')
})
