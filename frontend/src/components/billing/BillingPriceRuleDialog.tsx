import { Save } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import type { BillingPriceRuleForm } from '@/lib/billing-model'

type BillingPriceRuleDialogProps = {
  form: BillingPriceRuleForm
  pending: boolean
  savePending: boolean
  onChange: (patch: Partial<BillingPriceRuleForm>) => void
  onOpenChange: (open: boolean) => void
  onSave: () => void
  open: boolean
}

export function BillingPriceRuleDialog({
  form,
  pending,
  savePending,
  onChange,
  onOpenChange,
  onSave,
  open,
}: BillingPriceRuleDialogProps) {
  const { t } = useLocale()
  const timeZone = useDisplayTimeZone()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>
            {form.price_rule_id ? t('editPriceRule') : t('newPriceRule')}
          </DialogTitle>
          <DialogDescription>
            {`${t('browserTimeZone')}: ${timeZone.effective}`}
          </DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-3 text-xs"
          onSubmit={(event) => {
            event.preventDefault()
            onSave()
          }}
        >
          <fieldset className="grid gap-3" disabled={pending}>
            <label className="grid gap-1">
              <span className="text-muted-foreground">
                {t('effectiveFrom')}
              </span>
              <Input
                aria-label={t('effectiveFrom')}
                name="price_rule_effective_from"
                onChange={(event) =>
                  onChange({ effective_from: event.currentTarget.value })
                }
                required
                type="datetime-local"
                value={form.effective_from}
              />
            </label>
            <label className="grid gap-1">
              <span className="text-muted-foreground">{t('publicModel')}</span>
              <Input
                aria-label={t('publicModel')}
                name="price_rule_public_model"
                onChange={(event) =>
                  onChange({ public_model: event.currentTarget.value })
                }
                required
                type="text"
                value={form.public_model}
              />
            </label>
            <div className="grid gap-2 sm:grid-cols-2">
              <label className="grid gap-1">
                <span className="text-muted-foreground">
                  {`${t('inputRate')} (${t('perMillionTokens')})`}
                </span>
                <Input
                  aria-label={t('inputRate')}
                  inputMode="decimal"
                  name="price_rule_input_rate"
                  onChange={(event) =>
                    onChange({ input_rate: event.currentTarget.value })
                  }
                  required
                  type="text"
                  value={form.input_rate}
                />
              </label>
              <label className="grid gap-1">
                <span className="text-muted-foreground">
                  {`${t('cacheReadRate')} (${t('perMillionTokens')})`}
                </span>
                <Input
                  aria-label={t('cacheReadRate')}
                  inputMode="decimal"
                  name="price_rule_cache_read_rate"
                  onChange={(event) =>
                    onChange({ cache_read_rate: event.currentTarget.value })
                  }
                  required
                  type="text"
                  value={form.cache_read_rate}
                />
              </label>
              <label className="grid gap-1">
                <span className="text-muted-foreground">
                  {`${t('cacheWriteRate')} (${t('perMillionTokens')})`}
                </span>
                <Input
                  aria-label={t('cacheWriteRate')}
                  inputMode="decimal"
                  name="price_rule_cache_write_rate"
                  onChange={(event) =>
                    onChange({ cache_write_rate: event.currentTarget.value })
                  }
                  required
                  type="text"
                  value={form.cache_write_rate}
                />
              </label>
              <label className="grid gap-1">
                <span className="text-muted-foreground">
                  {`${t('outputRate')} (${t('perMillionTokens')})`}
                </span>
                <Input
                  aria-label={t('outputRate')}
                  inputMode="decimal"
                  name="price_rule_output_rate"
                  onChange={(event) =>
                    onChange({ output_rate: event.currentTarget.value })
                  }
                  required
                  type="text"
                  value={form.output_rate}
                />
              </label>
            </div>
          </fieldset>
          <DialogFooter>
            <Button
              disabled={pending}
              onClick={() => onOpenChange(false)}
              type="button"
              variant="outline"
            >
              {t('cancel')}
            </Button>
            <Button disabled={pending} type="submit">
              {savePending ? (
                <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
              ) : (
                <Save aria-hidden="true" />
              )}
              {t('save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
