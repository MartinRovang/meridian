/// A single path through an indexed series. No chart library: this is one polyline, two gridlines
/// and a fill, which is less code than configuring a library to draw the same thing.

export type Point = { day: string; index: number }

const W = 720
const H = 200
const PAD = 8

export function Line({ points, compare }: { points: Point[]; compare?: Point[] }) {
  if (points.length < 2) return null
  const values = points.map((p) => p.index)
  // The second line shares the axis, so it has to share the scale: drawing it on its own range
  // would put two different scales on one picture and make the loser look like the winner.
  const all = [...values, ...(compare ?? []).map((p) => p.index)]
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
      <path className="line-area" d={`${line} L${x(points.length - 1, points.length)} ${H} L${x(0, points.length)} ${H} Z`} fill="url(#line-fill)" />
      {compare && compare.length > 1 ? (
        <path className="line-path line-compare" d={path(compare)} />
      ) : null}
      <path className="line-path" d={line} stroke={up ? 'var(--color-up)' : 'var(--color-down)'} />
    </svg>
  )
}
