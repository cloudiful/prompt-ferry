import { useMemo, useState } from 'react'
import { useStore, type ReactFormExtendedApi } from '@tanstack/react-form'
import { Eye, EyeOff, Plus, Trash2 } from 'lucide-react'
import type {
  RedactionConfigSchema,
  RedactionCustomStringRuleRowSchema,
} from '@/generated/admin-api'
import { formatDateTime } from '@/format/datetime-core'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
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
import { useLocale } from '@/lib/i18n'
import { customStringRuleWarnings } from '@/lib/redaction'
import { createRedactionWorkspaceView } from '@/models/redaction'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/models/pagination'
import { DeleteRuleConfirmDialog } from './DeleteRuleConfirmDialog'
import { RuleWarningBadge } from './RuleWarningBadge'

export type RedactionRulesForm = ReactFormExtendedApi<
  RedactionConfigSchema,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined
>

type RedactionRulesPanelProps = {
  customStringDirty: boolean
  customStringFirst: number
  customStringRevealAll: boolean
  customStringRows: number
  customStringSearch: string
  form: RedactionRulesForm
  onAddRule: () => void
  onEditRule: () => void
  onPage: (first: number, rows: number) => void
  onRemoveRule: (arrayIndex: number) => void
  onSearch: (search: string) => void
  onToggleReveal: () => void
  pageError: boolean
  rows: RedactionCustomStringRuleRowSchema[]
  total: number
  validationError: boolean
}

export function RedactionRulesPanel({
  customStringDirty,
  customStringFirst,
  customStringRevealAll,
  customStringRows,
  customStringSearch,
  form,
  onAddRule,
  onEditRule,
  onPage,
  onRemoveRule,
  onSearch,
  onToggleReveal,
  pageError,
  rows,
  total,
  validationError,
}: RedactionRulesPanelProps) {
  const { locale, t } = useLocale()
  const [deleteIndex, setDeleteIndex] = useState<number | null>(null)
  const config = useStore(form.store, (state) => state.values)
  const warnings = useMemo(() => customStringRuleWarnings(config), [config])
  const workspace = useMemo(
    () =>
      createRedactionWorkspaceView({
        config,
        previewResult: null,
        t: (key) => t(key),
      }),
    [config, t],
  )

  function confirmDelete(): void {
    if (deleteIndex !== null) onRemoveRule(deleteIndex)
    setDeleteIndex(null)
  }

  const columns: Array<DataTableColumnDef<RedactionCustomStringRuleRowSchema>> =
    [
      {
        id: 'pattern',
        header: t('pattern'),
        cell: ({ row }) => {
          const arrayIndex = row.original.array_index
          const warning = warnings[arrayIndex] ?? {
            duplicate: false,
            conflict: false,
            invalidRegex: false,
          }
          const errorId = `redaction-invalid-regex-${arrayIndex}`
          return (
            <div className="min-w-64">
              <form.Field name={`custom_strings[${arrayIndex}].pattern`}>
                {(field) => (
                  <Input
                    aria-describedby={
                      warning.invalidRegex ? errorId : undefined
                    }
                    aria-invalid={warning.invalidRegex || undefined}
                    aria-label={`${t('pattern')} ${arrayIndex + 1}`}
                    autoComplete="off"
                    className="font-mono"
                    name="redaction-custom-pattern"
                    onChange={(event) => {
                      field.handleChange(event.currentTarget.value)
                      onEditRule()
                    }}
                    type={customStringRevealAll ? 'text' : 'password'}
                    value={field.state.value}
                  />
                )}
              </form.Field>
              {warning.invalidRegex && (
                <span className="sr-only" id={errorId} role="alert">
                  {t('ruleInvalidRegex')}
                </span>
              )}
            </div>
          )
        },
      },
      {
        id: 'matchType',
        header: t('matchType'),
        cell: ({ row }) => {
          const arrayIndex = row.original.array_index
          return (
            <form.Field name={`custom_strings[${arrayIndex}].match_type`}>
              {(field) => (
                <Select
                  onValueChange={(value) => {
                    if (
                      value !== 'exact' &&
                      value !== 'contains' &&
                      value !== 'regex'
                    )
                      return
                    field.handleChange(value)
                    onEditRule()
                  }}
                  value={field.state.value}
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
              )}
            </form.Field>
          )
        },
      },
      {
        id: 'scope',
        header: t('scopeField'),
        cell: ({ row }) => {
          const arrayIndex = row.original.array_index
          return (
            <form.Field name={`custom_strings[${arrayIndex}].scope`}>
              {(field) => (
                <Select
                  onValueChange={(value) => {
                    if (value !== 'text' && value !== 'line') return
                    field.handleChange(value)
                    onEditRule()
                  }}
                  value={field.state.value}
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
              )}
            </form.Field>
          )
        },
      },
      {
        id: 'createdAt',
        header: t('createdAt'),
        cell: ({ row }) => formatDateTime(row.original.created_at, locale),
      },
      {
        id: 'updatedAt',
        header: t('updatedAt'),
        cell: ({ row }) => formatDateTime(row.original.updated_at, locale),
      },
      {
        id: 'warnings',
        header: t('warnings'),
        cell: ({ row }) => (
          <div className="min-w-44 whitespace-normal">
            <RuleWarningBadge
              warning={
                warnings[row.original.array_index] ?? {
                  duplicate: false,
                  conflict: false,
                  invalidRegex: false,
                }
              }
            />
          </div>
        ),
      },
      {
        id: 'actions',
        header: () => <span className="sr-only">{t('actions')}</span>,
        cell: ({ row }) => (
          <Button
            aria-label={`${t('deleteRuleConfirmSubmit')} ${row.original.array_index + 1}`}
            onClick={() => setDeleteIndex(row.original.array_index)}
            size="icon-sm"
            type="button"
            variant="ghost"
          >
            <Trash2 aria-hidden="true" />
          </Button>
        ),
      },
    ]

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
            <form.Field key={option.key} name={`rules.${option.key}`}>
              {(field) => (
                <label className="flex min-h-9 items-center gap-2 rounded-md border px-3 text-sm">
                  <Checkbox
                    aria-label={option.label}
                    checked={field.state.value}
                    onCheckedChange={(checked) =>
                      field.handleChange(checked === true)
                    }
                  />
                  <span>{option.label}</span>
                </label>
              )}
            </form.Field>
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
            <Button
              aria-label={t(
                customStringRevealAll ? 'hideAllPlaintext' : 'showAllPlaintext',
              )}
              aria-pressed={customStringRevealAll}
              onClick={onToggleReveal}
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              {customStringRevealAll ? (
                <EyeOff aria-hidden="true" />
              ) : (
                <Eye aria-hidden="true" />
              )}
            </Button>
          </div>
          <div className="flex min-w-0 flex-wrap items-end gap-2">
            <label className="grid min-w-40 gap-1 text-xs text-muted-foreground">
              <span>{t('pattern')}</span>
              <Input
                aria-label={t('pattern')}
                autoComplete="off"
                onChange={(event) => onSearch(event.currentTarget.value)}
                type="search"
                value={customStringSearch}
              />
            </label>
            <Button onClick={onAddRule} type="button" variant="outline">
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
          <DataTable
            className="min-w-[900px]"
            columns={columns}
            data={rows}
            getRowId={(row) => String(row.array_index)}
            label={t('customStrings')}
            rowAttributes={(row) => ({
              'data-redaction-array-index': row.array_index,
            })}
          />
        )}

        <TablePagination
          first={customStringFirst}
          onChange={({ first: nextFirst, rows: pageRows }) =>
            onPage(nextFirst, pageRows)
          }
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
        open={deleteIndex !== null}
      />
    </div>
  )
}
