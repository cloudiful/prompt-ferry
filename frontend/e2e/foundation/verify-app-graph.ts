import { existsSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'
import { build } from 'vite'
import type { Plugin, UserConfig } from 'vite'
import viteConfig from '../../vite.config'

const foundationDir = import.meta.dirname
const frontendRoot = path.resolve(foundationDir, '../..')
const entry = path.resolve(frontendRoot, 'src/main.tsx')
const sourceRoot = path.resolve(frontendRoot, 'src')
const htmlPath = path.resolve(frontendRoot, 'index.html')
const forbiddenPackages = new Set([
  '@hey-api/openapi-ts',
  '@vue/devtools-api',
  '@nuxt/icon',
  '@nuxt/ui',
  '@vitejs/plugin-vue',
  '@tanstack/vue-table',
  'pinia',
  'typescript',
  'vue',
  'vue-i18n',
  'vue-router',
])
const importPatterns = [
  /\b(?:import|export)\s+(?:type\s+)?(?:[^'"`]*?\s+from\s*)?['"]([^'"]+)['"]/g,
  /\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
  /\brequire\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
]
const codeExtensions = new Set(['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs'])

function packageName(specifier: string): string | null {
  if (
    specifier.startsWith('.') ||
    specifier.startsWith('/') ||
    specifier.startsWith('@/')
  ) {
    return null
  }
  if (specifier.startsWith('@'))
    return specifier.split('/').slice(0, 2).join('/')
  return specifier.split('/')[0] ?? null
}

function splitResourceQuery(specifier: string): {
  pathname: string
  query: string | null
} {
  const queryIndex = specifier.indexOf('?')
  return queryIndex < 0
    ? { pathname: specifier, query: null }
    : {
        pathname: specifier.slice(0, queryIndex),
        query: specifier.slice(queryIndex + 1),
      }
}

function isViteResourceQuery(query: string | null): boolean {
  if (!query) return false
  const keys = [...new URLSearchParams(query).keys()]
  return keys.length > 0 && keys.every((key) => key === 'raw' || key === 'url')
}

function isTerminalResourceImport(specifier: string): boolean {
  const { query } = splitResourceQuery(specifier)
  return isViteResourceQuery(query)
}

function resolveLocal(specifier: string, importer: string): string | null {
  const { pathname, query } = splitResourceQuery(specifier)
  const localPath =
    query === null || isViteResourceQuery(query) ? pathname : specifier
  if (localPath.startsWith('@/')) {
    return path.resolve(sourceRoot, localPath.slice(2))
  }
  if (localPath.startsWith('.'))
    return path.resolve(path.dirname(importer), localPath)
  return null
}

function resolveFile(base: string): string | null {
  if (existsSync(base) && statSync(base).isFile()) return base
  const extension = path.extname(base)
  const candidates =
    extension && ['.css', '.js', '.jsx', '.ts', '.tsx'].includes(extension)
      ? []
      : [
          `${base}.ts`,
          `${base}.tsx`,
          `${base}.d.ts`,
          `${base}.js`,
          `${base}.jsx`,
          `${base}.css`,
          path.join(base, 'index.ts'),
          path.join(base, 'index.tsx'),
          path.join(base, 'index.d.ts'),
          path.join(base, 'index.js'),
          path.join(base, 'index.jsx'),
        ]
  return candidates.find((candidate) => existsSync(candidate)) ?? null
}

function importedSpecifiers(source: string): string[] {
  const matches: string[] = []
  for (const pattern of importPatterns) {
    pattern.lastIndex = 0
    for (const match of source.matchAll(pattern)) matches.push(match[1]!)
  }
  return matches
}

const failures: string[] = []
const resolverRegressions = [
  {
    specifier: '@/assets/providers/minimax.svg?raw',
    expected: path.resolve(sourceRoot, 'assets/providers/minimax.svg'),
    terminalResource: true,
  },
  {
    specifier: '@/types/i18n',
    expected: path.resolve(sourceRoot, 'types/i18n.d.ts'),
    terminalResource: false,
  },
]
for (const regression of resolverRegressions) {
  const localBase = resolveLocal(regression.specifier, entry)
  const resolved = localBase ? resolveFile(localBase) : null
  if (resolved !== regression.expected) {
    failures.push(`resolver regression failed for ${regression.specifier}`)
  }
  if (
    isTerminalResourceImport(regression.specifier) !==
    regression.terminalResource
  ) {
    failures.push(
      `resource query regression failed for ${regression.specifier}`,
    )
  }
}
const unresolvedQuery = resolveLocal(
  '@/assets/providers/minimax.svg?component',
  entry,
)
if (unresolvedQuery && resolveFile(unresolvedQuery)) {
  failures.push('unsupported Vite asset query was treated as a resolved import')
}
const visited = new Set<string>()
const activeFiles: string[] = []
const queue = [entry]

while (queue.length > 0) {
  const file = path.resolve(queue.pop()!)
  if (visited.has(file)) continue
  visited.add(file)

  if (!file.startsWith(`${sourceRoot}${path.sep}`)) {
    failures.push(
      `active local import escapes src: ${path.relative(frontendRoot, file)}`,
    )
    continue
  }

  const extension = path.extname(file)
  if (!codeExtensions.has(extension)) continue
  activeFiles.push(file)
  const source = readFileSync(file, 'utf8')

  for (const specifier of importedSpecifiers(source)) {
    const name = packageName(specifier)
    if (name && forbiddenPackages.has(name)) {
      failures.push(`${path.relative(sourceRoot, file)} imports ${specifier}`)
    }
    const { pathname } = splitResourceQuery(specifier)
    if (pathname.endsWith('.vue')) {
      failures.push(`${path.relative(sourceRoot, file)} imports ${specifier}`)
      continue
    }

    const localBase = resolveLocal(specifier, file)
    if (!localBase) continue
    const resolved = resolveFile(localBase)
    if (!resolved) {
      failures.push(
        `${path.relative(sourceRoot, file)} cannot resolve ${specifier}`,
      )
      continue
    }
    if (isTerminalResourceImport(specifier)) continue
    if (path.extname(resolved) === '.css') continue
    if (!codeExtensions.has(path.extname(resolved))) {
      failures.push(
        `${path.relative(sourceRoot, file)} imports unsupported asset ${specifier}; use a valid ?raw or ?url query`,
      )
      continue
    }
    queue.push(resolved)
  }
}

const bundledModules = new Set<string>()
const bundleGraphPlugin: Plugin = {
  name: 'foundation-bundle-graph',
  generateBundle(_options, bundle) {
    for (const output of Object.values(bundle)) {
      if (output.type === 'chunk') {
        for (const moduleId of Object.keys(output.modules)) {
          bundledModules.add(moduleId)
        }
      }
    }
  },
}

await build({
  ...viteConfig,
  configFile: false,
  plugins: [...(viteConfig.plugins ?? []), bundleGraphPlugin],
  build: { ...viteConfig.build, write: false },
} as UserConfig)

function bundledPackageName(moduleId: string): string | null {
  const match = moduleId.match(/(?:^|\/)node_modules\/(.+)$/)
  if (!match) return null
  const parts = match[1]!.split('/')
  return parts[0]?.startsWith('@')
    ? parts.slice(0, 2).join('/')
    : (parts[0] ?? null)
}

for (const moduleId of bundledModules) {
  const name = bundledPackageName(moduleId)
  if (name && forbiddenPackages.has(name)) {
    failures.push(`production bundle contains ${name}: ${moduleId}`)
  }
}

const html = readFileSync(htmlPath, 'utf8')
const moduleEntries = [
  ...html.matchAll(
    /<script\b[^>]*type="module"[^>]*src="([^"]+)"[^>]*><\/script>/g,
  ),
]
const entrySource = readFileSync(entry, 'utf8')
const createRootCalls = [...entrySource.matchAll(/\bcreateRoot\s*\(/g)].length
if (moduleEntries.length !== 1 || moduleEntries[0]?.[1] !== '/src/main.tsx') {
  failures.push('index.html must load only /src/main.tsx as its module entry')
}
if (createRootCalls !== 1)
  failures.push(`expected one createRoot call, found ${createRootCalls}`)
if (!html.includes('id="app"'))
  failures.push('index.html has no #app mount element')

const radixModules = [...bundledModules]
  .filter(
    (moduleId) =>
      moduleId.includes('/node_modules/radix-ui/') ||
      moduleId.includes('/node_modules/@radix-ui/'),
  )
  .sort()

if (failures.length > 0) {
  console.error(failures.join('\n'))
  process.exitCode = 1
} else {
  console.log(
    `React entry graph: ${activeFiles.length} source modules, ${bundledModules.size} bundled modules, ${radixModules.length} Radix modules, one mounted root, no Vue/Nuxt imports`,
  )
}
