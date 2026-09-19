import { expect, test } from 'bun:test'
import { createRowRevealState } from '../src/redaction-reveal'

test('reveal marks one row and is idempotent', () => {
  const state = createRowRevealState()
  expect(state.isRevealed(2)).toBe(false)
  state.reveal(2)
  state.reveal(2)
  expect(state.isRevealed(2)).toBe(true)
  expect(state.isRevealed(1)).toBe(false)
})

test('toggle flips one row and hide closes it again', () => {
  const state = createRowRevealState()
  state.toggle(0)
  expect(state.isRevealed(0)).toBe(true)
  state.toggle(0)
  expect(state.isRevealed(0)).toBe(false)
  state.reveal(0)
  state.hide(0)
  expect(state.isRevealed(0)).toBe(false)
})

test('remove drops the removed row and shifts higher indices down', () => {
  const state = createRowRevealState()
  state.reveal(1)
  state.reveal(3)
  state.reveal(5)
  state.remove(3)
  expect(state.isRevealed(3)).toBe(false)
  expect(state.isRevealed(1)).toBe(true)
  expect(state.isRevealed(2)).toBe(false)
  expect(state.isRevealed(4)).toBe(true)
})

test('clear resets every revealed row', () => {
  const state = createRowRevealState()
  state.reveal(0)
  state.reveal(4)
  state.clear()
  expect(state.isRevealed(0)).toBe(false)
  expect(state.isRevealed(4)).toBe(false)
})
