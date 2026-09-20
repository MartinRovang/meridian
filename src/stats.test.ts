import { expect, test } from 'vitest'
import { annualised, drawdown, volatility, type Point } from './stats'

const series = (values: number[]): Point[] =>
  values.map((index, i) => ({ day: `2026-01-${String(i + 1).padStart(2, '0')}`, index }))

test('a flat series has no volatility and no drawdown', () => {
  const flat = series(Array(300).fill(100))
  expect(volatility(flat)).toBe(0)
  expect(drawdown(flat)).toBe(0)
  expect(annualised(flat)).toBeCloseTo(0, 6)
})

test('drawdown is measured from the running peak, not from the start', () => {
  // Doubles, then halves back to where it began. Ending flat is not the same as never falling.
  expect(drawdown(series([100, 200, 100]))).toBeCloseTo(50, 6)
  // A fall before a later, higher peak still counts as the worst if it is the deepest.
  expect(drawdown(series([100, 60, 100, 90]))).toBeCloseTo(40, 6)
})

test('a series that only rises has no drawdown', () => {
  expect(drawdown(series([100, 110, 120]))).toBe(0)
})

test('volatility annualises the sample deviation of daily returns', () => {
  // Alternating +10%/-9.0909% returns: every daily return has the same magnitude, so the sample
  // deviation is computable by hand and the function must not drift from it.
  const values = [100]
  for (let i = 0; i < 20; i++) values.push(i % 2 === 0 ? 110 : 100)
  const v = volatility(series(values))
  const rets: number[] = []
  for (let i = 1; i < values.length; i++) rets.push(values[i] / values[i - 1] - 1)
  const mean = rets.reduce((a, b) => a + b, 0) / rets.length
  // n-1, not n: with 20 returns the Bessel correction is a 2% difference in the answer, which is
  // large enough to notice and small enough to mistake for rounding.
  const sd = Math.sqrt(rets.reduce((a, r) => a + (r - mean) ** 2, 0) / (rets.length - 1))
  expect(v).toBeCloseTo(sd * Math.sqrt(252) * 100, 4)
})

test('too few points is zero rather than a division by zero', () => {
  expect(volatility([])).toBe(0)
  expect(volatility(series([100]))).toBe(0)
  expect(volatility(series([100, 110]))).toBe(0)
  expect(drawdown([])).toBe(0)
})

test('a run under a year is not annualised, because it would be a preposterous claim', () => {
  // 3% over a fortnight annualises to over 100%. Refusing is the honest answer.
  expect(annualised(series(Array.from({ length: 14 }, (_, i) => 100 + i * 0.2)))).toBeNull()
})

test('a doubling over exactly two years annualises to the square root of two', () => {
  const values = Array.from({ length: 504 }, (_, i) => 100 * Math.pow(2, i / 503))
  expect(annualised(series(values))).toBeCloseTo((Math.SQRT2 - 1) * 100, 1)
})
