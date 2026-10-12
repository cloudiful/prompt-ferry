export async function loadActionFactory<T>(
  path: string,
  name: string,
  dependencies: Record<string, unknown>,
): Promise<T> {
  const source = await Bun.file(new URL(path, import.meta.url)).text()
  const functionSource = source
    .slice(source.indexOf(`export function ${name}`))
    .replace(/^export /, '')
  const javascript = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    functionSource,
  )
  return new Function(
    ...Object.keys(dependencies),
    `${javascript}; return ${name}`,
  )(...Object.values(dependencies)) as T
}
