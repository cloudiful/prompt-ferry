import { useLocale } from '@/lib/i18n'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

type ProxyScheme = 'direct' | 'http' | 'https' | 'socks5' | 'socks5h' | 'kept'

type ProxySettingsFieldsProps = {
  hasSaved: boolean
  onChange: (value: { proxyUrl: string; hasSaved: boolean }) => void
  proxyUrl: string
}

const PROXY_SCHEMES: Exclude<ProxyScheme, 'kept'>[] = [
  'direct',
  'http',
  'https',
  'socks5',
  'socks5h',
]

function parseProxyUrl(value: string): {
  scheme: ProxyScheme
  address: string
} {
  const trimmed = (value ?? '').trim()
  if (!trimmed) return { scheme: 'direct', address: '' }
  const match = trimmed.match(/^(http|https|socks5h|socks5):\/\/(.*)$/i)
  if (match) {
    return {
      scheme: match[1].toLowerCase() as ProxyScheme,
      address: (match[2] ?? '').trim(),
    }
  }
  return { scheme: 'http', address: trimmed }
}

export function isProxyUrlValid(value: string | null | undefined): boolean {
  const trimmed = (value ?? '').trim()
  if (!trimmed) return true
  const match = trimmed.match(/^(http|https|socks5h|socks5):\/\/(.*)$/i)
  return !match || (match[2] ?? '').trim() !== ''
}

export function ProxySettingsFields({
  hasSaved,
  onChange,
  proxyUrl,
}: ProxySettingsFieldsProps) {
  const { t } = useLocale()
  const parsed = parseProxyUrl(proxyUrl)
  const currentScheme: ProxyScheme =
    !proxyUrl.trim() && hasSaved ? 'kept' : parsed.scheme
  const currentAddress =
    currentScheme === 'direct' || currentScheme === 'kept' ? '' : parsed.address
  const schemeItems: ProxyScheme[] = hasSaved
    ? ['kept', ...PROXY_SCHEMES]
    : PROXY_SCHEMES

  function changeScheme(value: ProxyScheme): void {
    if (value === 'kept') {
      onChange({ proxyUrl: '', hasSaved: true })
      return
    }
    if (value === 'direct') {
      onChange({ proxyUrl: '', hasSaved: false })
      return
    }
    const address = currentAddress.trim()
    onChange({
      proxyUrl: !address
        ? `${value}://`
        : address.includes('://')
          ? address
          : `${value}://${address}`,
      hasSaved,
    })
  }

  function changeAddress(value: string): void {
    if (currentScheme === 'direct' || currentScheme === 'kept') return
    const trimmed = value.trim()
    onChange({
      proxyUrl: !trimmed
        ? `${currentScheme}://`
        : trimmed.includes('://')
          ? trimmed
          : `${currentScheme}://${trimmed}`,
      hasSaved,
    })
  }

  const addressPlaceholder =
    currentScheme === 'https'
      ? 'https://127.0.0.1:8443'
      : currentScheme === 'socks5'
        ? 'socks5://user:pass@10.0.0.1:1080'
        : currentScheme === 'socks5h'
          ? 'socks5h://user:pass@10.0.0.1:1080'
          : 'http://127.0.0.1:7890'
  const showClear = hasSaved || proxyUrl.trim() !== ''

  return (
    <div className="grid gap-3">
      {hasSaved ? (
        <span className="w-fit rounded bg-muted px-2 py-1 text-xs text-muted-foreground">
          {t('saved')}
        </span>
      ) : null}
      <Select
        onValueChange={(value) => changeScheme(value as ProxyScheme)}
        value={currentScheme}
      >
        <SelectTrigger aria-label={t('proxyScheme')} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {schemeItems.map((scheme) => (
            <SelectItem key={scheme} value={scheme}>
              {scheme === 'direct'
                ? t('proxyDirect')
                : scheme === 'kept'
                  ? t('proxySchemeKept')
                  : scheme}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {currentScheme !== 'direct' && currentScheme !== 'kept' ? (
        <Input
          aria-label={t('proxyAddress')}
          autoComplete="off"
          onChange={(event) => changeAddress(event.currentTarget.value)}
          placeholder={addressPlaceholder}
          value={currentAddress}
        />
      ) : null}
      {showClear ? (
        <Button
          className="justify-self-start"
          onClick={() => onChange({ proxyUrl: '', hasSaved: false })}
          size="sm"
          type="button"
          variant="ghost"
        >
          {t('proxyClear')}
        </Button>
      ) : null}
    </div>
  )
}
