import { expect, test } from 'vitest'
import { shock, worstWindow } from './stress'
import type { Point } from './stats'

const series = (values: number[]): Point[] =>
  values.map((index, i) => ({ day: `d${String(i).padStart(2, '0')}`, index }))

const h = (value: number, currency: string, priced = true) => ({
  name: 'x',
  value,
  currency,
  priced,
})

test('the worst window is the worst move over that span, not the worst single day in it', () => {
  // Falls 10% on one day, then grinds down 5% more over the next two. The worst 3-day window is
  // the whole slide, which no single-day figure would show.
  const w = worstWindow(series([100, 90, 88, 85.5, 86]), 3)
  expect(w).not.toBeNull()
  expect(w?.pct).toBeCloseTo(-14.5, 6)
  expect(w?.from).toBe('d00')
  expect(w?.to).toBe('d03')
})

test('a span longer than the series has no answer, rather than a wrong one', () => {
  expect(worstWindow(series([100, 90]), 5)).toBeNull()
  expect(worstWindow([], 1)).toBeNull()
})

test('a series that only rises reports its smallest gain, not a fabricated loss', () => {
  const w = worstWindow(series([100, 110, 121]), 1)
  expect(w?.pct).toBeCloseTo(10, 6)
})

test('a market fall hits every holding, whatever it is priced in', () => {
  const s = shock([h(600, 'NOK'), h(400, 'USD')], 'NOK', 20, 0)
  expect(s.before).toBeCloseTo(1000, 6)
  expect(s.after).toBeCloseTo(800, 6)
  expect(s.loss).toBeCloseTo(-200, 6)
})

test('a currency fall hits only what is held abroad', () => {
  const s = shock([h(600, 'NOK'), h(400, 'USD')], 'NOK', 0, 10)
  expect(s.after).toBeCloseTo(600 + 360, 6)
  expect(s.foreign).toBeCloseTo(400, 6)
})

test('the two shocks compound rather than adding up', () => {
  // 20% off then 10% off is 28%, not 30%. Adding them would overstate the damage.
  const s = shock([h(1000, 'USD')], 'NOK', 20, 10)
  expect(s.after).toBeCloseTo(720, 6)
})

test('an unpriced holding is named, never shocked as if it were worth nothing', () => {
  // The value carried here is deliberately not zero: `priced: false` means the field is
  // meaningless, and a guard that only works because the number happens to be zero is no guard.
  const s = shock([h(1000, 'NOK'), { ...h(999, 'USD', false), name: 'MYSTERY' }], 'NOK', 50, 0)
  expect(s.before).toBeCloseTo(1000, 6)
  expect(s.after).toBeCloseTo(500, 6)
  expect(s.foreign).toBe(0)
  expect(s.unpriced).toEqual(['MYSTERY'])
})
