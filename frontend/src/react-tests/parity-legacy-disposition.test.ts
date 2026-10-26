import { describe, expect, it } from 'vitest'
import {
  LEGACY_EVIDENCE_KINDS,
  LEGACY_MODULE_DISPOSITIONS,
  LEGACY_MODULE_STATES,
  LEGACY_SUPPORT_FILES,
  LEGACY_TEST_DISPOSITIONS,
  RETAINED_LEGACY_MODULE_NAMES,
  legacyModuleSourcePath,
  validateLegacyModuleDispositions,
  type LegacyModuleDisposition,
} from '../../e2e/parity.matrix'
import QUOTA_CORE_SOURCE from './quota-core.test.ts?raw'

const repositoryFiles = new Set([
  ...Object.keys(import.meta.glob('/tests/**/*.{ts,tsx}')),
  ...Object.keys(import.meta.glob('/src/**/*.{ts,tsx}')),
  ...Object.keys(import.meta.glob('/e2e/**/*.{ts,tsx,json}')),
])

// The validator reads a cited React test title out of the cited file, so the
// repository check has to see the real file content and not only its path.
const repositorySources: Record<string, string> = {
  ...import.meta.glob('/tests/**/*.{ts,tsx}', {
    query: '?raw',
    import: 'default',
    eager: true,
  }),
  ...import.meta.glob('/src/**/*.{ts,tsx}', {
    query: '?raw',
    import: 'default',
    eager: true,
  }),
}

const repositoryChecks = {
  exists: (path: string) => repositoryFiles.has(`/${path}`),
  read: (path: string) => repositorySources[`/${path}`] ?? '',
}

const fixtureFiles = new Map<string, string>([
  ['tests/legacy.test.ts', 'export const legacy = 1\n'],
  ['src/react-tests/quota-core.test.ts', QUOTA_CORE_SOURCE],
])

const fixtureChecks = {
  exists: (path: string) => fixtureFiles.has(path),
  read: (path: string) => fixtureFiles.get(path) ?? '',
}

function preservedFixture(
  file = 'tests/legacy.test.ts',
): LegacyModuleDisposition {
  return {
    file,
    state: 'preserved',
    disposition: 'preserved-framework-neutral',
    evidence: [{ kind: 'kept-suite', path: file }],
  }
}

function rejects(problems: string[], fragment: string): void {
  expect(
    problems.some((problem) => problem.includes(fragment)),
    `expected a problem containing "${fragment}", got ${JSON.stringify(problems)}`,
  ).toBe(true)
}

describe('legacy module dispositions', () => {
  it('records one disposition for every retained legacy module', () => {
    expect(LEGACY_MODULE_DISPOSITIONS.length).toBe(134)
    const names = LEGACY_MODULE_DISPOSITIONS.map((entry) => entry.file)
    expect(new Set(names).size).toBe(names.length)
    expect([...names].sort()).toEqual([...RETAINED_LEGACY_MODULE_NAMES].sort())
    for (const entry of LEGACY_MODULE_DISPOSITIONS) {
      expect(LEGACY_MODULE_STATES).toContain(entry.state)
      expect(LEGACY_TEST_DISPOSITIONS).toContain(entry.disposition)
      expect(entry.evidence.length).toBeGreaterThan(0)
      for (const item of entry.evidence) {
        expect(LEGACY_EVIDENCE_KINDS).toContain(item.kind)
      }
    }
    expect(
      validateLegacyModuleDispositions(
        LEGACY_MODULE_DISPOSITIONS,
        RETAINED_LEGACY_MODULE_NAMES,
        repositoryChecks,
      ),
    ).toEqual([])
  })

  it('keeps the preserved set to the retained suites and the helper states explicit', () => {
    // The preserved set is exactly the modules that stay on disk: the two
    // retained native Bun suites and the four active form-mapper suites.
    const preserved = LEGACY_MODULE_DISPOSITIONS.filter(
      (entry) => entry.state === 'preserved',
    )
      .map((entry) => legacyModuleSourcePath(entry.file))
      .sort()
    expect(preserved).toEqual([
      'src/admin-mappers/forms/config-archive.test.ts',
      'src/admin-mappers/forms/endpoint.test.ts',
      'src/admin-mappers/forms/model-route.test.ts',
      'src/admin-mappers/forms/relay-host.test.ts',
      'tests/dev-proxy-scope.test.ts',
      'tests/theme-first-paint.test.ts',
    ])
    for (const path of preserved) {
      expect(repositoryChecks.exists(path)).toBe(true)
    }

    // The helper states are explicit: one replaced fixture and two obsolete
    // legacy bootstraps whose only importers are the retiring suites.
    expect(LEGACY_SUPPORT_FILES.length).toBe(3)
    expect(
      LEGACY_SUPPORT_FILES.filter((entry) => entry.state === 'replaced'),
    ).toHaveLength(1)
    expect(
      LEGACY_SUPPORT_FILES.filter((entry) => entry.state === 'obsolete'),
    ).toHaveLength(2)
    for (const entry of LEGACY_SUPPORT_FILES) {
      expect(entry.disposition).toBe('support-fixture')
      if (entry.state === 'obsolete') {
        const reason = entry.evidence.find((item) => item.kind === 'none')
        expect(
          reason && 'reason' in reason ? reason.reason.trim().length : 0,
        ).toBeGreaterThan(0)
      }
    }

    // The aggregate disposition of the 134 modules and the 3 helpers.
    const counts = new Map<string, number>()
    for (const entry of [
      ...LEGACY_MODULE_DISPOSITIONS,
      ...LEGACY_SUPPORT_FILES,
    ]) {
      counts.set(entry.state, (counts.get(entry.state) ?? 0) + 1)
    }
    expect(Object.fromEntries(counts)).toEqual({
      obsolete: 2,
      ported: 19,
      preserved: 6,
      replaced: 110,
    })
    expect(LEGACY_MODULE_DISPOSITIONS.length).toBe(134)
    expect(
      LEGACY_MODULE_DISPOSITIONS.length + LEGACY_SUPPORT_FILES.length,
    ).toBe(137)

    expect(
      validateLegacyModuleDispositions(
        LEGACY_SUPPORT_FILES,
        LEGACY_SUPPORT_FILES.map((entry) => entry.file),
        repositoryChecks,
      ),
    ).toEqual([])
  })

  it('rejects a missing disposition entry', () => {
    const problems = validateLegacyModuleDispositions(
      [],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'missing disposition for tests/legacy.test.ts')
  })

  it('rejects a duplicate disposition entry', () => {
    const problems = validateLegacyModuleDispositions(
      [preservedFixture(), preservedFixture()],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'duplicate disposition for tests/legacy.test.ts')
  })

  it('rejects an unknown legacy module name', () => {
    const problems = validateLegacyModuleDispositions(
      [preservedFixture(), preservedFixture('tests/unknown.test.ts')],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'unknown legacy module tests/unknown.test.ts')
  })

  it('rejects an unknown classifier disposition', () => {
    const entry = {
      ...preservedFixture(),
      disposition: 'invented-disposition' as never,
    }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'unknown disposition invented-disposition')
  })

  it('rejects an unknown state', () => {
    const entry = { ...preservedFixture(), state: 'invented' as never }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'unknown state invented')
  })

  it('rejects empty evidence', () => {
    const problems = validateLegacyModuleDispositions(
      [{ ...preservedFixture(), evidence: [] }],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'missing evidence for tests/legacy.test.ts')
  })

  it('rejects a dangling evidence path', () => {
    const entry = {
      ...preservedFixture(),
      evidence: [{ kind: 'kept-suite' as const, path: 'tests/gone.test.ts' }],
    }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'dangling evidence path tests/gone.test.ts')
  })

  it('rejects an evidence path outside the project tree', () => {
    const entry = {
      ...preservedFixture(),
      evidence: [
        { kind: 'kept-suite' as const, path: '../outside/legacy.test.ts' },
      ],
    }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'invalid evidence path ../outside/legacy.test.ts')
  })

  it('rejects a ported entry whose test title is not in the file', () => {
    const entry = {
      ...preservedFixture(),
      state: 'ported' as const,
      evidence: [
        {
          kind: 'react-test' as const,
          path: 'src/react-tests/quota-core.test.ts',
          test: 'a test title that no file carries',
        },
      ],
    }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(
      problems,
      'test title not found in src/react-tests/quota-core.test.ts',
    )
    rejects(problems, 'ported entry lacks a react-test evidence')
  })

  it('rejects a preserved entry whose legacy file is gone', () => {
    const entry = {
      ...preservedFixture('tests/gone.test.ts'),
      evidence: [{ kind: 'kept-suite' as const, path: 'tests/gone.test.ts' }],
    }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/gone.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'preserved legacy file is missing: tests/gone.test.ts')
  })

  it('rejects an obsolete entry without a technical reason', () => {
    const entry = {
      ...preservedFixture(),
      state: 'obsolete' as const,
      evidence: [{ kind: 'none' as const, reason: '  ' }],
    }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'empty reason for tests/legacy.test.ts')
    rejects(problems, 'obsolete entry lacks a technical reason')
  })

  it('rejects a replaced entry without a suite or browser evidence', () => {
    const entry = {
      ...preservedFixture(),
      state: 'replaced' as const,
      evidence: [{ kind: 'kept-suite' as const, path: 'tests/legacy.test.ts' }],
    }
    const problems = validateLegacyModuleDispositions(
      [entry],
      ['tests/legacy.test.ts'],
      fixtureChecks,
    )
    rejects(problems, 'replaced entry lacks a react-suite or browser-scenario')
  })

  it('accepts a ported entry whose test title is in the file', () => {
    const entry = {
      ...preservedFixture(),
      state: 'ported' as const,
      evidence: [
        {
          kind: 'react-test' as const,
          path: 'src/react-tests/quota-core.test.ts',
          test: 'keeps the badge layer on the shared pure calculation layer',
        },
      ],
    }
    expect(
      validateLegacyModuleDispositions(
        [entry],
        ['tests/legacy.test.ts'],
        fixtureChecks,
      ),
    ).toEqual([])
  })
})
