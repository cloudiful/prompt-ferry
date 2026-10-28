export const TARGET_ROW_WIDTHS = [1440, 1024, 768, 390] as const
export const TARGET_ROW_DIALOG_WIDTHS = [1024, 992, 736, 358] as const

type RecordValue = Record<string, unknown>

function isRecord(value: unknown): value is RecordValue {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value)
}

function isEmptyList(value: unknown): boolean {
  return Array.isArray(value) && value.length === 0
}

function hasExactWidths(value: unknown): boolean {
  return (
    Array.isArray(value) &&
    value.length === TARGET_ROW_WIDTHS.length &&
    TARGET_ROW_WIDTHS.every((width, index) => value[index] === width)
  )
}

function controlChecks(row: RecordValue): boolean {
  return (
    isEmptyList(row.overlapPairs) &&
    isEmptyList(row.outsideRow) &&
    isEmptyList(row.outsideDialog) &&
    row.fullNameInDom === true &&
    row.accessibleDescription === true &&
    row.labelWrapperMinWidth === true &&
    row.labelTruncated === true &&
    isFiniteNumber(row.triggerOverflow) &&
    row.triggerOverflow <= 1 &&
    row.modelValueIntact === true
  )
}

function dialogChecks(dialog: unknown): boolean {
  if (!isRecord(dialog)) return false
  const {
    dialogWidth,
    expectedDialogWidth,
    minMargin,
    minimumControlWidth,
    triggerWidth,
    modelWidth,
    documentNoOverflow,
  } = dialog
  if (
    !isFiniteNumber(dialogWidth) ||
    !isFiniteNumber(expectedDialogWidth) ||
    !isFiniteNumber(minMargin) ||
    !isFiniteNumber(minimumControlWidth) ||
    !isFiniteNumber(triggerWidth) ||
    !isFiniteNumber(modelWidth)
  )
    return false
  return (
    Math.abs(dialogWidth - expectedDialogWidth) <= 1 &&
    minMargin >= 15 &&
    documentNoOverflow === true &&
    triggerWidth >= minimumControlWidth - 1 &&
    modelWidth >= minimumControlWidth - 1 &&
    Math.abs(minimumControlWidth - Math.max(160, 0.22 * dialogWidth)) <= 0.01
  )
}

function expectedDialogWidths(rows: RecordValue[]): unknown[] {
  return rows.map((row) =>
    isRecord(row.dialog) ? row.dialog.expectedDialogWidth : undefined,
  )
}

function operabilityChecks(operability: unknown): boolean {
  if (!isRecord(operability)) return false
  return (
    operability.keyboardSelectOpened === true &&
    operability.modelEditable === true &&
    operability.toggleClicked === true &&
    operability.settingsOpenedAndClosed === true &&
    operability.singleTargetDeleteDisabled === true
  )
}

export function evaluateTargetRowLayout(evidence: unknown): string[] {
  const failures: string[] = []
  const layout = isRecord(evidence) ? evidence : null
  const rows = layout && Array.isArray(layout.rows) ? layout.rows : null
  const complete =
    rows !== null && rows.length === 4 && rows.every((row) => isRecord(row))
  const measured: RecordValue[] = complete ? (rows as RecordValue[]) : []

  if (!layout || !hasExactWidths(layout.widths) || !complete) {
    failures.push(
      'target row layout evidence is missing its four measured widths',
    )
  }

  if (
    !complete ||
    !measured.every((row, index) => row.width === TARGET_ROW_WIDTHS[index])
  ) {
    failures.push('target row measurement widths are missing or out of order')
  }

  if (!complete || !measured.every(controlChecks)) {
    failures.push(
      'target row controls overlapped, escaped their container, or lost their full endpoint name',
    )
  }

  const wideRows = measured.filter(
    (row) => isFiniteNumber(row.width) && row.width >= 768,
  )
  const narrowRows = measured.filter(
    (row) => isFiniteNumber(row.width) && row.width < 768,
  )
  if (
    !complete ||
    wideRows.length !== 3 ||
    !wideRows.every(
      (row) => row.stacked === false && row.horizontalOrderClear === true,
    )
  ) {
    failures.push(
      'wide target rows did not keep trigger/model/actions in one clear row',
    )
  }
  if (
    !complete ||
    narrowRows.length !== 1 ||
    !narrowRows.every((row) => row.stacked === true)
  ) {
    failures.push('narrow target rows did not stack their controls')
  }

  if (!complete || !measured.some((row) => row.labelClipped === true)) {
    failures.push('long selected endpoint label was never visually truncated')
  }

  if (!complete || !measured.every((row) => dialogChecks(row.dialog))) {
    failures.push(
      'dialog width, margins, or input widths did not meet the intended desktop layout',
    )
  }

  if (
    !complete ||
    JSON.stringify(expectedDialogWidths(measured)) !==
      JSON.stringify(TARGET_ROW_DIALOG_WIDTHS)
  ) {
    failures.push(
      'dialog width target did not follow min(64rem, viewport minus 2rem)',
    )
  }

  if (
    !complete ||
    !measured.some(
      (row) =>
        isRecord(row.dialog) &&
        isFiniteNumber(row.dialog.dialogWidth) &&
        row.dialog.dialogWidth > 512,
    )
  ) {
    failures.push(
      'desktop dialog never grew past the current narrow 512px layout',
    )
  }

  if (
    !layout ||
    layout.savedEndpointId !== 'ep-openai-codex' ||
    !isFiniteNumber(layout.savedModelLength) ||
    layout.savedModelLength <= 40 ||
    layout.savedModelUnicode !== true ||
    layout.createdRuleDeleted !== true
  ) {
    failures.push(
      'target row save did not preserve the long endpoint id and upstream model',
    )
  }

  if (!layout || !operabilityChecks(layout.operability)) {
    failures.push('target row controls were not operable after the layout fix')
  }

  return failures
}
