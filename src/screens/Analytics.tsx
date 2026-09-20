import { useEffect, useState } from 'react'
import { get } from '../api'
import { pct, signed } from '../format'
import { SkelScreen } from '../Skeleton'
import { Stat } from '../Stat'
import { useIndicators } from '../Indicators'
import { useApp } from '../store'
import { annualised, drawdown, volatility } from '../stats'
import { Line, Rsi, type Point } from './Line'

type History = { points: Point[]; missing: string[] }

export function Analytics() {
  const { state, pid } = useApp()
  const [hist, setHist] = useState<History | null>(null)
  const [shown, switches] = useIndicators()
  const [error, setError] = useState('')

  const p = state?.portfolios.find((x) => x.id === pid) ?? state?.portfolios[0]
  const id = p?.id

  useEffect(() => {
    if (!id) return
    setHist(null)
    setError('')
    get<History>(`/api/history?portfolio=${encodeURIComponent(id)}`)
      .then(setHist)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [id])

  if (!state) return <SkelScreen />
  if (!p) return <p className="text-muted">No portfolios yet.</p>
  if (error) return <div className="banner bad">{error}</div>
  if (!hist) return <SkelScreen />

  const { points, missing } = hist
  if (points.length < 2) {
    return (
      <p className="text-muted">
        No history for this portfolio yet.
        {missing.length ? ` Nothing is known about ${missing.join(', ')}.` : ''}
      </p>
    )
  }

  const total = points[points.length - 1].index - 100
  const annual = annualised(points)

  return (
    <div className="screen">
      <div className="stat-band">
        <div className="stat-head">
          <div className="stat-total">{signed(total)}%</div>
          <div className="text-muted">
            over {points[0].day} to {points[points.length - 1].day}
          </div>
        </div>
      </div>

      {/* The one thing this screen must never be mistaken for. */}
      <p className="warn-soft">
        This is what today&apos;s holdings would have done, not what this portfolio did. Meridian
        has no record of what you owned in the past, so it values today&apos;s share count at each
        day&apos;s prices.
      </p>

      <section className="panel">
        <div className="panel-head">Indexed to 100 at the start</div>
        {switches}
        <Line points={points} bands={shown.bands} ma={shown.ma} />
        {shown.rsi ? <Rsi points={points} /> : null}
      </section>

      <div className="stat-row">
        <Stat label="Total" value={`${signed(total)}%`} />
        <Stat label="Annualised" value={annual === null ? 'needs a year' : `${signed(annual)}%`} />
        <Stat label="Volatility, annualised" value={pct(volatility(points))} />
        <Stat label="Worst drawdown" value={`-${pct(drawdown(points))}`} />
      </div>

      {missing.length ? (
        <p className="text-muted">
          Left out, because nothing is known about their past: {missing.join(', ')}. The chart
          covers the rest of the portfolio, not all of it.
        </p>
      ) : null}
    </div>
  )
}
