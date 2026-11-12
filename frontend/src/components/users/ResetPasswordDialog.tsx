import { useEffect } from 'react'
import { useForm } from '@tanstack/react-form'
import { LoaderCircle } from 'lucide-react'
import type { User } from '@/generated/admin-api'
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

type ResetPasswordDialogProps = {
  onOpenChange: (open: boolean) => void
  onSubmit: (password: string) => void
  open: boolean
  pending: boolean
  user: User | null
}

export function ResetPasswordDialog({
  onOpenChange,
  onSubmit,
  open,
  pending,
  user,
}: ResetPasswordDialogProps) {
  const { t } = useLocale()
  const inputId = `user-reset-password-${user?.user_id ?? 'target'}`

  const form = useForm({
    defaultValues: { password: '' },
    onSubmit: async ({ value }) => {
      onSubmit(value.password)
    },
  })

  useEffect(() => {
    if (open) form.reset({ password: '' })
  }, [open])

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
            event.stopPropagation()
            void form.handleSubmit()
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
              <form.Field name="password">
                {(field) => (
                  <Input
                    autoFocus
                    id={inputId}
                    onChange={(event) =>
                      field.handleChange(event.currentTarget.value)
                    }
                    type="password"
                    value={field.state.value}
                  />
                )}
              </form.Field>
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
