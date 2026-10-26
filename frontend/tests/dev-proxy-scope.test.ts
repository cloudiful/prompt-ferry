import { expect, test } from 'bun:test'

import config from '../vite.config'

test('the dev proxy forwards only the /api/ path boundary', () => {
  const proxy = config.server?.proxy

  expect(Object.keys(proxy ?? {})).toEqual(['/api/'])
  expect(proxy?.['/api/']).toBe('http://127.0.0.1:8789')
})

test('the dev watcher ignores the focused browser harness output directory', () => {
  const ignored = config.server?.watch?.ignored
  const patterns = (
    Array.isArray(ignored) ? ignored : ignored ? [ignored] : []
  ).filter((pattern): pattern is string => typeof pattern === 'string')

  expect(patterns).toContain('**/.playwright-cli/**')
  for (const pattern of patterns) {
    expect(pattern).not.toBe('*')
    expect(pattern).not.toBe('**')
    expect(pattern).not.toBe('**/*')
    expect(pattern).toContain('/')
  }
})
