/// Statistics over an indexed series. Pure functions, so they are the one part of the Analytics
/// screen that can be tested without a browser, and they are the part that must be right.

export type Point = { day: string; index: number }

/// Annualised standard deviation of daily returns, in percent.
///
/// The square root of 252 is the conventional trading-day count. It is a convention rather than a
/// measurement, which is why the screen says "annualised" rather than claiming a yearly figure.
export function volatility(points: Point[]): number {
  if (points.length < 3) return 0
  const rets: number[] = []
  for (let i = 1; i < points.length; i++) rets.push(points[i].index / points[i - 1].index - 1)
  const mean = rets.reduce((a, b) => a + b, 0) / rets.length
  // n-1: these are a sample of returns, not the whole population of them.
  const variance = rets.reduce((a, r) => a + (r - mean) ** 2, 0) / (rets.length - 1)
  return Math.sqrt(variance) * Math.sqrt(252) * 100
}

/// The worst peak-to-trough fall in the series, as a positive percent.
///
/// Measured against the running peak, not against the start: a portfolio that doubles and then
/// halves has had a 50% drawdown even though it ends where it began.
export function drawdown(points: Point[]): number {
  let peak = -Infinity
  let worst = 0
  for (const p of points) {
    peak = Math.max(peak, p.index)
    worst = Math.min(worst, p.index / peak - 1)
  }
  // Negating zero gives -0, which renders as "-0.0%" on a portfolio that never fell. The
  // comparison catches both signs of zero, since -0 === 0.
  return worst === 0 ? 0 : -worst * 100
}

/// Compound annual growth, in percent, or null when the run is too short to annualise.
///
/// Under a year, annualising turns a small move into a preposterous yearly claim: a 3% gain over
/// a fortnight annualises to over 100%.
export function annualised(points: Point[]): number | null {
  if (points.length < 252) return null
  const years = points.length / 252
  return (Math.pow(points[points.length - 1].index / 100, 1 / years) - 1) * 100
}

/// One Bollinger reading: the window mean and the envelope around it. Null during the warm-up,
/// where there are not yet `span` days to average.
export type Band = { mid: number; upper: number; lower: number }

/// Bollinger bands over the indexed series, aligned one-for-one with `points`.
///
/// The deviation is the population one, over the window itself: the window is the whole of what
/// is being averaged, not a sample drawn from something larger. This differs from `volatility`
/// above, deliberately, and both match their own convention.
export function bollinger(points: Point[], span = 20, k = 2): (Band | null)[] {
  return points.map((_, i) => {
    if (i + 1 < span) return null
    const w = points.slice(i + 1 - span, i + 1).map((p) => p.index)
    const mid = w.reduce((a, v) => a + v, 0) / span
    const sd = Math.sqrt(w.reduce((a, v) => a + (v - mid) ** 2, 0) / span)
    return { mid, upper: mid + k * sd, lower: mid - k * sd }
  })
}

/// Relative strength, 0 to 100, with Wilder's smoothing. Null until there are `span` changes.
///
/// ponytail: a running pair of averages rather than a rolling window. Wilder's smoothing is
/// recursive by definition, so there is nothing to slice.
export function rsi(points: Point[], span = 14): (number | null)[] {
  const out: (number | null)[] = points.map(() => null)
  if (points.length <= span) return out
  let gain = 0
  let loss = 0
  for (let i = 1; i <= span; i++) {
    const d = points[i].index - points[i - 1].index
    if (d >= 0) gain += d
    else loss -= d
  }
  gain /= span
  loss /= span
  // A run with no falls at all divides by zero. 100 is the limit it is heading for, and it is
  // what every charting package shows there.
  const value = () => (loss === 0 ? 100 : 100 - 100 / (1 + gain / loss))
  out[span] = value()
  for (let i = span + 1; i < points.length; i++) {
    const d = points[i].index - points[i - 1].index
    gain = (gain * (span - 1) + Math.max(d, 0)) / span
    loss = (loss * (span - 1) + Math.max(-d, 0)) / span
    out[i] = value()
  }
  return out
}

/// Move the series so its first day reads 100 again.
///
/// Slicing a range off the front leaves the chart's one gridline pointing at a day that is no
/// longer on it. Rebasing makes 100 mean "where this range started", which is the only reading
/// that matches the label on the axis.
export function rebase(points: Point[]): Point[] {
  const first = points[0]?.index
  if (!first) return points
  return points.map((p) => ({ day: p.day, index: (p.index / first) * 100 }))
}
