import { describe, expect, it } from 'vitest'
import {
  TARGET_ROW_DIALOG_WIDTHS,
  TARGET_ROW_WIDTHS,
  evaluateTargetRowLayout,
} from '../../e2e/foundation/validate-target-row'

type Json = Record<string, unknown>
type Row = Record<string, unknown>

const widthFailure =
  'target row layout evidence is missing its four measured widths'
const widthOrderFailure =
  'target row measurement widths are missing or out of order'
const controlFailure =
  'target row controls overlapped, escaped their container, or lost their full endpoint name'
const wideFailure =
  'wide target rows did not keep trigger/model/actions in one clear row'
const narrowFailure = 'narrow target rows did not stack their controls'
const clippedFailure =
  'long selected endpoint label was never visually truncated'
const dialogFailure =
  'dialog width, margins, or input widths did not meet the intended desktop layout'
const expectedDialogFailure =
  'dialog width target did not follow min(64rem, viewport minus 2rem)'
const growthFailure =
  'desktop dialog never grew past the current narrow 512px layout'
const saveFailure =
  'target row save did not preserve the long endpoint id and upstream model'
const operabilityFailure =
  'target row controls were not operable after the layout fix'

function validRow(
  width: number,
  dialogWidth: number,
  stacked: boolean,
  horizontalOrderClear: boolean,
): Row {
  const minimumControlWidth = Math.max(160, 0.22 * dialogWidth)
  return {
    width,
    overlapPairs: [],
    outsideRow: [],
    outsideDialog: [],
    stacked,
    horizontalOrderClear,
    fullNameInDom: true,
    accessibleDescription: true,
    labelWrapperMinWidth: true,
    labelTruncated: true,
    labelClipped: true,
    triggerOverflow: 0,
    modelValueIntact: true,
    dialog: {
      dialogWidth,
      expectedDialogWidth: dialogWidth,
      leftMargin: 16,
      rightMargin: 16,
      minMargin: 16,
      minimumControlWidth,
      triggerWidth: minimumControlWidth + 50,
      modelWidth: minimumControlWidth + 40,
      documentNoOverflow: true,
    },
  }
}

function validLayout(): Json {
  return {
    widths: [...TARGET_ROW_WIDTHS],
    rows: [
      validRow(1440, 1024, false, true),
      validRow(1024, 992, false, true),
      validRow(768, 736, false, true),
      validRow(390, 358, true, false),
    ],
    operability: {
      keyboardSelectOpened: true,
      modelEditable: true,
      toggleClicked: true,
      settingsOpenedAndClosed: true,
      singleTargetDeleteDisabled: true,
    },
    savedEndpointId: 'ep-openai-codex',
    savedModelLength: 45,
    savedModelUnicode: true,
    createdRuleDeleted: true,
  }
}

function rowsOf(layout: Json): Row[] {
  return layout.rows as Row[]
}

function dialogOf(row: Row): Row {
  return row.dialog as Row
}

function withLayout(mutate: (layout: Json) => void): unknown {
  const layout = structuredClone(validLayout())
  mutate(layout)
  return layout
}

describe('evaluateTargetRowLayout', () => {
  it('accepts a complete measured target-row report', () => {
    const layout = validLayout()
    expect(rowsOf(layout)).toHaveLength(4)
    expect(TARGET_ROW_DIALOG_WIDTHS).toEqual([1024, 992, 736, 358])
    expect(evaluateTargetRowLayout(layout)).toEqual([])
  })

  it('fails closed for missing or non-object evidence', () => {
    for (const evidence of [undefined, null, 'routes', 42, [], {}])
      expect(evaluateTargetRowLayout(evidence)).toContain(widthFailure)
  })

  it('rejects missing rows and missing top-level widths', () => {
    const missingRows = withLayout((layout) => {
      delete layout.rows
    })
    expect(evaluateTargetRowLayout(missingRows)).toContain(widthFailure)

    const missingWidths = withLayout((layout) => {
      delete layout.widths
    })
    expect(evaluateTargetRowLayout(missingWidths)).toContain(widthFailure)

    const emptyRows = withLayout((layout) => {
      layout.rows = []
    })
    expect(evaluateTargetRowLayout(emptyRows)).toContain(widthFailure)
  })

  it('rejects duplicate or missing measured widths and row width order drift', () => {
    const duplicate = withLayout((layout) => {
      ;(layout.widths as number[])[1] = 1440
    })
    expect(evaluateTargetRowLayout(duplicate)).toContain(widthFailure)

    const missingWidth = withLayout((layout) => {
      ;(layout.widths as number[]).splice(2, 1)
    })
    expect(evaluateTargetRowLayout(missingWidth)).toContain(widthFailure)

    const rowMismatch = withLayout((layout) => {
      rowsOf(layout)[1]!.width = 900
    })
    expect(evaluateTargetRowLayout(rowMismatch)).toContain(widthOrderFailure)

    const rowDuplicate = withLayout((layout) => {
      rowsOf(layout)[2]!.width = 1024
    })
    expect(evaluateTargetRowLayout(rowDuplicate)).toContain(widthOrderFailure)

    const rowNonFinite = withLayout((layout) => {
      rowsOf(layout)[0]!.width = Number.NaN
    })
    expect(evaluateTargetRowLayout(rowNonFinite)).toContain(widthOrderFailure)
  })

  it('rejects overlapping or escaping controls', () => {
    const overlap = withLayout((layout) => {
      rowsOf(layout)[0]!.overlapPairs = ['trigger/model']
    })
    expect(evaluateTargetRowLayout(overlap)).toContain(controlFailure)

    const malformedOverlap = withLayout((layout) => {
      rowsOf(layout)[0]!.overlapPairs = 'none'
    })
    expect(evaluateTargetRowLayout(malformedOverlap)).toContain(controlFailure)

    const rowEscape = withLayout((layout) => {
      rowsOf(layout)[2]!.outsideRow = ['trigger']
    })
    expect(evaluateTargetRowLayout(rowEscape)).toContain(controlFailure)

    const dialogEscape = withLayout((layout) => {
      rowsOf(layout)[3]!.outsideDialog = ['settings']
    })
    expect(evaluateTargetRowLayout(dialogEscape)).toContain(controlFailure)
  })

  it('rejects lost names, accessibility, shrink evidence or model values', () => {
    const mutations: Array<(row: Row) => void> = [
      (row) => {
        row.fullNameInDom = false
      },
      (row) => {
        row.accessibleDescription = false
      },
      (row) => {
        row.labelWrapperMinWidth = false
      },
      (row) => {
        row.labelTruncated = false
      },
      (row) => {
        row.triggerOverflow = 2
      },
      (row) => {
        row.triggerOverflow = Number.POSITIVE_INFINITY
      },
      (row) => {
        row.modelValueIntact = false
      },
    ]
    for (const mutate of mutations) {
      const layout = withLayout((draft) => mutate(rowsOf(draft)[0]!))
      expect(evaluateTargetRowLayout(layout)).toContain(controlFailure)
    }
  })

  it('rejects wide-row order and narrow-row stacking violations', () => {
    const wideStacked = withLayout((layout) => {
      rowsOf(layout)[0]!.stacked = true
    })
    expect(evaluateTargetRowLayout(wideStacked)).toContain(wideFailure)

    const wideOrder = withLayout((layout) => {
      rowsOf(layout)[1]!.horizontalOrderClear = false
    })
    expect(evaluateTargetRowLayout(wideOrder)).toContain(wideFailure)

    const narrowUnstacked = withLayout((layout) => {
      rowsOf(layout)[3]!.stacked = false
    })
    expect(evaluateTargetRowLayout(narrowUnstacked)).toContain(narrowFailure)
  })

  it('rejects a report where no long endpoint label was clipped', () => {
    const layout = withLayout((draft) => {
      for (const row of rowsOf(draft)) row.labelClipped = false
    })
    expect(evaluateTargetRowLayout(layout)).toContain(clippedFailure)
  })

  it('rejects incorrect dialog widths, margins, control minimums or escapes', () => {
    const widthDrift = withLayout((layout) => {
      dialogOf(rowsOf(layout)[0]!).dialogWidth = 512
    })
    expect(evaluateTargetRowLayout(widthDrift)).toContain(dialogFailure)

    const marginDrift = withLayout((layout) => {
      dialogOf(rowsOf(layout)[2]!).minMargin = 10
    })
    expect(evaluateTargetRowLayout(marginDrift)).toContain(dialogFailure)

    const documentOverflow = withLayout((layout) => {
      dialogOf(rowsOf(layout)[1]!).documentNoOverflow = false
    })
    expect(evaluateTargetRowLayout(documentOverflow)).toContain(dialogFailure)

    const triggerTooNarrow = withLayout((layout) => {
      const dialog = dialogOf(rowsOf(layout)[0]!)
      dialog.triggerWidth = (dialog.minimumControlWidth as number) - 2
    })
    expect(evaluateTargetRowLayout(triggerTooNarrow)).toContain(dialogFailure)

    const modelTooNarrow = withLayout((layout) => {
      const dialog = dialogOf(rowsOf(layout)[0]!)
      dialog.modelWidth = (dialog.minimumControlWidth as number) - 2
    })
    expect(evaluateTargetRowLayout(modelTooNarrow)).toContain(dialogFailure)

    const wrongMinimum = withLayout((layout) => {
      dialogOf(rowsOf(layout)[3]!).minimumControlWidth = 120
    })
    expect(evaluateTargetRowLayout(wrongMinimum)).toContain(dialogFailure)

    const malformedMinimum = withLayout((layout) => {
      dialogOf(rowsOf(layout)[0]!).minimumControlWidth = '300'
    })
    expect(evaluateTargetRowLayout(malformedMinimum)).toContain(dialogFailure)

    const nonFiniteWidth = withLayout((layout) => {
      dialogOf(rowsOf(layout)[0]!).dialogWidth = Number.NaN
    })
    expect(evaluateTargetRowLayout(nonFiniteWidth)).toContain(dialogFailure)

    const missingDialog = withLayout((layout) => {
      delete rowsOf(layout)[1]!.dialog
    })
    expect(evaluateTargetRowLayout(missingDialog)).toContain(dialogFailure)
  })

  it('rejects an unexpected dialog width target sequence', () => {
    const layout = withLayout((draft) => {
      dialogOf(rowsOf(draft)[1]!).expectedDialogWidth = 1000
    })
    expect(evaluateTargetRowLayout(layout)).toContain(expectedDialogFailure)
  })

  it('rejects a desktop dialog that never grew past the narrow layout', () => {
    const layout = withLayout((draft) => {
      for (const row of rowsOf(draft)) {
        const dialog = dialogOf(row)
        dialog.dialogWidth = 512
        dialog.expectedDialogWidth = 512
        dialog.minimumControlWidth = 160
        dialog.triggerWidth = 200
        dialog.modelWidth = 200
      }
    })
    expect(evaluateTargetRowLayout(layout)).toContain(growthFailure)
  })

  it('rejects incorrect saved payload or fixture-cleanup evidence', () => {
    const mutations: Array<(layout: Json) => void> = [
      (layout) => {
        layout.savedEndpointId = 'ep-other'
      },
      (layout) => {
        layout.savedModelLength = 40
      },
      (layout) => {
        layout.savedModelLength = Number.NaN
      },
      (layout) => {
        layout.savedModelUnicode = false
      },
      (layout) => {
        layout.createdRuleDeleted = false
      },
    ]
    for (const mutate of mutations) {
      expect(evaluateTargetRowLayout(withLayout(mutate))).toContain(saveFailure)
    }
  })

  it('rejects incomplete operability evidence', () => {
    const keys = [
      'keyboardSelectOpened',
      'modelEditable',
      'toggleClicked',
      'settingsOpenedAndClosed',
      'singleTargetDeleteDisabled',
    ]
    for (const key of keys) {
      const layout = withLayout((draft) => {
        ;(draft.operability as Json)[key] = false
      })
      expect(evaluateTargetRowLayout(layout)).toContain(operabilityFailure)
    }

    const missing = withLayout((layout) => {
      delete layout.operability
    })
    expect(evaluateTargetRowLayout(missing)).toContain(operabilityFailure)
  })
})
