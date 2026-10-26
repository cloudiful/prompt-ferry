import { describe, expect, it } from 'vitest'
import { appRoutes, navSections } from '@/lib/nav'
import {
  REDIRECTS,
  RETAINED_LEGACY_MODULES,
  ROUTES,
  classifyLegacyTest,
  type LegacyTestDisposition,
} from '../../e2e/parity.matrix'
import APP_SOURCE from '../App.tsx?raw'
import NAV_SOURCE from '../lib/nav.ts?raw'

const ALLOWED_DISPOSITIONS: LegacyTestDisposition[] = [
  'preserved-framework-neutral',
  'preserved-form-mapper',
  'pending-react-replacement',
  'retire-source-string',
  'support-fixture',
]

type RouteTag = { path: string | null; to: string | null }
type RedirectContract = { id: string; from: string; to: string }

function readAttribute(segment: string, name: string): string | null {
  const match = new RegExp(
    `\\b${name}=(?:"([^"]*)"|'([^']*)'|\\{([^}]*)\\})`,
  ).exec(segment)
  if (!match) return null
  return match[1] ?? match[2] ?? match[3] ?? null
}

/** Splits the app source into one segment per `<Route>` element. */
function parseRouteTags(source: string): RouteTag[] {
  return source
    .split('<Route')
    .slice(1)
    .map((segment) => ({
      path: readAttribute(segment, 'path'),
      to: readAttribute(segment, 'to'),
    }))
}

/** Reports every redirect whose source path does not reach its target. */
function redirectMismatches(
  tags: RouteTag[],
  redirects: RedirectContract[],
): string[] {
  const problems: string[] = []
  for (const redirect of redirects) {
    if (redirect.from.includes('<')) {
      const catchAll = tags.filter((tag) => tag.path === '*')
      if (
        catchAll.length === 0 ||
        catchAll.some((tag) => tag.to !== redirect.to)
      ) {
        problems.push(redirect.id)
      }
      continue
    }
    const tag = tags.find((entry) => entry.path === redirect.from)
    if (!tag || tag.to !== redirect.to) problems.push(redirect.id)
  }
  return problems
}

function missingRouteIds(matrixIds: string[], navIds: string[]): string[] {
  return matrixIds.filter((id) => !navIds.includes(id))
}

function extraRouteIds(matrixIds: string[], navIds: string[]): string[] {
  return navIds.filter((id) => !matrixIds.includes(id))
}

function unclassifiedModules(
  modules: { file: string; disposition: string }[],
): string[] {
  return modules
    .filter(
      (module) =>
        !ALLOWED_DISPOSITIONS.includes(
          module.disposition as LegacyTestDisposition,
        ),
    )
    .map((module) => module.file)
}

const navRouteIds = appRoutes.map((route) => route.id)
const navRoutePaths = appRoutes.map((route) => route.path)
const livePaths = new Set([...navRoutePaths, '/login'])
const matrixPaths = ROUTES.map((row) => row.route)
const concreteRedirectSources = REDIRECTS.filter(
  (redirect) => !redirect.from.includes('<'),
).map((redirect) => redirect.from)

describe('React parity routes', () => {
  it('routes exactly the matrix route set', () => {
    expect(livePaths.size).toBe(22)
    expect([...livePaths].sort()).toEqual([...matrixPaths].sort())
    const navMatrixIds = ROUTES.map((row) => row.id).filter(
      (id) => id !== 'R-LOGIN',
    )
    expect(missingRouteIds(navMatrixIds, navRouteIds)).toEqual([])
    expect(extraRouteIds(navMatrixIds, navRouteIds)).toEqual([])
    expect(
      parseRouteTags(APP_SOURCE).some((tag) => tag.path === '/login'),
    ).toBe(true)
  })

  it('keeps the nav ids, paths and sections aligned with the matrix rows', () => {
    expect(new Set(navRouteIds).size).toBe(navRouteIds.length)
    expect(new Set(navRoutePaths).size).toBe(navRoutePaths.length)
    const sectionOfRoute = new Map<string, string>()
    for (const section of navSections) {
      for (const route of section.routes)
        sectionOfRoute.set(route.id, section.id)
    }
    for (const row of ROUTES) {
      if (row.id === 'R-LOGIN') continue
      const route = appRoutes.find((entry) => entry.id === row.id)
      expect(route, `missing React route for ${row.id}`).toBeDefined()
      expect(route?.path).toBe(row.route)
      expect(route?.adminOnly).toBe(row.scope === 'admin')
      expect(sectionOfRoute.get(row.id)).toBe(row.navSection)
    }
    for (const section of navSections) {
      expect(
        ROUTES.some((row) => row.navSection === section.id),
        `nav section ${section.id} has no matrix row`,
      ).toBe(true)
    }
  })

  it('redirects every concrete source to its matrix target', () => {
    const tags = parseRouteTags(APP_SOURCE)
    expect(redirectMismatches(tags, REDIRECTS)).toEqual([])
    expect(tags.find((tag) => tag.path === '/')).toEqual({
      path: '/',
      to: '/api-keys',
    })
    expect(tags.filter((tag) => tag.path === '*')).not.toHaveLength(0)
  })

  it('rejects a redirect whose target moved to another route', () => {
    const rootIndex = APP_SOURCE.indexOf('path="/"')
    const targetIndex = APP_SOURCE.lastIndexOf('to="', rootIndex)
    expect(targetIndex).toBeGreaterThan(-1)
    const valueStart = targetIndex + 'to="'.length
    const valueEnd = APP_SOURCE.indexOf('"', valueStart)
    const swapped = `${APP_SOURCE.slice(0, valueStart)}/settings/general${APP_SOURCE.slice(valueEnd)}`
    expect(swapped).not.toBe(APP_SOURCE)
    expect(redirectMismatches(parseRouteTags(swapped), REDIRECTS)).toContain(
      'RD-ROOT',
    )
  })

  it('rejects a missing and an extra route', () => {
    expect(missingRouteIds(['R-API-KEYS', 'R-GONE'], navRouteIds)).toEqual([
      'R-GONE',
    ])
    expect(extraRouteIds(['R-API-KEYS'], ['R-API-KEYS', 'R-EXTRA'])).toEqual([
      'R-EXTRA',
    ])
  })

  it('rejects an unclassified legacy module name', () => {
    expect(
      unclassifiedModules([
        { file: 'new-suite.test.ts', disposition: 'unknown-disposition' },
      ]),
    ).toEqual(['new-suite.test.ts'])
    expect(unclassifiedModules(RETAINED_LEGACY_MODULES)).toEqual([])
  })

  it('retains the legacy module inventory with its dispositions', () => {
    const names = RETAINED_LEGACY_MODULES.map((module) => module.file)
    expect(names.length).toBe(134)
    expect(new Set(names).size).toBe(names.length)
    const inventory = new Set(names)
    for (const row of ROUTES) {
      for (const file of row.legacyTests) {
        expect(inventory.has(file), `${row.id} references ${file}`).toBe(true)
        expect(ALLOWED_DISPOSITIONS).toContain(classifyLegacyTest(file))
      }
    }
    for (const module of RETAINED_LEGACY_MODULES) {
      expect(ALLOWED_DISPOSITIONS).toContain(module.disposition)
    }
  })

  it('keeps every nav section root routable or redirected', () => {
    for (const section of navSections) {
      const root = `/${section.id}`
      if (livePaths.has(root)) continue
      expect(concreteRedirectSources).toContain(root)
    }
  })

  it('keeps the React nav free of Vue imports', () => {
    expect(NAV_SOURCE).not.toMatch(/from\s+['"]vue['"]/)
  })
})
