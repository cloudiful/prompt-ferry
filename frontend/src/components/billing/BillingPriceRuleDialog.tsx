import { useForm } from '@tanstack/react-form'
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
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import type { BillingPriceRuleForm } from '@/lib/billing-model'

type BillingPriceRuleDialogProps = {
  initialForm: BillingPriceRuleForm
  pending: boolean
  savePending: boolean
  onOpenChange: (open: boolean) => void
  onSave: (form: BillingPriceRuleForm) => void
  open: boolean
}

type FieldErrorProps = {
  attempts: number
  errors: Array<unknown>
}

function FieldError({ attempts, errors }: FieldErrorProps) {
  if (attempts === 0 || errors.length === 0) return null
  return (
    <p className="text-destructive" role="alert">
      {String(errors[0])}
    </p>
  )
}

export function BillingPriceRuleDialog({
  initialForm,
  pending,
  savePending,
  onOpenChange,
  onSave,
  open,
}: BillingPriceRuleDialogProps) {
  const { t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const form = useForm({
    defaultValues: initialForm,
    onSubmit: async ({ value }) => {
      onSave(value)
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>
            {initialForm.price_rule_id ? t('editPriceRule') : t('newPriceRule')}
          </DialogTitle>
          <DialogDescription>
            {`${t('browserTimeZone')}: ${timeZone.effective}`}
          </DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-3 text-xs"
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            void form.handleSubmit()
          }}
        >
          <form.Subscribe selector={(state) => state.submissionAttempts}>
            {(attempts) => (
              <fieldset className="grid gap-3" disabled={pending}>
                <form.Field
                  name="effective_from"
                  validators={{
                    onChange: ({ value }) =>
                      value.trim() ? undefined : t('effectiveFromRequired'),
                  }}
                >
                  {(field) => (
                    <label className="grid gap-1">
                      <span className="text-muted-foreground">
                        {t('effectiveFrom')}
                      </span>
                      <Input
                        aria-label={t('effectiveFrom')}
                        name="price_rule_effective_from"
                        onBlur={field.handleBlur}
                        onChange={(event) =>
                          field.handleChange(event.currentTarget.value)
                        }
                        required
                        type="datetime-local"
                        value={field.state.value}
                      />
                      <FieldError
                        attempts={attempts}
                        errors={field.state.meta.errors}
                      />
                    </label>
                  )}
                </form.Field>
                <form.Field
                  name="public_model"
                  validators={{
                    onChange: ({ value }) =>
                      value.trim() ? undefined : t('publicModelRequired'),
                  }}
                >
                  {(field) => (
                    <label className="grid gap-1">
                      <span className="text-muted-foreground">
                        {t('publicModel')}
                      </span>
                      <Input
                        aria-label={t('publicModel')}
                        name="price_rule_public_model"
                        onBlur={field.handleBlur}
                        onChange={(event) =>
                          field.handleChange(event.currentTarget.value)
                        }
                        required
                        type="text"
                        value={field.state.value}
                      />
                      <FieldError
                        attempts={attempts}
                        errors={field.state.meta.errors}
                      />
                    </label>
                  )}
                </form.Field>
                <div className="grid gap-2 sm:grid-cols-2">
                  <form.Field
                    name="input_rate"
                    validators={{
                      onChange: ({ value }) =>
                        value.trim() ? undefined : t('rateRequired'),
                    }}
                  >
                    {(field) => (
                      <label className="grid gap-1">
                        <span className="text-muted-foreground">
                          {`${t('inputRate')} (${t('perMillionTokens')})`}
                        </span>
                        <Input
                          aria-label={t('inputRate')}
                          inputMode="decimal"
                          name="price_rule_input_rate"
                          onBlur={field.handleBlur}
                          onChange={(event) =>
                            field.handleChange(event.currentTarget.value)
                          }
                          required
                          type="text"
                          value={field.state.value}
                        />
                        <FieldError
                          attempts={attempts}
                          errors={field.state.meta.errors}
                        />
                      </label>
                    )}
                  </form.Field>
                  <form.Field
                    name="cache_read_rate"
                    validators={{
                      onChange: ({ value }) =>
                        value.trim() ? undefined : t('rateRequired'),
                    }}
                  >
                    {(field) => (
                      <label className="grid gap-1">
                        <span className="text-muted-foreground">
                          {`${t('cacheReadRate')} (${t('perMillionTokens')})`}
                        </span>
                        <Input
                          aria-label={t('cacheReadRate')}
                          inputMode="decimal"
                          name="price_rule_cache_read_rate"
                          onBlur={field.handleBlur}
                          onChange={(event) =>
                            field.handleChange(event.currentTarget.value)
                          }
                          required
                          type="text"
                          value={field.state.value}
                        />
                        <FieldError
                          attempts={attempts}
                          errors={field.state.meta.errors}
                        />
                      </label>
                    )}
                  </form.Field>
                  <form.Field
                    name="cache_write_rate"
                    validators={{
                      onChange: ({ value }) =>
                        value.trim() ? undefined : t('rateRequired'),
                    }}
                  >
                    {(field) => (
                      <label className="grid gap-1">
                        <span className="text-muted-foreground">
                          {`${t('cacheWriteRate')} (${t('perMillionTokens')})`}
                        </span>
                        <Input
                          aria-label={t('cacheWriteRate')}
                          inputMode="decimal"
                          name="price_rule_cache_write_rate"
                          onBlur={field.handleBlur}
                          onChange={(event) =>
                            field.handleChange(event.currentTarget.value)
                          }
                          required
                          type="text"
                          value={field.state.value}
                        />
                        <FieldError
                          attempts={attempts}
                          errors={field.state.meta.errors}
                        />
                      </label>
                    )}
                  </form.Field>
                  <form.Field
                    name="output_rate"
                    validators={{
                      onChange: ({ value }) =>
                        value.trim() ? undefined : t('rateRequired'),
                    }}
                  >
                    {(field) => (
                      <label className="grid gap-1">
                        <span className="text-muted-foreground">
                          {`${t('outputRate')} (${t('perMillionTokens')})`}
                        </span>
                        <Input
                          aria-label={t('outputRate')}
                          inputMode="decimal"
                          name="price_rule_output_rate"
                          onBlur={field.handleBlur}
                          onChange={(event) =>
                            field.handleChange(event.currentTarget.value)
                          }
                          required
                          type="text"
                          value={field.state.value}
                        />
                        <FieldError
                          attempts={attempts}
                          errors={field.state.meta.errors}
                        />
                      </label>
                    )}
                  </form.Field>
                </div>
              </fieldset>
            )}
          </form.Subscribe>
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
