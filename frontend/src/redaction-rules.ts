import type { CustomStringRuleSchema } from './generated/admin-api'

export type CustomStringRuleWarning = {
  duplicate: boolean
  conflict: boolean
  invalidRegex: boolean
}

export const EMPTY_RULE_WARNING: CustomStringRuleWarning = {
  duplicate: false,
  conflict: false,
  invalidRegex: false,
}

export function validateCustomStringRule(
  pattern: string,
  matchType: string,
): { invalidRegex: boolean } {
  const trimmed = pattern.trim()
  if (matchType !== 'regex' || !trimmed) {
    return { invalidRegex: false }
  }
  try {
    new RegExp(trimmed)
    return { invalidRegex: false }
  } catch {
    return { invalidRegex: true }
  }
}

function ruleSignature(rule: {
  pattern: string
  match_type: string
  scope: string
}): string {
  return `${rule.pattern.trim()}::${rule.match_type}::${rule.scope}`
}

function rulePatternKey(rule: { pattern: string }): string {
  return rule.pattern.trim()
}

export function findRuleWarnings(
  rules: CustomStringRuleSchema[],
): CustomStringRuleWarning[] {
  const signatures = new Map<string, number>()
  const patterns = new Map<string, number>()
  for (const rule of rules) {
    if (!rule.pattern.trim()) continue
    const signature = ruleSignature(rule)
    signatures.set(signature, (signatures.get(signature) ?? 0) + 1)
    const key = rulePatternKey(rule)
    patterns.set(key, (patterns.get(key) ?? 0) + 1)
  }
  return rules.map((rule) => {
    const trimmed = rule.pattern.trim()
    if (!trimmed) return EMPTY_RULE_WARNING
    const { invalidRegex } = validateCustomStringRule(
      rule.pattern,
      rule.match_type,
    )
    const signature = ruleSignature(rule)
    const pattern = rulePatternKey(rule)
    const signatureCount = signatures.get(signature) ?? 0
    const patternCount = patterns.get(pattern) ?? 0
    return {
      duplicate: signatureCount > 1,
      conflict:
        !signatureCount || signatureCount === 1 ? patternCount > 1 : false,
      invalidRegex,
    }
  })
}
