import { useEffect } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import { Info, LoaderCircle, Trash2 } from 'lucide-react'
import type { UsageClearScope, User } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useLocale } from '@/lib/i18n'
import {
  createDefaultUsageClearForm,
  usageClearScopeOptions,
  usageClearUserOptions,
  type UsageClearForm,
} from '@/lib/usage-clear'

type UsageClearDialogProps = {
  currentUserLabel: string
  isAdmin: boolean
  onOpenChange: (open: boolean) => void
  onSubmit: (captured: UsageClearForm) => void
  open: boolean
  pending: boolean
  pendingOrigin: string | null
  users: User[]
}

/**
 * The clear-history dialog.
 *
 * The dialog owns the form values and snapshots them on submit; the parent
 * owns the request guard and decides whether a completion may close it. A clear
 * in flight freezes every dismissal path and only its own submit shows a
 * spinner; the category is never part of the payload.
 */
export function UsageClearDialog({
  currentUserLabel,
  isAdmin,
  onOpenChange,
  onSubmit,
  open,
  pending,
  pendingOrigin,
  users,
}: UsageClearDialogProps) {
  const { t } = useLocale()
  const form = useForm({
    defaultValues: createDefaultUsageClearForm(),
    onSubmit: async ({ value }) => {
      onSubmit({ ...value })
    },
  })
  const values = useStore(form.store, (state) => state.values)

  useEffect(() => {
    if (open) form.reset(createDefaultUsageClearForm())
  }, [open])

  const scopeOptions = usageClearScopeOptions(isAdmin, {
    allUsers: t('clearAllRecords'),
    currentUser: t('clearCurrentUserHistory'),
    targetUser: t('clearSelectedUserHistory'),
  })
  const userOptions = usageClearUserOptions(users)
  const clearing = pendingOrigin === 'clear'

  return (
    <Dialog
      onOpenChange={(next) => {
        if (pending && !next) return
        onOpenChange(next)
      }}
      open={open}
    >
      <DialogContent
        aria-describedby={undefined}
        className="sm:max-w-xl"
        onEscapeKeyDown={(event) => {
          if (pending) event.preventDefault()
        }}
        onInteractOutside={(event) => {
          if (pending) event.preventDefault()
        }}
        showCloseButton={!pending}
      >
        <DialogTitle>{t('clearHistory')}</DialogTitle>
        <form
          className="grid gap-3 text-xs"
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            if (pending) return
            void form.handleSubmit()
          }}
        >
          <div className="grid gap-1">
            <div className="flex items-center gap-1">
              <Label htmlFor="usage-clear-scope">{t('clearScope')}</Label>
              <span
                aria-hidden="true"
                className="text-muted-foreground"
                title={t('clearHistoryHint')}
              >
                <Info className="size-3.5" />
              </span>
            </div>
            <form.Field name="scope">
              {(field) => (
                <Select
                  disabled={pending}
                  onValueChange={(value) =>
                    field.handleChange(value as UsageClearScope)
                  }
                  value={field.state.value}
                >
                  <SelectTrigger
                    aria-label={t('clearScope')}
                    id="usage-clear-scope"
                    size="sm"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {scopeOptions.map((option) => (
                      <SelectItem key={option.value} value={option.value}>
                        {option.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </form.Field>
          </div>

          {values.scope === 'target_user' && (
            <div className="grid gap-1">
              <Label htmlFor="usage-clear-user">{t('selectUser')}</Label>
              <form.Field name="user_id">
                {(field) => (
                  <Select
                    disabled={pending}
                    onValueChange={(value) => field.handleChange(Number(value))}
                    value={
                      field.state.value === null
                        ? ''
                        : String(field.state.value)
                    }
                  >
                    <SelectTrigger
                      aria-label={t('selectUser')}
                      id="usage-clear-user"
                      size="sm"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {userOptions.map((option) => (
                        <SelectItem
                          key={option.value}
                          value={String(option.value)}
                        >
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </form.Field>
            </div>
          )}

          {values.scope === 'current_user' && (
            <div className="rounded border bg-muted px-3 py-2 text-muted-foreground">
              {currentUserLabel}
            </div>
          )}

          <form.Field name="delete_all">
            {(field) => (
              <label className="inline-flex min-h-8 items-center gap-2 text-[0.75rem]">
                <Checkbox
                  checked={field.state.value}
                  disabled={pending}
                  id="usage-clear-all"
                  onCheckedChange={(checked) =>
                    field.handleChange(checked === true)
                  }
                />
                {t('clearAllRecords')}
              </label>
            )}
          </form.Field>

          <div className="grid gap-3 sm:grid-cols-2">
            <form.Field name="start_at">
              {(field) => (
                <div className="grid gap-1">
                  <Label htmlFor="usage-clear-start">{t('startTime')}</Label>
                  <Input
                    disabled={pending || values.delete_all}
                    id="usage-clear-start"
                    onChange={(event) => field.handleChange(event.target.value)}
                    type="datetime-local"
                    value={field.state.value}
                  />
                </div>
              )}
            </form.Field>
            <form.Field name="end_at">
              {(field) => (
                <div className="grid gap-1">
                  <Label htmlFor="usage-clear-end">{t('endTime')}</Label>
                  <Input
                    disabled={pending || values.delete_all}
                    id="usage-clear-end"
                    onChange={(event) => field.handleChange(event.target.value)}
                    type="datetime-local"
                    value={field.state.value}
                  />
                </div>
              )}
            </form.Field>
          </div>

          <div className="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 leading-5 text-destructive">
            {t('clearHistoryWarning')}
          </div>

          <div className="flex justify-end gap-2 pt-1">
            <Button
              disabled={pending}
              onClick={() => onOpenChange(false)}
              size="sm"
              type="button"
              variant="outline"
            >
              {t('cancel')}
            </Button>
            <Button
              disabled={pending}
              size="sm"
              type="submit"
              variant="destructive"
            >
              {clearing && (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              )}
              <Trash2 aria-hidden="true" className="size-4" />
              {t('clearHistorySubmit')}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
