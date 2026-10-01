import { expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import {
  THEME_MODE_STORAGE_KEY,
  readStorage,
  writeStorage,
} from '../src/storage'

const indexHtml = readFileSync(
  new URL('../index.html', import.meta.url),
  'utf8',
)

function inlineThemeScript(): string {
  const match = indexHtml.match(
    /<script id="theme-first-paint">([\s\S]*?)<\/script>/,
  )
  if (!match) {
    throw new Error('index.html is missing the theme-first-paint inline script')
  }
  return match[1]
}

interface DocumentElementStub {
  classList: {
    toggle: (name: string, force?: boolean) => boolean
    contains: (name: string) => boolean
  }
  style: { colorScheme: string }
}

function createDocumentElement(): DocumentElementStub {
  const classes = new Set<string>()
  return {
    classList: {
      toggle(name, force = !classes.has(name)) {
        if (force) classes.add(name)
        else classes.delete(name)
        return force
      },
      contains: (name) => classes.has(name),
    },
    style: { colorScheme: '' },
  }
}

function executeThemeInit(
  element: DocumentElementStub,
  storage: unknown,
): void {
  const execute = new Function(
    'document',
    'localStorage',
    inlineThemeScript(),
  ) as (document: unknown, localStorage: unknown) => void
  execute({ documentElement: element }, storage)
}

function runThemeInit(savedValue: string | null): DocumentElementStub {
  const element = createDocumentElement()
  const requestedKeys: string[] = []
  executeThemeInit(element, {
    getItem: (key) => {
      requestedKeys.push(key)
      return key === THEME_MODE_STORAGE_KEY ? savedValue : null
    },
  })
  expect(requestedKeys).toEqual([THEME_MODE_STORAGE_KEY])
  return element
}

test('persisted dark selection is applied before first paint', () => {
  const element = runThemeInit('dark')

  expect(element.classList.contains('dark')).toBe(true)
  expect(element.style.colorScheme).toBe('dark')
})

test('persisted light selection is applied before first paint', () => {
  const element = runThemeInit('light')

  expect(element.classList.contains('dark')).toBe(false)
  expect(element.style.colorScheme).toBe('light')
})

test('missing selection falls back to dark before first paint', () => {
  const element = runThemeInit(null)

  expect(element.classList.contains('dark')).toBe(true)
  expect(element.style.colorScheme).toBe('dark')
})

test('unrecognized selection falls back to dark', () => {
  const element = runThemeInit('sepia')

  expect(element.classList.contains('dark')).toBe(true)
  expect(element.style.colorScheme).toBe('dark')
})

test('blocked storage still applies the dark default', () => {
  const element = createDocumentElement()

  executeThemeInit(element, {
    getItem() {
      throw new Error('storage blocked')
    },
  })

  expect(element.classList.contains('dark')).toBe(true)
  expect(element.style.colorScheme).toBe('dark')
})

test('the init is a synchronous head script rendered before the module entry', () => {
  const openTag = indexHtml.match(/<script[^>]*id="theme-first-paint"[^>]*>/)
  expect(openTag).not.toBeNull()
  expect(openTag?.[0]).not.toContain('type="module"')

  const inlineIndex = indexHtml.indexOf('id="theme-first-paint"')
  const headEnd = indexHtml.indexOf('</head>')
  const moduleIndex = indexHtml.indexOf('/src/main.ts')

  expect(inlineIndex).toBeGreaterThan(-1)
  expect(headEnd).toBeGreaterThan(inlineIndex)
  expect(moduleIndex).toBeGreaterThan(inlineIndex)
})

// --- Independent verification additions (tester, phase P4) ---

function themeScriptOpenTag(): string {
  const match = indexHtml.match(/<script[^>]*id="theme-first-paint"[^>]*>/)
  if (!match) {
    throw new Error('index.html is missing the theme-first-paint script tag')
  }
  return match[0]
}

test('the inline init uses the exact key the app persists', () => {
  expect(THEME_MODE_STORAGE_KEY).toBe('prompt-ferry:theme-mode')
  expect(inlineThemeScript()).toContain(
    "localStorage.getItem('prompt-ferry:theme-mode')",
  )
})

test('the inline init has no deferred or asynchronous execution path', () => {
  const openTag = themeScriptOpenTag()
  expect(openTag).not.toContain('src=')
  expect(openTag).not.toContain('defer')
  expect(openTag).not.toContain('async')
  expect(openTag).not.toContain('type=')

  const body = inlineThemeScript()
  for (const deferred of [
    'setTimeout',
    'setInterval',
    'requestAnimationFrame',
    'queueMicrotask',
    'addEventListener',
    'DOMContentLoaded',
    'await ',
    'import(',
  ]) {
    expect(body).not.toContain(deferred)
  }
})

test('there is exactly one first-paint theme script', () => {
  const occurrences = indexHtml.match(/id="theme-first-paint"/g) ?? []
  expect(occurrences).toHaveLength(1)
})

test('inline init normalization matches the app for every stored value', () => {
  const cases: Array<[stored: string | null, expected: 'dark' | 'light']> = [
    ['dark', 'dark'],
    ['light', 'light'],
    [null, 'dark'],
    ['', 'dark'],
    ['Dark', 'dark'],
    ['LIGHT', 'dark'],
    ['light ', 'dark'],
    ['true', 'dark'],
    ['0', 'dark'],
  ]

  for (const [stored, expected] of cases) {
    const element = runThemeInit(stored)
    expect(element.classList.contains('dark')).toBe(expected === 'dark')
    expect(element.style.colorScheme).toBe(expected)
  }
})

test('inline init removes a stale dark class when light is persisted', () => {
  const element = createDocumentElement()
  element.classList.toggle('dark', true)

  executeThemeInit(element, { getItem: () => 'light' })

  expect(element.classList.contains('dark')).toBe(false)
  expect(element.style.colorScheme).toBe('light')
})

test('re-running the inline init is idempotent', () => {
  const element = createDocumentElement()
  executeThemeInit(element, { getItem: () => 'light' })
  executeThemeInit(element, { getItem: () => 'light' })

  expect(element.classList.contains('dark')).toBe(false)
  expect(element.style.colorScheme).toBe('light')
})

test('inline init never rewrites the persisted preference', () => {
  const element = createDocumentElement()
  const writes: Array<[string, string]> = []
  const storage = {
    getItem: (): string | null => 'light',
    setItem: (key: string, value: string): void => {
      writes.push([key, value])
    },
  }

  executeThemeInit(element, storage)

  expect(writes).toEqual([])
})

test('missing localStorage still applies the dark default', () => {
  const element = createDocumentElement()

  executeThemeInit(element, undefined)

  expect(element.classList.contains('dark')).toBe(true)
  expect(element.style.colorScheme).toBe('dark')
})

// --- P1: LibreWolf/Firefox storage failures and inline/runtime parity ---

const baseCss = readFileSync(
  new URL('../src/styles/base.css', import.meta.url),
  'utf8',
)

type Globals = Record<string, unknown>

/** Runs `body` with `overrides` installed as globals, then restores them. */
async function withGlobals<T>(
  overrides: Globals,
  body: () => T | Promise<T>,
): Promise<T> {
  const globals = globalThis as Globals
  const saved = new Map<string, unknown>()
  for (const [key, value] of Object.entries(overrides)) {
    saved.set(key, globals[key])
    if (value === undefined) delete globals[key]
    else globals[key] = value
  }
  try {
    return await body()
  } finally {
    for (const [key, value] of saved) {
      if (value === undefined) delete globals[key]
      else globals[key] = value
    }
  }
}

let loadCounter = 0
/** A fresh module instance, so every case runs its own startup path. */
function loadThemeModule(): Promise<typeof import('../src/theme/appTheme')> {
  loadCounter += 1
  return import(`../src/theme/appTheme?case=${loadCounter}`)
}

function documentStub(element: DocumentElementStub): Globals {
  return { document: { documentElement: element } }
}

test('a blocked store never breaks a read or a write', async () => {
  const blocked = {
    getItem() {
      throw new Error('storage blocked')
    },
    setItem() {
      throw new Error('quota exceeded')
    },
  }

  for (const localStorage of [undefined, blocked]) {
    await withGlobals({ localStorage }, () => {
      expect(readStorage(THEME_MODE_STORAGE_KEY)).toBeNull()
      expect(() => writeStorage(THEME_MODE_STORAGE_KEY, 'light')).not.toThrow()
    })
  }
})

test('the theme module starts dark and writes nothing when storage is blocked', async () => {
  const writes: Array<[string, string]> = []

  const theme = await withGlobals(
    {
      localStorage: {
        getItem() {
          throw new Error('storage blocked')
        },
        setItem: (key: string, value: string) => writes.push([key, value]),
      },
    },
    loadThemeModule,
  )

  expect(theme.themeMode.value).toBe('dark')
  expect(writes).toEqual([])
})

test('the theme module starts from the persisted mode without rewriting it', async () => {
  const writes: Array<[string, string]> = []

  const theme = await withGlobals(
    {
      localStorage: {
        getItem: (key: string) =>
          key === THEME_MODE_STORAGE_KEY ? 'light' : null,
        setItem: (key: string, value: string) => writes.push([key, value]),
      },
    },
    loadThemeModule,
  )

  expect(theme.themeMode.value).toBe('light')
  // Module initialization must not undo a correct first paint by persisting.
  expect(writes).toEqual([])

  const element = createDocumentElement()
  await withGlobals(documentStub(element), () => theme.initTheme())

  expect(element.classList.contains('dark')).toBe(false)
  expect(element.style.colorScheme).toBe('light')
})

test('the inline init and the runtime resolve every stored value identically', async () => {
  const theme = await loadThemeModule()
  const cases: Array<string | null> = [
    'dark',
    'light',
    null,
    '',
    'Dark',
    'LIGHT',
    'light ',
    'true',
    '0',
  ]

  for (const stored of cases) {
    const inline = runThemeInit(stored)
    const runtime = createDocumentElement()

    await withGlobals(documentStub(runtime), () =>
      theme.applyThemeToDocument(theme.resolveThemeMode(stored)),
    )

    expect(runtime.classList.contains('dark')).toBe(
      inline.classList.contains('dark'),
    )
    expect(runtime.style.colorScheme).toBe(inline.style.colorScheme)
  }
})

test('the served HTML carries the dark default on the root before any script', () => {
  expect(indexHtml.match(/<html[^>]*>/)?.[0]).toMatch(
    /class="[^"]*\bdark\b[^"]*"/,
  )
  expect(baseCss.match(/:root\s*\{([^}]*)\}/)?.[1]).toMatch(
    /color-scheme:\s*dark/,
  )
})
