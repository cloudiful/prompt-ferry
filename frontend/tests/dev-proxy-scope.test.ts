import { expect, test } from 'bun:test'

import config from '../vite.config'

test('the dev proxy forwards only the /api/ path boundary', () => {
  const proxy = config.server?.proxy

  expect(Object.keys(proxy ?? {})).toEqual(['/api/'])
  expect(proxy?.['/api/']).toBe('http://127.0.0.1:8789')
})
