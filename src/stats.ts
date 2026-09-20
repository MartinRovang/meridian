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
