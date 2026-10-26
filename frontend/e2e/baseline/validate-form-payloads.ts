import { readFileSync } from 'node:fs'
const r = JSON.parse(
  readFileSync(new URL('./form-payloads.json', import.meta.url), 'utf8'),
)
function check(ok: unknown, message: string) {
  if (!ok) throw new Error(message)
}
check(r.failures.length === 0, 'form callback failed')
const d = r.checks.dimensions
check(
  d.invalid === 'true' && d.disabled && d.invalidRequests === 0,
  'invalid dimension must block save',
)
const positive = d.positive.body.targets[0]
check(
  positive.embedding_dimensions_override === 1536 && d.roundtrip === '1536',
  'positive dimension roundtrip',
)
check(
  JSON.stringify(positive.active_windows) ===
    JSON.stringify([{ start: '00:00', end: '24:00', days: [1, 2, 3, 4, 5] }]),
  'canonical weekday all-day',
)
check(
  d.cleared.body.targets[0].embedding_dimensions_override === null &&
    d.cleared.body.targets[0].active_windows.length === 0,
  'clear dimensions/inherit schedule',
)
const c = r.checks.chatControls.saved.body.targets[0]
check(
  c.native_api === 'chat' &&
    c.compact_mode === 'self_summarize' &&
    c.thinking_effort_override === 'high' &&
    c.dev_system_normalize &&
    c.thinking_downgrade_enabled,
  'chat payload',
)
check(
  r.checks.chatControls.combos.includes('Chat') &&
    r.checks.chatControls.combos.includes('自研摘要'),
  'chat roundtrip',
)
const targets = r.checks.targets.saved.body.targets
check(
  targets.length === 2 &&
    targets[0].endpoint_id === 'ep-minimax-cn' &&
    targets[0].enabled === false &&
    targets[1].endpoint_id === 'ep-openai-codex',
  'keyboard order/toggle/add-remove payload',
)
check(
  targets[0].native_api === 'chat' &&
    targets[1].native_api === 'responses' &&
    targets[1].upstream_model.includes('gpt-5.1'),
  'model/protocol association',
)
for (const protocol of ['embeddings', 'chat', 'responses']) {
  check(
    r.checks.modelSuggestionQueries.some(
      (q: any) => q.query.native_api === protocol,
    ),
    `missing ${protocol} suggestion query`,
  )
}
console.log(
  'Dimension, schedule, chat override, target ordering and scoped suggestion payload assertions OK; remaining scenarios not waived',
)
