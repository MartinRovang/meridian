/// A single path through an indexed series. No chart library: this is one polyline, two gridlines
/// and a fill, which is less code than configuring a library to draw the same thing.

import { bollinger, rsi } from '../stats'

export type Point = { day: string; index: number }

const W = 720
const H = 200
const PAD = 8

/// Twenty days and two deviations, the numbers Bollinger published. They are conventions, not
/// findings, and changing them here would change every chart at once.
export const SPAN = 20
export const K = 2
/// Fourteen, likewise, is Wilder's own period.
export const RSI_SPAN = 14

export function Line({
  points,
  compare,
  bands,
  ma,
}: {
  points: Point[]
  compare?: Point[]
  bands?: boolean
  ma?: boolean
}) {
  if (points.length < 2) return null
  const values = points.map((p) => p.index)
  const band = bands || ma ? bollinger(points, SPAN, K) : []
  // The second line shares the axis, so it has to share the scale: drawing it on its own range
  // would put two different scales on one picture and make the loser look like the winner. The
  // envelope joins the same reckoning, or a band wider than the price would be clipped off.
  const all = [
    ...values,
    ...(compare ?? []).map((p) => p.index),
    ...(bands ? band.flatMap((b) => (b ? [b.upper, b.lower] : [])) : []),
  ]
  const lo = Math.min(...all, 100)
  const hi = Math.max(...all, 100)
  // A flat series would divide by zero; a hair of range keeps the line in the middle instead.
  const span = hi - lo || 1
  const x = (i: number, n: number) => PAD + (i / (n - 1)) * (W - PAD * 2)
  const y = (v: number) => PAD + (1 - (v - lo) / span) * (H - PAD * 2)
  const path = (ps: Point[]) =>
    ps.map((p, i) => `${i ? 'L' : 'M'}${x(i, ps.length).toFixed(1)} ${y(p.index).toFixed(1)}`).join(' ')
  const line = path(points)
  const up = values[values.length - 1] >= 100

  // The warm-up days have no band, so the envelope simply starts later rather than being drawn
  // flat at the first value it can compute.
  const drawn = band.map((b, i) => ({ b, i })).filter((r) => r.b !== null)
  const trace = (pick: (b: NonNullable<(typeof band)[number]>) => number) =>
    drawn
      .map((r, j) => `${j ? 'L' : 'M'}${x(r.i, points.length).toFixed(1)} ${y(pick(r.b!)).toFixed(1)}`)
      .join(' ')
  // The lower edge is walked back the way it came, so the two edges close into one shape.
  let back = ''
  for (let j = drawn.length - 1; j >= 0; j--) {
    const r = drawn[j]
    back += ` L${x(r.i, points.length).toFixed(1)} ${y(r.b!.lower).toFixed(1)}`
  }
  const envelope = drawn.length ? `${trace((b) => b.upper)}${back} Z` : ''

  return (
    <svg className="line" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img">
      <defs>
        <linearGradient id="line-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={up ? 'var(--color-up)' : 'var(--color-down)'} stopOpacity="0.22" />
          <stop offset="100%" stopColor={up ? 'var(--color-up)' : 'var(--color-down)'} stopOpacity="0" />
        </linearGradient>
      </defs>
      {/* 100 is where the money started, so it is the only gridline worth drawing. */}
      <line className="line-base" x1={PAD} x2={W - PAD} y1={y(100)} y2={y(100)} />
      {bands && envelope ? <path className="line-band" d={envelope} /> : null}
      <path className="line-area" d={`${line} L${x(points.length - 1, points.length)} ${H} L${x(0, points.length)} ${H} Z`} fill="url(#line-fill)" />
      {compare && compare.length > 1 ? (
        <path className="line-path line-compare" d={path(compare)} />
      ) : null}
      {ma && drawn.length ? <path className="line-path line-ma" d={trace((b) => b.mid)} /> : null}
      <path className="line-path" d={line} stroke={up ? 'var(--color-up)' : 'var(--color-down)'} />
    </svg>
  )
}

/// Relative strength under the chart, on its own 0 to 100 axis.
///
/// It gets its own picture rather than a second axis on the first: two scales in one frame is the
/// chart mistake that makes a flat line look like a rally.
export function Rsi({ points }: { points: Point[] }) {
  const r = rsi(points, RSI_SPAN)
  const drawn = r.map((v, i) => ({ v, i })).filter((p) => p.v !== null)
  if (drawn.length < 2) return null
  const h = 72
  const x = (i: number) => PAD + (i / (points.length - 1)) * (W - PAD * 2)
  const y = (v: number) => PAD + (1 - v / 100) * (h - PAD * 2)
  const d = drawn
    .map((p, j) => `${j ? 'L' : 'M'}${x(p.i).toFixed(1)} ${y(p.v!).toFixed(1)}`)
    .join(' ')
  return (
    <svg className="rsi" viewBox={`0 0 ${W} ${h}`} preserveAspectRatio="none" role="img">
      {[70, 30].map((v) => (
        <line key={v} className="line-base" x1={PAD} x2={W - PAD} y1={y(v)} y2={y(v)} />
      ))}
      <path className="line-path line-rsi" d={d} />
    </svg>
  )
}
