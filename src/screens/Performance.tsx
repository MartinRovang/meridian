/// The portfolio's own path, on the Dashboard rather than on a screen of its own.
///
/// It fetches separately from the rest of the Dashboard: the history is a walk over every day of
/// every holding, and the numbers at the top of the screen should not wait for it.

import { useEffect, useState } from 'react'
import { get } from '../api'
import { pct, signed } from '../format'
import { useIndicators, useRange } from '../Indicators'
import { Stat } from '../Stat'
import { annualised, drawdown, rebase, volatility } from '../stats'
import { Line, Rsi, type Point } from './Line'

type History = { points: Point[]; missing: string[] }

export function Performance({ id }: { id: string }) {
  const [hist, setHist] = useState<History | null>(null)
  const [error, setError] = useState('')
  const [shown, switches] = useIndicators()
  const [days, rangeUi] = useRange()

  useEffect(() => {
    setHist(null)
    setError('')
    get<History>(`/api/history?portfolio=${encodeURIComponent(id)}`)
      .then(setHist)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [id])

  if (error) return <div className="banner bad">{error}</div>
  if (!hist) return null

  const { missing } = hist
  // Rebased, because the chart's one gridline says 100 and the range may start anywhere.
  const points = rebase(days ? hist.points.slice(-days) : hist.points)
  if (points.length < 2) {
    return (
      <section className="panel">
        <div className="panel-head">Performance</div>
        {rangeUi}
        <p className="text-muted">
          {hist.points.length < 2
            ? 'No history for this portfolio yet.'
            : 'Not enough days in this range to draw anything.'}
          {missing.length ? ` Nothing is known about ${missing.join(', ')}.` : ''}
        </p>
      </section>
    )
  }

  const total = points[points.length - 1].index - 100
  const annual = annualised(points)

  return (
    <section className="panel">
      <div className="panel-head">
        Performance
        <span className="text-muted">
          {' '}
          indexed to 100 on {points[0].day}
        </span>
      </div>
      {rangeUi}
      {switches}
      <Line
        points={points}
        bands={shown.bands}
        ma={shown.ma}
        span={shown.span}
        k={shown.k}
      />
      {shown.rsi ? <Rsi points={points} span={shown.rsiSpan} /> : null}

      <div className="stat-row">
        <Stat label="Over this range" value={`${signed(total)}%`} />
        <Stat label="Annualised" value={annual === null ? 'needs a year' : `${signed(annual)}%`} />
        <Stat label="Volatility, annualised" value={pct(volatility(points))} />
        <Stat label="Worst drawdown" value={`-${pct(drawdown(points))}`} />
      </div>

      {/* The one thing this panel must never be mistaken for. */}
      <p className="warn-soft">
        This is what today&apos;s holdings would have done, not what this portfolio did. Meridian
        has no record of what you owned in the past, so it values today&apos;s share count at each
        day&apos;s prices.
        {missing.length
          ? ` Left out, because nothing is known about their past: ${missing.join(', ')}.`
          : ''}
      </p>
    </section>
  )
}
