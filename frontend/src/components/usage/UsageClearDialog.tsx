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
  usageClearScopeOptions,
  usageClearUserOptions,
  type UsageClearForm,
} from '@/lib/usage-clear'

type UsageClearDialogProps = {
  currentUserLabel: string
  form: UsageClearForm
  isAdmin: boolean
  onFormChange: (form: UsageClearForm) => void
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
 * The form belongs to the parent, which snapshots it before the action and
 * decides whether a completion may close this dialog or reset a newer draft.
 * A clear in flight freezes every dismissal path and only its own submit shows
 * a spinner; the category is never part of the payload.
 */
export function UsageClearDialog({
  currentUserLabel,
  form,
  isAdmin,
  onFormChange,
  onOpenChange,
  onSubmit,
  open,
  pending,
  pendingOrigin,
  users,
}: UsageClearDialogProps) {
  const { t } = useLocale()
  const scopeOptions = usageClearScopeOptions(isAdmin, {
    allUsers: t('clearAllRecords'),
    currentUser: t('clearCurrentUserHistory'),
    targetUser: t('clearSelectedUserHistory'),
  })
  const userOptions = usageClearUserOptions(users)
  const clearing = pendingOrigin === 'clear'

  function patch(next: Partial<UsageClearForm>): void {
    onFormChange({ ...form, ...next })
  }

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
          onSubmit={(event) => {
            event.preventDefault()
            if (pending) return
            onSubmit(form)
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
            <Select
              disabled={pending}
              onValueChange={(value) =>
                patch({ scope: value as UsageClearScope })
              }
              value={form.scope}
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
          </div>

          {form.scope === 'target_user' && (
            <div className="grid gap-1">
              <Label htmlFor="usage-clear-user">{t('selectUser')}</Label>
              <Select
                disabled={pending}
                onValueChange={(value) => patch({ user_id: Number(value) })}
                value={form.user_id === null ? '' : String(form.user_id)}
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
                    <SelectItem key={option.value} value={String(option.value)}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          {form.scope === 'current_user' && (
            <div className="rounded border bg-muted px-3 py-2 text-muted-foreground">
              {currentUserLabel}
            </div>
          )}

          <label className="inline-flex min-h-8 items-center gap-2 text-[0.75rem]">
            <Checkbox
              checked={form.delete_all}
              disabled={pending}
              id="usage-clear-all"
              onCheckedChange={(checked) =>
                patch({ delete_all: checked === true })
              }
            />
            {t('clearAllRecords')}
          </label>

          <div className="grid gap-3 sm:grid-cols-2">
            <div className="grid gap-1">
              <Label htmlFor="usage-clear-start">{t('startTime')}</Label>
              <Input
                disabled={pending || form.delete_all}
                id="usage-clear-start"
                onChange={(event) => patch({ start_at: event.target.value })}
                type="datetime-local"
                value={form.start_at}
              />
            </div>
            <div className="grid gap-1">
              <Label htmlFor="usage-clear-end">{t('endTime')}</Label>
              <Input
                disabled={pending || form.delete_all}
                id="usage-clear-end"
                onChange={(event) => patch({ end_at: event.target.value })}
                type="datetime-local"
                value={form.end_at}
              />
            </div>
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
