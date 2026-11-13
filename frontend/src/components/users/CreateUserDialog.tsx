import { useForm } from '@tanstack/react-form'
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
import { emptyUserForm, type NewUserForm } from '@/models/users'

type CreateUserDialogProps = {
  onOpenChange: (open: boolean) => void
  onSubmit: (form: NewUserForm) => Promise<boolean> | boolean
  open: boolean
  pending: boolean
}

export function CreateUserDialog({
  onOpenChange,
  onSubmit,
  open,
  pending,
}: CreateUserDialogProps) {
  const { t } = useLocale()

  const form = useForm({
    defaultValues: emptyUserForm(),
    onSubmit: async ({ value }) => {
      const saved = await onSubmit(value)
      if (saved) form.reset(emptyUserForm())
    },
  })

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
            event.stopPropagation()
            void form.handleSubmit()
          }}
        >
          <fieldset className="grid gap-3" disabled={pending}>
            <div className="grid gap-2">
              <Label htmlFor="user-create-login-name">{t('loginName')}</Label>
              <form.Field name="login_name">
                {(field) => (
                  <Input
                    autoFocus
                    id="user-create-login-name"
                    onChange={(event) =>
                      field.handleChange(event.currentTarget.value)
                    }
                    placeholder={t('userLoginPlaceholder')}
                    value={field.state.value}
                  />
                )}
              </form.Field>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="user-create-display-name">
                {t('displayName')}
              </Label>
              <form.Field name="display_name">
                {(field) => (
                  <Input
                    id="user-create-display-name"
                    onChange={(event) =>
                      field.handleChange(event.currentTarget.value)
                    }
                    value={field.state.value}
                  />
                )}
              </form.Field>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="user-create-password">
                {t('userInitialPassword')}
              </Label>
              <form.Field name="password">
                {(field) => (
                  <Input
                    id="user-create-password"
                    onChange={(event) =>
                      field.handleChange(event.currentTarget.value)
                    }
                    type="password"
                    value={field.state.value}
                  />
                )}
              </form.Field>
            </div>
            <Label className="min-h-8" htmlFor="user-create-admin">
              <form.Field name="is_admin">
                {(field) => (
                  <Checkbox
                    checked={field.state.value}
                    id="user-create-admin"
                    onCheckedChange={(checked) =>
                      field.handleChange(checked === true)
                    }
                  />
                )}
              </form.Field>
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
