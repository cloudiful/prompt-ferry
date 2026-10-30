import type { ComponentProps } from 'react'
import { Copy, LoaderCircle, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useLocale } from '@/lib/i18n'

type KeyDialogProps = {
  generatedSecret: string
  keyLabel: string
  onCloseAutoFocus?: ComponentProps<typeof DialogContent>['onCloseAutoFocus']
  onCopySecret: () => void
  onKeyLabelChange: (value: string) => void
  onOpenChange: (open: boolean) => void
  onSubmit: () => void
  open: boolean
  pending: boolean
}

export function KeyDialog({
  generatedSecret,
  keyLabel,
  onCloseAutoFocus,
  onCopySecret,
  onKeyLabelChange,
  onOpenChange,
  onSubmit,
  open,
  pending,
}: KeyDialogProps) {
  const { t } = useLocale()

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent onCloseAutoFocus={onCloseAutoFocus} showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>{t('generateKey')}</DialogTitle>
        </DialogHeader>
        <DialogClose asChild>
          <Button
            aria-label={t('cancel')}
            className="absolute right-4 top-4"
            disabled={pending}
            size="icon-sm"
            type="button"
            variant="ghost"
          >
            <X aria-hidden="true" />
          </Button>
        </DialogClose>
        <form
          className="grid gap-4"
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            onSubmit()
          }}
        >
          {generatedSecret ? (
            <div className="grid min-w-0 gap-3">
              <code className="max-h-40 overflow-auto break-all rounded-md bg-muted p-3 text-sm">
                {generatedSecret}
              </code>
              <Button
                className="justify-self-start"
                onClick={onCopySecret}
                type="button"
                variant="outline"
              >
                <Copy aria-hidden="true" />
                <span>{t('copy')}</span>
              </Button>
            </div>
          ) : (
            <div className="grid gap-2">
              <Label htmlFor="client-key-label">{t('keyLabel')}</Label>
              <Input
                autoFocus
                disabled={pending}
                id="client-key-label"
                onChange={(event) =>
                  onKeyLabelChange(event.currentTarget.value)
                }
                value={keyLabel}
              />
            </div>
          )}
          {!generatedSecret && (
            <DialogFooter>
              <Button disabled={pending} type="submit">
                {pending && (
                  <LoaderCircle
                    aria-hidden="true"
                    className="size-4 animate-spin"
                  />
                )}
                <span>{t('generateKey')}</span>
              </Button>
            </DialogFooter>
          )}
        </form>
      </DialogContent>
    </Dialog>
  )
}
