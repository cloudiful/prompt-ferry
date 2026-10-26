import { ChevronLeft, ChevronRight } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/i18n'

type WorkspacePagerBarProps = {
  canNext: boolean
  canPrev: boolean
  currentPage: number
  end: number
  onNext: () => void
  onPrev: () => void
  start: number
  total: number
}

export function WorkspacePagerBar({
  canNext,
  canPrev,
  currentPage,
  end,
  onNext,
  onPrev,
  start,
  total,
}: WorkspacePagerBarProps) {
  const { t } = useLocale()

  return (
    <nav
      aria-label={t('paginationPageCurrent', { page: currentPage })}
      className="flex items-center justify-between gap-3 rounded-xl border bg-card px-3 py-3"
    >
      <span className="text-xs text-muted-foreground">
        {start}-{end} / {total}
      </span>
      <div className="flex gap-2">
        <Button
          aria-label={t('paginationPrevious')}
          disabled={!canPrev}
          onClick={onPrev}
          size="icon-sm"
          type="button"
          variant="outline"
        >
          <ChevronLeft aria-hidden="true" />
        </Button>
        <Button
          aria-label={t('paginationNext')}
          disabled={!canNext}
          onClick={onNext}
          size="icon-sm"
          type="button"
          variant="outline"
        >
          <ChevronRight aria-hidden="true" />
        </Button>
      </div>
    </nav>
  )
}
