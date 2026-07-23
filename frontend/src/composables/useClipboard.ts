export async function copyText(text: string | null | undefined): Promise<void> {
  if (!text) return
  await navigator.clipboard.writeText(text)
}
