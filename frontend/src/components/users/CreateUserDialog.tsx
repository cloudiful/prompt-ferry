import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useLocale } from '@/lib/i18n'
import type { NewUserForm } from '@/models/users'

type CreateUserDialogProps = {
  form: NewUserForm
  onChange: (field: keyof NewUserForm, value: string | boolean) => void
  onOpenChange: (open: boolean) => void
  onSubmit: () => void
  open: boolean
  pending: boolean
}

export function CreateUserDialog({
  form,
  onChange,
  onOpenChange,
  onSubmit,
  open,
  pending,
}: CreateUserDialogProps) {
  const { t } = useLocale()

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
          <DialogTitle>{t('createUser')}</DialogTitle>
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
            <div className="grid gap-2">
              <Label htmlFor="user-create-login-name">{t('loginName')}</Label>
              <Input
                autoFocus
                id="user-create-login-name"
                onChange={(event) =>
                  onChange('login_name', event.currentTarget.value)
                }
                placeholder={t('userLoginPlaceholder')}
                value={form.login_name}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="user-create-display-name">
                {t('displayName')}
              </Label>
              <Input
                id="user-create-display-name"
                onChange={(event) =>
                  onChange('display_name', event.currentTarget.value)
                }
                value={form.display_name}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="user-create-password">
                {t('userInitialPassword')}
              </Label>
              <Input
                id="user-create-password"
                onChange={(event) =>
                  onChange('password', event.currentTarget.value)
                }
                type="password"
                value={form.password}
              />
            </div>
            <Label className="min-h-8" htmlFor="user-create-admin">
              <Checkbox
                checked={form.is_admin}
                id="user-create-admin"
                onCheckedChange={(checked) =>
                  onChange('is_admin', checked === true)
                }
              />
              {t('admin')}
            </Label>
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
              {t('create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
