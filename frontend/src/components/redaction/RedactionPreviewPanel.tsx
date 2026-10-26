import { useEffect, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { Check, Clipboard, Eye, LoaderCircle } from 'lucide-react'
import type {
  AppliedReplacementSchema,
  RedactionFindingSchema,
} from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import { copyText } from '@/lib/clipboard'
import { useLocale } from '@/lib/i18n'
import {
  redactionInputSegments,
  redactionOutputSegments,
} from '@/lib/redaction-preview'
import { useRedactionStore } from '@/lib/redaction'
import { createRedactionWorkspaceView } from '@/models/redaction'

type RedactionPreviewPanelProps = {
  onRunPreview: () => void
  previewing: boolean
}

type PreviewPane = 'input' | 'output'

export function RedactionPreviewPanel({
  onRunPreview,
  previewing,
}: RedactionPreviewPanelProps) {
  const { t } = useLocale()
  const config = useRedactionStore((state) => state.config)
  const previewText = useRedactionStore((state) => state.previewText)
  const previewInputKind = useRedactionStore((state) => state.previewInputKind)
  const previewResult = useRedactionStore((state) => state.previewResult)
  const findings = previewResult?.findings
  const [activeFindingIndex, setActiveFindingIndex] = useState<number | null>(
    null,
  )
  const [activePane, setActivePane] = useState<PreviewPane>('input')
  const [copied, setCopied] = useState(false)
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const selectedFinding: RedactionFindingSchema | null =
    activeFindingIndex === null
      ? null
      : (findings?.[activeFindingIndex] ?? null)
  const workspace = useMemo(
    () =>
      createRedactionWorkspaceView({
        config,
        previewResult: previewResult ?? null,
        t: (key) => t(key),
      }),
    [config, previewResult, t],
  )
  const inputSegments = useMemo(
    () => redactionInputSegments(previewText, selectedFinding),
    [previewText, selectedFinding],
  )
  const outputSegments = useMemo(
    () =>
      redactionOutputSegments(
        previewResult?.redacted_text ?? '',
        selectedFinding,
      ),
    [previewResult?.redacted_text, selectedFinding],
  )

  useEffect(() => {
    setActiveFindingIndex(null)
    setActivePane('input')
    setCopied(false)
    if (copyTimer.current !== null) clearTimeout(copyTimer.current)
    copyTimer.current = null
  }, [findings])

  useEffect(
    () => () => {
      if (copyTimer.current !== null) clearTimeout(copyTimer.current)
    },
    [],
  )

  function setInputKind(value: string): void {
    if (value !== 'text' && value !== 'git_diff') return
    useRedactionStore.getState().setPreviewInputKind(value)
  }

  function selectFinding(index: number): void {
    setActiveFindingIndex(index)
    setActivePane('output')
  }

  async function copyOutput(): Promise<void> {
    const text = previewResult?.redacted_text ?? ''
    if (!text) return
    try {
      await copyText(text)
      setCopied(true)
      if (copyTimer.current !== null) clearTimeout(copyTimer.current)
      copyTimer.current = setTimeout(() => {
        setCopied(false)
        copyTimer.current = null
      }, 1500)
    } catch {
      setCopied(false)
    }
  }

  function findingColumns(): ReactNode {
    if (!findings?.length) {
      return (
        <TableRow>
          <TableCell className="text-muted-foreground" colSpan={4}>
            {t('noFindings')}
          </TableCell>
        </TableRow>
      )
    }
    return findings.map((finding, index) => (
      <TableRow
        data-finding-index={index}
        key={`${finding.kind}-${finding.start}-${finding.end}-${index}`}
      >
        <TableCell className="break-words">{finding.kind}</TableCell>
        <TableCell className="break-words">{finding.source}</TableCell>
        <TableCell>{finding.confidence}</TableCell>
        <TableCell className="max-w-72 whitespace-normal break-all">
          <button
            aria-pressed={activeFindingIndex === index}
            className="rounded px-0.5 text-left hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => selectFinding(index)}
            type="button"
          >
            {finding.match_text}
          </button>
        </TableCell>
      </TableRow>
    ))
  }

  function replacementRows(): ReactNode {
    const replacements: AppliedReplacementSchema[] =
      previewResult?.applied_replacements ?? []
    if (!replacements.length) {
      return (
        <TableRow>
          <TableCell className="text-muted-foreground" colSpan={4}>
            {t('noReplacements')}
          </TableCell>
        </TableRow>
      )
    }
    return replacements.map((replacement, index) => (
      <TableRow key={`${replacement.kind}-${index}`}>
        <TableCell className="break-words">{replacement.kind}</TableCell>
        <TableCell className="max-w-40 whitespace-normal break-all">
          {replacement.replacement}
        </TableCell>
        <TableCell className="max-w-56 whitespace-normal break-all">
          {replacement.display_value ?? '-'}
        </TableCell>
        <TableCell className="break-words">{replacement.strategy}</TableCell>
      </TableRow>
    ))
  }

  return (
    <div className="grid min-w-0 gap-4" data-redaction-preview-panel>
      <section className="grid min-w-0 gap-3 rounded-lg border p-4">
        <div className="flex min-w-0 flex-wrap items-end justify-between gap-3">
          <label className="grid min-w-0 gap-2 sm:w-64">
            <span className="text-sm text-muted-foreground">
              {t('inputKind')}
            </span>
            <Select onValueChange={setInputKind} value={previewInputKind}>
              <SelectTrigger
                aria-label={t('inputKind')}
                className="w-full"
                id="redaction-input-kind"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {workspace.input_kind_options.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <Button disabled={previewing} onClick={onRunPreview} type="button">
            {previewing ? (
              <LoaderCircle aria-hidden="true" className="animate-spin" />
            ) : (
              <Eye aria-hidden="true" />
            )}
            {t('preview')}
          </Button>
        </div>

        {previewResult && (
          <div className="flex flex-wrap gap-2">
            {workspace.preview_stats.map((stat) => (
              <Badge key={stat.label} variant="outline">
                {stat.label} {stat.value}
              </Badge>
            ))}
          </div>
        )}

        <div
          aria-label={t('redactionPreview')}
          className="hidden gap-2 max-[767px]:flex"
          role="tablist"
        >
          <button
            aria-pressed={activePane === 'input'}
            className={`flex-1 rounded-full border px-2.5 py-1 text-xs ${activePane === 'input' ? 'border-primary bg-accent text-primary' : 'border-border text-muted-foreground'}`}
            onClick={() => setActivePane('input')}
            role="tab"
            type="button"
          >
            {t('redactionInput')}
          </button>
          <button
            aria-pressed={activePane === 'output'}
            className={`flex-1 rounded-full border px-2.5 py-1 text-xs ${activePane === 'output' ? 'border-primary bg-accent text-primary' : 'border-border text-muted-foreground'}`}
            onClick={() => setActivePane('output')}
            role="tab"
            type="button"
          >
            {t('redactionOutput')}
          </button>
        </div>

        <div className="grid min-w-0 items-start gap-4 md:grid-cols-2">
          <div
            className={`grid min-w-0 gap-2 ${activePane === 'input' ? '' : 'max-[767px]:hidden'}`}
          >
            <span className="text-sm text-muted-foreground">
              {t('redactionInput')}
            </span>
            {selectedFinding ? (
              <div
                aria-label={t('redactionInput')}
                aria-readonly="true"
                className="min-h-52 w-full overflow-auto whitespace-pre-wrap rounded-md border border-input bg-transparent px-3 py-2 font-mono text-sm leading-6"
                id="redaction-preview-input"
                role="textbox"
              >
                {inputSegments.map((segment, index) => (
                  <span
                    data-preview-highlight={segment.highlighted || undefined}
                    className={
                      segment.highlighted
                        ? 'rounded bg-warning/20 px-0.5 text-warning'
                        : undefined
                    }
                    key={`input-${index}`}
                  >
                    {segment.text}
                  </span>
                ))}
              </div>
            ) : (
              <Textarea
                aria-label={t('redactionInput')}
                id="redaction-preview-input"
                name="redaction-preview-input"
                onChange={(event) =>
                  useRedactionStore
                    .getState()
                    .setPreviewText(event.currentTarget.value)
                }
                rows={7}
                value={previewText}
              />
            )}
          </div>

          <div
            className={`grid min-w-0 gap-2 ${activePane === 'output' ? '' : 'max-[767px]:hidden'}`}
          >
            <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
              <span className="text-sm text-muted-foreground">
                {t('redactionOutput')}
              </span>
              {previewResult && (
                <Button
                  aria-label={t('copyOutput')}
                  onClick={() => void copyOutput()}
                  size="sm"
                  type="button"
                  variant="ghost"
                >
                  {copied ? (
                    <Check aria-hidden="true" />
                  ) : (
                    <Clipboard aria-hidden="true" />
                  )}
                  {copied ? t('copied') : t('copy')}
                </Button>
              )}
            </div>
            {previewResult?.stats.llm_request_failed &&
              previewResult.stats.llm_error && (
                <div
                  className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
                  role="alert"
                >
                  {previewResult.stats.llm_error}
                </div>
              )}
            {previewResult ? (
              <div
                className="min-h-52 w-full overflow-auto whitespace-pre-wrap break-words rounded-md border border-input bg-transparent px-3 py-2 font-mono text-sm leading-6"
                id="redaction-preview-output"
              >
                {outputSegments.map((segment, index) => (
                  <span
                    data-preview-highlight={segment.highlighted || undefined}
                    className={
                      segment.highlighted
                        ? 'rounded bg-warning/20 px-0.5 text-warning'
                        : undefined
                    }
                    key={`output-${index}`}
                  >
                    {segment.text}
                  </span>
                ))}
              </div>
            ) : (
              <div
                className="flex min-h-44 items-center justify-center rounded-md border border-dashed border-border p-6 text-center text-sm text-muted-foreground"
                id="redaction-preview-output"
              >
                {t('preview')}
              </div>
            )}
          </div>
        </div>
      </section>

      <section className="grid min-w-0 gap-4">
        <h2 className="m-0 text-base font-semibold">
          {t('redactionPreviewDetails')}
        </h2>
        <section className="grid min-w-0 gap-2">
          <h3 className="m-0 text-sm font-semibold">{t('findings')}</h3>
          <Table className="min-w-[480px]">
            <TableHeader>
              <TableRow>
                <TableHead>{t('type')}</TableHead>
                <TableHead>{t('sourceField')}</TableHead>
                <TableHead>{t('scoreField')}</TableHead>
                <TableHead>{t('pattern')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>{findingColumns()}</TableBody>
          </Table>
        </section>

        <section className="grid min-w-0 gap-2">
          <h3 className="m-0 text-sm font-semibold">{t('replacements')}</h3>
          <Table className="min-w-[480px]">
            <TableHeader>
              <TableRow>
                <TableHead>{t('type')}</TableHead>
                <TableHead>{t('replacementField')}</TableHead>
                <TableHead>{t('hintField')}</TableHead>
                <TableHead>{t('strategyField')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>{replacementRows()}</TableBody>
          </Table>
        </section>
      </section>
    </div>
  )
}
