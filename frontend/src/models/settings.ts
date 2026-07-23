export type SettingsWorkspaceView = {
  request_retention_note: string
}

type SettingsWorkspaceOptions = {
  requestRetentionDays: number
  t: TranslateFn
}

export function createSettingsWorkspaceView(
  options: SettingsWorkspaceOptions,
): SettingsWorkspaceView {
  return {
    request_retention_note: options
      .t('contentLoggingRetentionHint')
      .replace('{days}', String(options.requestRetentionDays)),
  }
}

export function resolveSettingsTab(
  tab: string | null | undefined,
  isAdmin: boolean,
): string {
  if (tab === 'requests' || tab === 'network' || tab === 'review') {
    return isAdmin ? tab : 'general'
  }
  return 'general'
}
