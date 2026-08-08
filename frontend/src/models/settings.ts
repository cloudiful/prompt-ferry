export function resolveSettingsTab(
  tab: string | null | undefined,
  isAdmin: boolean,
): string {
  if (tab === 'requests' || tab === 'network' || tab === 'review') {
    return isAdmin ? tab : 'general'
  }
  return 'general'
}
