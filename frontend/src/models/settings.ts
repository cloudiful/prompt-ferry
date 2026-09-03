export type SettingsTab =
  'general' | 'requests' | 'network' | 'review' | 'storage'

const adminTabs: ReadonlySet<string> = new Set<SettingsTab>([
  'requests',
  'network',
  'review',
  'storage',
])

export function resolveSettingsTab(
  tab: string | null | undefined,
  isAdmin: boolean,
): SettingsTab {
  if (tab && adminTabs.has(tab)) {
    return (isAdmin ? tab : 'general') as SettingsTab
  }
  return 'general'
}
