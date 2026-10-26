import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useLocale } from '@/lib/i18n'
import type { User } from '@/generated/admin-api'

type ResetPasswordDialogProps = {
  onOpenChange: (open: boolean) => void
  onPasswordChange: (value: string) => void
  onSubmit: () => void
  open: boolean
  password: string
  pending: boolean
  user: User | null
}

export function ResetPasswordDialog({
  onOpenChange,
  onPasswordChange,
  onSubmit,
  open,
  password,
  pending,
  user,
}: ResetPasswordDialogProps) {
  const { t } = useLocale()
  const inputId = `user-reset-password-${user?.user_id ?? 'target'}`

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        onEscapeKeyDown={(event) => {
          if (pending) event.preventDefault()
        }}
        onPointerDownOutside={(event) => {
          if (pending) event.preventDefault()
        }}
        showCloseButton={false}
      >
        <DialogHeader>
          <DialogTitle>{t('resetPassword')}</DialogTitle>
        </DialogHeader>
        <form
          className="grid gap-4"
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            onSubmit()
          }}
        >
          <fieldset className="grid gap-3" disabled={pending}>
            {user && (
              <div className="font-medium text-foreground">
                {user.login_name}
              </div>
            )}
            <div className="grid gap-2">
              <Label htmlFor={inputId}>{t('newPassword')}</Label>
              <Input
                autoFocus
                id={inputId}
                onChange={(event) =>
                  onPasswordChange(event.currentTarget.value)
                }
                type="password"
                value={password}
              />
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
              {pending && (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              )}
              {t('save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
