/// The indicator switches, and the state behind them.
///
/// Two screens draw the same chart, so they share one hook rather than each keeping their own
/// three booleans in step with their own three checkboxes.

import { useState, type JSX } from 'react'
import { K, RSI_SPAN, SPAN } from './screens/Line'

export type Shown = { bands: boolean; ma: boolean; rsi: boolean }

const OPTIONS: [keyof Shown, string][] = [
  ['bands', `Bollinger bands (${SPAN} days, ${K} deviations)`],
  ['ma', `${SPAN}-day average`],
  ['rsi', `Relative strength (${RSI_SPAN} days)`],
]

export function useIndicators(): [Shown, JSX.Element] {
  const [shown, setShown] = useState<Shown>({ bands: false, ma: false, rsi: false })
  const ui = (
    <div className="indicators">
      {OPTIONS.map(([key, label]) => (
        <label key={key}>
          <input
            type="checkbox"
            checked={shown[key]}
            onChange={(e) => setShown({ ...shown, [key]: e.target.checked })}
          />
          {label}
        </label>
      ))}
    </div>
  )
  return [shown, ui]
}
