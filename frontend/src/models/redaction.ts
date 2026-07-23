import type {
  RedactionConfigSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
} from '../generated/admin-api'

export type RedactionRuleOptionView = {
  key: keyof RedactionConfigSchema['rules']
  label: string
}

export type RedactionOptionView<T = string> = {
  label: string
  value: T
}

export type RedactionPreviewStatView = {
  label: string
  value: string
}

export type RedactionWorkspaceView = {
  enabled_rule_count: number
  rule_options: RedactionRuleOptionView[]
  input_kind_options: RedactionOptionView<RedactionInputKindSchema>[]
  match_type_options: RedactionOptionView<'exact' | 'contains' | 'regex'>[]
  scope_options: RedactionOptionView<'text' | 'line'>[]
  preview_stats: RedactionPreviewStatView[]
  findings_count: number
  replacements_count: number
}

export function createRedactionWorkspaceView(options: {
  config: RedactionConfigSchema
  previewResult: RedactionPreviewSchema | null
  t: TranslateFn
}): RedactionWorkspaceView {
  const ruleOptions: RedactionRuleOptionView[] = [
    { key: 'secret', label: options.t('ruleSecret') },
    { key: 'domain', label: options.t('ruleDomain') },
    { key: 'url', label: options.t('ruleUrl') },
    { key: 'email', label: options.t('ruleEmail') },
    { key: 'ip', label: options.t('ruleIp') },
    { key: 'cidr', label: options.t('ruleCidr') },
    { key: 'phone', label: options.t('rulePhone') },
    { key: 'person', label: options.t('rulePerson') },
    { key: 'organization', label: options.t('ruleOrganization') },
  ]
  const stats = options.previewResult?.stats
  return {
    enabled_rule_count: ruleOptions.filter(
      (rule) => options.config.rules[rule.key],
    ).length,
    rule_options: ruleOptions,
    input_kind_options: [
      { label: options.t('textScope'), value: 'text' },
      { label: 'Git diff', value: 'git_diff' },
    ],
    match_type_options: [
      { label: options.t('exact'), value: 'exact' },
      { label: options.t('contains'), value: 'contains' },
      { label: options.t('regex'), value: 'regex' },
    ],
    scope_options: [
      { label: options.t('textScope'), value: 'text' },
      { label: options.t('lineScope'), value: 'line' },
    ],
    preview_stats: [
      {
        label: options.t('findings'),
        value: String(stats?.total_findings ?? 0),
      },
      {
        label: options.t('replacements'),
        value: String(stats?.applied_replacements ?? 0),
      },
      {
        label: options.t('failed'),
        value: String(stats?.dropped_findings ?? 0),
      },
      {
        label: 'LLM',
        value: stats
          ? stats.llm_configured
            ? options.t('active')
            : options.t('disabled')
          : '-',
      },
    ],
    findings_count: options.previewResult?.findings.length ?? 0,
    replacements_count: options.previewResult?.applied_replacements.length ?? 0,
  }
}
