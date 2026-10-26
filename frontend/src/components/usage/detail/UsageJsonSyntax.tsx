import { Fragment, useMemo, type ReactNode } from 'react'
import { usageJsonText } from '@/lib/usage-detail-model'

const TOKEN_REGEX =
  /("(?:\\.|[^"\\])*")(\s*:)?|\btrue\b|\bfalse\b|\bnull\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g

function tokenClass(
  token: string,
  rest: string | undefined,
): string | undefined {
  if (token.startsWith('"')) {
    return rest === undefined ? 'text-[#86efac]' : 'text-[#7dd3fc]'
  }
  if (token === 'true' || token === 'false') return 'text-[#f9a8d4]'
  if (token === 'null') return 'text-[#fda4af]'
  return 'text-[#c4b5fd]'
}

function lineNodes(line: string, key: number): ReactNode[] {
  const nodes: ReactNode[] = []
  let lastIndex = 0
  TOKEN_REGEX.lastIndex = 0
  let match = TOKEN_REGEX.exec(line)
  let index = 0
  while (match) {
    if (match.index > lastIndex) {
      nodes.push(
        <Fragment key={`text-${key}-${index}`}>
          {line.slice(lastIndex, match.index)}
        </Fragment>,
      )
      index += 1
    }
    const token = match[0]
    nodes.push(
      <span
        className={tokenClass(token, match[2])}
        key={`token-${key}-${index}`}
      >
        {token}
      </span>,
    )
    index += 1
    lastIndex = match.index + token.length
    match = TOKEN_REGEX.exec(line)
  }
  if (lastIndex < line.length) {
    nodes.push(<Fragment key={`tail-${key}`}>{line.slice(lastIndex)}</Fragment>)
  }
  if (nodes.length === 0)
    nodes.push(<Fragment key={`empty-${key}`}> </Fragment>)
  return nodes
}

type UsageJsonSyntaxProps = {
  value: unknown
  maxHeight?: string
}

/**
 * A syntax-highlighted JSON block rendered as React elements.
 *
 * The payload is untrusted text, so it is never assembled into an HTML string:
 * every token is a text node, which keeps `&`, `<` and `>` literal and closes
 * the injection hazard of the legacy HTML builder. The scroll container keeps
 * a long payload readable without truncating it.
 */
export function UsageJsonSyntax({
  value,
  maxHeight = '24rem',
}: UsageJsonSyntaxProps) {
  const lines = useMemo(() => usageJsonText(value).split('\n'), [value])
  return (
    <div
      className="overflow-auto rounded-md border bg-[#0a0d12] px-3 py-2.5 font-mono text-[0.78rem] leading-6 text-[#d7e0ea]"
      data-usage-json
      style={{ maxHeight }}
    >
      {lines.map((line, lineIndex) => (
        <div
          className="grid grid-cols-[2rem_minmax(0,1fr)] gap-4"
          key={lineIndex}
        >
          <div className="text-right text-[#5f6b7a] select-none">
            {lineIndex + 1}
          </div>
          <code className="block break-words whitespace-pre-wrap">
            {lineNodes(line, lineIndex)}
          </code>
        </div>
      ))}
    </div>
  )
}
