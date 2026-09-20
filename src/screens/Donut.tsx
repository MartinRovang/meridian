import type { ClassSlice } from '../store'

// The design's allocation ring. Hand-rolled with stroke-dasharray on concentric circles rather
// than arc paths: one circle per slice, each rotated to start where the last ended. No sweep-flag
// arithmetic, and no chart library for seven numbers.
const HUES = [
  'var(--color-accent)',
  'var(--color-accent-400)',
  'var(--color-accent-2-600)',
  'var(--color-accent-700)',
  'var(--color-accent-2)',
  'var(--color-neutral-600)',
  'var(--color-accent-800)',
]

export const hue = (i: number) => HUES[i % HUES.length]

const R = 60
const CIRC = 2 * Math.PI * R

export function Donut({ slices }: { slices: ClassSlice[] }) {
  let offset = 0
  return (
    <svg viewBox="0 0 160 160" className="donut" role="img" aria-label="Allocation by class">
      {slices.map((s, i) => {
        const len = (s.pct / 100) * CIRC
        // -90deg puts the first slice at twelve o'clock, where a reader starts.
        const rotation = (offset / CIRC) * 360 - 90
        offset += len
        return (
          <circle
            key={s.cls}
            cx="80"
            cy="80"
            r={R}
            fill="none"
            stroke={hue(i)}
            strokeWidth="18"
            strokeDasharray={`${len} ${CIRC - len}`}
            transform={`rotate(${rotation} 80 80)`}
          />
        )
      })}
    </svg>
  )
}
