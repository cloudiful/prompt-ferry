import { useMemo, useState } from 'react'
import { Eye, EyeOff, Plus, Trash2 } from 'lucide-react'
import type { RedactionCustomStringRuleRowSchema } from '@/generated/admin-api'
import { formatDateTime } from '@/format/datetime-core'
import { TablePagination } from '@/components/shared/TablePagination'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
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
import { useLocale } from '@/lib/i18n'
import {
  customStringRuleWarnings,
  useRedactionStore,
  visibleRedactionRows,
  visibleRedactionTotal,
} from '@/lib/redaction'
import { createRedactionWorkspaceView } from '@/models/redaction'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/models/pagination'
import { DeleteRuleConfirmDialog } from './DeleteRuleConfirmDialog'
import { RuleWarningBadge } from './RuleWarningBadge'

export function RedactionRulesPanel() {
  const { locale, t } = useLocale()
  const [deleteIndex, setDeleteIndex] = useState<number | null>(null)
  const config = useRedactionStore((state) => state.config)
  const customStringPage = useRedactionStore((state) => state.customStringPage)
  const customStringFirst = useRedactionStore(
    (state) => state.customStringFirst,
  )
  const customStringRows = useRedactionStore((state) => state.customStringRows)
  const customStringTotal = useRedactionStore(
    (state) => state.customStringTotal,
  )
  const customStringSearch = useRedactionStore(
    (state) => state.customStringSearch,
  )
  const customStringDirty = useRedactionStore(
    (state) => state.customStringDirty,
  )
  const customStringReveal = useRedactionStore(
    (state) => state.customStringReveal,
  )
  const validationError = useRedactionStore((state) => state.validationError)
  const pageError = useRedactionStore((state) => state.pageError)
  const warnings = useMemo(() => customStringRuleWarnings(config), [config])
  const rows = useMemo(
    () =>
      visibleRedactionRows({
        config,
        customStringDirty,
        customStringFirst,
        customStringRows,
        customStringSearch,
        customStringPage,
      }),
    [
      config,
      customStringDirty,
      customStringFirst,
      customStringRows,
      customStringSearch,
      customStringPage,
    ],
  )
  const total = useMemo(
    () =>
      visibleRedactionTotal({
        config,
        customStringDirty,
        customStringSearch,
        customStringTotal,
      }),
    [config, customStringDirty, customStringSearch, customStringTotal],
  )
  const workspace = useMemo(
    () =>
      createRedactionWorkspaceView({
        config,
        previewResult: null,
        t: (key) => t(key),
      }),
    [config, t],
  )
  const store = useRedactionStore.getState()
  const pendingDelete = deleteIndex === null ? null : { index: deleteIndex }

  function updatePattern(arrayIndex: number, pattern: string): void {
    store.updateCustomStringRule(arrayIndex, { pattern })
  }

  function changeMatchType(arrayIndex: number, value: string): void {
    if (value !== 'exact' && value !== 'contains' && value !== 'regex') return
    store.updateCustomStringRule(arrayIndex, { match_type: value })
  }

  function changeScope(arrayIndex: number, value: string): void {
    if (value !== 'text' && value !== 'line') return
    store.updateCustomStringRule(arrayIndex, { scope: value })
  }

  function confirmDelete(): void {
    if (deleteIndex !== null) store.removeCustomStringRule(deleteIndex)
    setDeleteIndex(null)
  }

  function requestPage(first: number, pageRows: number): void {
    void store.setCustomStringPage(first, pageRows).catch(() => undefined)
  }

  function updateSearch(search: string): void {
    void store.setCustomStringSearch(search).catch(() => undefined)
  }

  function renderRule(row: RedactionCustomStringRuleRowSchema) {
    const arrayIndex = row.array_index
    const warning = warnings[arrayIndex] ?? {
      duplicate: false,
      conflict: false,
      invalidRegex: false,
    }
    const revealed = customStringReveal.has(arrayIndex)
    const errorId = `redaction-invalid-regex-${arrayIndex}`

    return (
      <TableRow key={arrayIndex} data-redaction-array-index={arrayIndex}>
        <TableCell className="min-w-64">
          <div className="flex min-w-56 items-center gap-1">
            <Input
              aria-describedby={warning.invalidRegex ? errorId : undefined}
              aria-invalid={warning.invalidRegex || undefined}
              aria-label={`${t('pattern')} ${arrayIndex + 1}`}
              autoComplete="off"
              className="font-mono"
              name="redaction-custom-pattern"
              onChange={(event) =>
                updatePattern(arrayIndex, event.currentTarget.value)
              }
              type={revealed ? 'text' : 'password'}
              value={row.pattern}
            />
            <Button
              aria-label={`${t(revealed ? 'hidePlaintext' : 'showPlaintext')} ${arrayIndex + 1}`}
              onClick={() => store.toggleCustomStringReveal(arrayIndex)}
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              {revealed ? (
                <EyeOff aria-hidden="true" />
              ) : (
                <Eye aria-hidden="true" />
              )}
            </Button>
          </div>
          {warning.invalidRegex && (
            <span className="sr-only" id={errorId} role="alert">
              {t('ruleInvalidRegex')}
            </span>
          )}
        </TableCell>
        <TableCell>
          <Select
            onValueChange={(value) => changeMatchType(arrayIndex, value)}
            value={row.match_type}
          >
            <SelectTrigger
              aria-label={`${t('matchType')} ${arrayIndex + 1}`}
              className="min-w-28"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {workspace.match_type_options.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </TableCell>
        <TableCell>
          <Select
            onValueChange={(value) => changeScope(arrayIndex, value)}
            value={row.scope}
          >
            <SelectTrigger
              aria-label={`${t('scopeField')} ${arrayIndex + 1}`}
              className="min-w-24"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {workspace.scope_options.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </TableCell>
        <TableCell>{formatDateTime(row.created_at, locale)}</TableCell>
        <TableCell>{formatDateTime(row.updated_at, locale)}</TableCell>
        <TableCell className="min-w-44 whitespace-normal">
          <RuleWarningBadge warning={warning} />
        </TableCell>
        <TableCell>
          <Button
            aria-label={`${t('deleteRuleConfirmSubmit')} ${arrayIndex + 1}`}
            onClick={() => setDeleteIndex(arrayIndex)}
            size="icon-sm"
            type="button"
            variant="ghost"
          >
            <Trash2 aria-hidden="true" />
          </Button>
        </TableCell>
      </TableRow>
    )
  }

  return (
    <div className="grid min-w-0 gap-4">
      <section
        aria-labelledby="redaction-built-in-heading"
        className="grid gap-3"
      >
        <h2
          className="m-0 text-base font-semibold"
          id="redaction-built-in-heading"
        >
          {t('redactionRules')}
        </h2>
        <div className="grid grid-cols-1 gap-x-5 gap-y-2 sm:grid-cols-2 lg:grid-cols-3">
          {workspace.rule_options.map((option) => (
            <label
              className="flex min-h-9 items-center gap-2 rounded-md border px-3 text-sm"
              key={option.key}
            >
              <Checkbox
                aria-label={option.label}
                checked={config.rules[option.key]}
                onCheckedChange={(checked) =>
                  store.setBuiltinRule(option.key, checked === true)
                }
              />
              <span>{option.label}</span>
            </label>
          ))}
        </div>
      </section>

      <section
        aria-labelledby="redaction-custom-strings-heading"
        className="grid min-w-0 gap-3"
      >
        <div className="flex min-w-0 flex-wrap items-end justify-between gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <h2
              className="m-0 text-base font-semibold"
              id="redaction-custom-strings-heading"
            >
              {t('customStrings')}
            </h2>
            {customStringDirty && (
              <span className="rounded border border-warning/40 bg-warning/10 px-1.5 py-0.5 text-xs text-warning">
                {t('ruleDirtyBadge')}
              </span>
            )}
          </div>
          <div className="flex min-w-0 flex-wrap items-end gap-2">
            <label className="grid min-w-40 gap-1 text-xs text-muted-foreground">
              <span>{t('pattern')}</span>
              <Input
                aria-label={t('pattern')}
                autoComplete="off"
                onChange={(event) => updateSearch(event.currentTarget.value)}
                type="search"
                value={customStringSearch}
              />
            </label>
            <Button
              onClick={() => store.addCustomStringRule()}
              type="button"
              variant="outline"
            >
              <Plus aria-hidden="true" />
              {t('addRule')}
            </Button>
          </div>
        </div>

        {validationError && (
          <p
            className="m-0 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
            id="redaction-save-validation-error"
            role="alert"
          >
            {t('ruleInvalidRegex')}
          </p>
        )}
        {pageError && (
          <p className="m-0 text-sm text-destructive" role="alert">
            {t('failed')}
          </p>
        )}

        {rows.length === 0 ? (
          <p className="m-0 py-5 text-center text-sm text-muted-foreground">
            {t('noCustomStrings')}
          </p>
        ) : (
          <Table className="min-w-[900px]">
            <TableHeader>
              <TableRow>
                <TableHead>{t('pattern')}</TableHead>
                <TableHead>{t('matchType')}</TableHead>
                <TableHead>{t('scopeField')}</TableHead>
                <TableHead>{t('createdAt')}</TableHead>
                <TableHead>{t('updatedAt')}</TableHead>
                <TableHead>{t('ruleConflictHint')}</TableHead>
                <TableHead>{t('deleteRuleConfirmSubmit')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>{rows.map(renderRule)}</TableBody>
          </Table>
        )}

        <TablePagination
          first={customStringFirst}
          onChange={({ first, rows: pageRows }) => requestPage(first, pageRows)}
          pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
          rows={customStringRows}
          total={total}
        />
      </section>

      <DeleteRuleConfirmDialog
        onConfirm={confirmDelete}
        onOpenChange={(open) => {
          if (!open) setDeleteIndex(null)
        }}
        open={pendingDelete !== null}
      />
    </div>
  )
}
