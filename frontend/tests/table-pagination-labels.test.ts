import { expect, test } from 'bun:test'

import { messages } from '../src/i18n'

const source = () =>
  Bun.file(
    new URL('../src/components/shared/TablePagination.vue', import.meta.url),
  ).text()

const paginationKeys = [
  'paginationFirst',
  'paginationPrevious',
  'paginationNext',
  'paginationLast',
  'paginationPage',
  'paginationPageCurrent',
  'paginationPageSize',
] as const

test('pagination names exist in both app locales', () => {
  for (const locale of ['zh-CN', 'en-US'] as const) {
    for (const key of paginationKeys) {
      expect(messages[locale][key].trim()).not.toBe('')
    }
  }
  expect(messages['zh-CN'].paginationPage).toContain('{page}')
  expect(messages['en-US'].paginationPage).toContain('{page}')
  expect(messages['zh-CN'].paginationPageCurrent).toContain('当前')
  expect(messages['en-US'].paginationPageCurrent).toContain('current')
})

test('pager controls use the localized labels through supported slots', async () => {
  const text = await source()

  expect(text).toContain("import { useLocale } from '@/composables/useLocale'")
  expect(text).toContain(':aria-label="t(\'paginationPageSize\')"')
  for (const slot of ['first', 'prev', 'next', 'last']) {
    expect(text).toContain(`<template #${slot}>`)
  }
  for (const key of [
    'paginationFirst',
    'paginationPrevious',
    'paginationNext',
    'paginationLast',
  ]) {
    expect(text).toContain(`:aria-label="t('${key}')"`)
  }
  expect(text).toContain('<template #item="{ item, page: currentPage }">')
  expect(text).toContain('pageLabel(item.value, currentPage === item.value)')
  expect(text).toContain(':aria-label="pageLabel(')
})

test('pager keeps real page events and disabled state', async () => {
  const text = await source()

  expect(text).toContain('@update:page="changePage"')
  expect(text).toContain("emit('change', pageChange(nextPage, props.rows))")
  expect(text).toContain("emit('change', pageChange(1, rows))")
  // The default slot buttons stay the click/disabled surface owned by
  // UPagination, so the localized label does not replace its events.
  expect(text).not.toContain('@click')
})
