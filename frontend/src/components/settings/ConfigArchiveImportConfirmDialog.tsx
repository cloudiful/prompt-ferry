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
import { useSettingsConfigArchiveStore } from '@/lib/config-archive'

/**
 * The destructive confirmation for the import apply.
 *
 * Opening it is local only: no request is sent until the operator confirms, and
 * the warning states the irreversible replacement in the existing catalog copy.
 * While the apply is in flight the dialog cannot be dismissed — not by the close
 * button, Escape or an outside click — so the answer always reaches a visible
 * surface.
 */
export function ConfigArchiveImportConfirmDialog() {
  const { t } = useLocale()
  const open = useSettingsConfigArchiveStore((state) => state.confirmOpen)
  const pending = useSettingsConfigArchiveStore((state) => state.importPending)
  const unavailable = useSettingsConfigArchiveStore(
    (state) => state.importUnavailable,
  )
  const error = useSettingsConfigArchiveStore((state) => state.importError)
  const confirmApply = useSettingsConfigArchiveStore(
    (state) => state.confirmApply,
  )
  const cancelApply = useSettingsConfigArchiveStore(
    (state) => state.cancelApply,
  )

  const blocked = pending || unavailable

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !pending) cancelApply()
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
            onClick={() => cancelApply()}
            type="button"
            variant="outline"
          >
            {t('cancel')}
          </Button>
          <Button
            disabled={blocked}
            id="settings-config-archive-confirm-apply"
            onClick={() => void confirmApply()}
            type="button"
          >
            {t('configArchiveImportConfirmSubmit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
