import { expect, test } from 'vitest'
import { annualised, bollinger, drawdown, rebase, rsi, volatility, type Point } from './stats'

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

test('a flat series sits exactly on its own band, with no width', () => {
  // Nothing moved, so the mean is the value and the deviation is zero. A band with width here
  // would mean the arithmetic invented movement that never happened.
  const b = bollinger(series(Array(30).fill(100)), 20, 2)
  expect(b.slice(0, 19).every((x) => x === null)).toBe(true)
  expect(b[19]).toEqual({ mid: 100, upper: 100, lower: 100 })
})

test('the band is the window mean plus and minus k deviations of the window', () => {
  const values = Array.from({ length: 20 }, (_, i) => 100 + i)
  const b = bollinger(series(values), 20, 2)
  const mean = values.reduce((a, v) => a + v, 0) / 20
  // Population deviation, not the sample one: the window is the whole of what is being averaged,
  // and every published Bollinger band is drawn this way.
  const sd = Math.sqrt(values.reduce((a, v) => a + (v - mean) ** 2, 0) / 20)
  expect(b[19]?.mid).toBeCloseTo(mean, 9)
  expect(b[19]?.upper).toBeCloseTo(mean + 2 * sd, 9)
  expect(b[19]?.lower).toBeCloseTo(mean - 2 * sd, 9)
})

test('a series shorter than the window has no band at all', () => {
  expect(bollinger(series([100, 101, 102]), 20, 2).every((x) => x === null)).toBe(true)
})

test('a series that only rises is pinned at 100, one that only falls at 0', () => {
  const up = rsi(series(Array.from({ length: 30 }, (_, i) => 100 + i)), 14)
  expect(up[13]).toBeNull()
  expect(up[14]).toBeCloseTo(100, 9)
  const down = rsi(series(Array.from({ length: 30 }, (_, i) => 200 - i)), 14)
  expect(down[14]).toBeCloseTo(0, 9)
})

test('rsi smooths the way Wilder did, not as a plain rolling mean', () => {
  // Fourteen equal gains then one loss of the same size. A plain mean would drop the average
  // gain to 13/14 of its value; Wilder's smoothing keeps 13/14 of it and adds nothing, which is
  // the same number here, so the loss side is what separates the two: 1/14 of the loss, not the
  // whole of it over a window of one.
  const values = [100]
  for (let i = 0; i < 14; i++) values.push(values[values.length - 1] + 1)
  values.push(values[values.length - 1] - 1)
  const r = rsi(series(values), 14)
  const avgGain = (14 / 14) * (13 / 14)
  const avgLoss = 1 / 14
  expect(r[15]).toBeCloseTo(100 - 100 / (1 + avgGain / avgLoss), 6)
})

test('rebasing puts 100 at the first day of the range and leaves the shape alone', () => {
  const r = rebase(series([120, 132, 108]))
  expect(r[0].index).toBe(100)
  expect(r[1].index).toBeCloseTo(110, 9)
  expect(r[2].index).toBeCloseTo(90, 9)
  // The ratio between any two days is what the chart is drawing, and it must survive the move.
  expect(r[2].index / r[1].index).toBeCloseTo(108 / 132, 9)
})

test('rebasing an empty or zero-based series does not produce infinities', () => {
  expect(rebase([])).toEqual([])
  // A zero first day cannot be divided by. Leaving the series alone is wrong in principle and
  // harmless in practice: an index of zero means the money was all gone on day one.
  expect(rebase(series([0, 5]))[1].index).toBe(5)
})
