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

export function DeleteRuleConfirmDialog({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
}) {
  const { t } = useLocale()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby="redaction-delete-rule-description">
        <DialogHeader>
          <DialogTitle>{t('deleteRuleConfirmTitle')}</DialogTitle>
          <DialogDescription id="redaction-delete-rule-description">
            {t('deleteRuleConfirmBody')}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            onClick={() => onOpenChange(false)}
            type="button"
            variant="outline"
          >
            {t('deleteRuleCancel')}
          </Button>
          <Button onClick={onConfirm} type="button" variant="destructive">
            {t('deleteRuleConfirmSubmit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
