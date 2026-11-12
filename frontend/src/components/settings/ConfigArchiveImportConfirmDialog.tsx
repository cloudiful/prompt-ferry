import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useLocale } from '@/lib/i18n'

type ConfigArchiveImportConfirmDialogProps = {
  error: string
  onApply: () => void
  onCancel: () => void
  open: boolean
  pending: boolean
  unavailable: boolean
}

/**
 * The destructive confirmation for the import apply.
 *
 * Opening it is local only: no request is sent until the operator confirms, and
 * the warning states the irreversible replacement in the existing catalog copy.
 * The selected file, preview and passphrase are already held by the import form
 * and stay untouched while the dialog is open. While the apply is in flight the
 * dialog cannot be dismissed — not by the close button, Escape or an outside
 * click — so the answer always reaches a visible surface.
 */
export function ConfigArchiveImportConfirmDialog({
  error,
  onApply,
  onCancel,
  open,
  pending,
  unavailable,
}: ConfigArchiveImportConfirmDialogProps) {
  const { t } = useLocale()
  const blocked = pending || unavailable

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !pending) onCancel()
      }}
    >
      <DialogContent
        id="settings-config-archive-confirm"
        onEscapeKeyDown={(event) => {
          if (pending) event.preventDefault()
        }}
        onInteractOutside={(event) => {
          if (pending) event.preventDefault()
        }}
        showCloseButton={!pending}
      >
        <DialogHeader>
          <DialogTitle>{t('configArchiveImportConfirmTitle')}</DialogTitle>
          <DialogDescription>
            {t('configArchiveImportConfirmBody')}
          </DialogDescription>
        </DialogHeader>

        {error && !unavailable ? (
          <p
            className="m-0 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs leading-relaxed text-muted-foreground"
            id="settings-config-archive-confirm-error"
            role="alert"
          >
            {error}
          </p>
        ) : null}

        <DialogFooter>
          <Button
            disabled={blocked}
            id="settings-config-archive-confirm-cancel"
            onClick={onCancel}
            type="button"
            variant="outline"
          >
            {t('cancel')}
          </Button>
          <Button
            disabled={blocked}
            id="settings-config-archive-confirm-apply"
            onClick={onApply}
            type="button"
          >
            {t('configArchiveImportConfirmSubmit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
