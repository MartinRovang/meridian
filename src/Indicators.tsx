/// The indicator switches and the chart's time range, with the state behind them.
///
/// Two screens draw the same chart, so they share these rather than each keeping their own
/// booleans in step with their own checkboxes.

import { useState, type JSX } from 'react'

export type Shown = {
  bands: boolean
  ma: boolean
  rsi: boolean
  /// Days in the Bollinger and average window, shared: the band is drawn around that average.
  span: number
  /// Deviations out to the edge of the envelope.
  k: number
  rsiSpan: number
}

/// Bollinger's own numbers, and Wilder's. Conventions rather than findings, which is why they
/// are the starting point and not the only point.
const DEFAULTS: Shown = { bands: true, ma: false, rsi: false, span: 20, k: 2, rsiSpan: 14 }

/// A window shorter than this cannot have a deviation, and one longer than the series has no
/// days to average. The bound is here rather than in the arithmetic: `bollinger` answers null
/// for a window it cannot fill, and a typed 0 should not reach it as a division.
const MIN_SPAN = 2

export function useIndicators(): [Shown, JSX.Element] {
  const [shown, setShown] = useState<Shown>(DEFAULTS)
  const set = (patch: Partial<Shown>) => setShown({ ...shown, ...patch })
  const num = (key: 'span' | 'k' | 'rsiSpan', min: number, step: number) => (
    <input
      className="input num"
      type="number"
      min={min}
      step={step}
      value={shown[key]}
      onChange={(e) => set({ [key]: Number(e.target.value) || DEFAULTS[key] })}
    />
  )

  const ui = (
    <div className="indicators">
      <label>
        <input
          type="checkbox"
          checked={shown.bands}
          onChange={(e) => set({ bands: e.target.checked })}
        />
        Bollinger bands
      </label>
      <label>
        <input type="checkbox" checked={shown.ma} onChange={(e) => set({ ma: e.target.checked })} />
        Average on its own
      </label>
      {shown.bands || shown.ma ? (
        <span className="indicator-window">
          over {num('span', MIN_SPAN, 1)} days
          {shown.bands ? <>, {num('k', 0.5, 0.5)} deviations wide</> : null}
        </span>
      ) : null}
      <label>
        <input
          type="checkbox"
          checked={shown.rsi}
          onChange={(e) => set({ rsi: e.target.checked })}
        />
        Relative strength
      </label>
      {shown.rsi ? (
        <span className="indicator-window">over {num('rsiSpan', MIN_SPAN, 1)} days</span>
      ) : null}
    </div>
  )
  return [shown, ui]
}

/// How far back the chart looks, in trading days. Null is everything there is.
///
/// ponytail: trading days rather than calendar ones, because that is what a row in the series
/// is. Roughly 21 to the month, which is close enough for a button labelled "1 month".
const RANGES: [string, number | null][] = [
  ['1M', 21],
  ['3M', 63],
  ['6M', 126],
  ['1Y', 252],
  ['3Y', 756],
  ['All', null],
]

export function useRange(): [number | null, JSX.Element] {
  const [days, setDays] = useState<number | null>(null)
  const ui = (
    <div className="range-picker">
      {RANGES.map(([label, n]) => (
        <button
          key={label}
          className={`btn ${days === n ? 'btn-primary' : 'btn-secondary'}`}
          onClick={() => setDays(n)}
        >
          {label}
        </button>
      ))}
    </div>
  )
  return [days, ui]
}
