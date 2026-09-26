/// What has changed about each holding, on the Dashboard under the chart.
///
/// Renders what /api/outliers computes and computes nothing. Fetched on its own for the same
/// reason Performance is: it walks every day of every holding.

import { useEffect, useState } from 'react'
import { get } from '../api'
import { pct, signed } from '../format'

type Flag = 'volatility' | 'decoupling' | 'move'
type Row = {
  symbol: string
  vol_baseline: number
  vol_recent: number
  vol_ratio: number
  corr_baseline: number | null
  corr_recent: number | null
  day: string
  move_1d: number
  move_5d: number
  z_1d: number
  z_5d: number
  flags: Flag[]
}
type Report = {
  rows: Row[]
  clusters: { symbols: string[]; avg_corr: number }[]
  excluded: string[]
  from: string
  recent_from: string
  through: string
}

const corr = (c: number | null) => (c === null ? 'n/a' : c.toFixed(2))

function tag(r: Row, f: Flag) {
  if (f === 'volatility') return r.vol_ratio >= 1 ? 'wilder' : 'calmer'
  if (f === 'decoupling') return 'moves differently'
  return 'unusual move'
}

export function Outliers({ id }: { id: string }) {
  const [rep, setRep] = useState<Report | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    setRep(null)
    setError('')
    get<Report>(`/api/outliers?portfolio=${encodeURIComponent(id)}`)
      .then(setRep)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [id])

  if (error) return <div className="banner bad">{error}</div>
  if (!rep) return null

  const excluded = rep.excluded.length ? (
    <p className="text-muted">
      Not measured, too little history for a year's baseline: {rep.excluded.join(', ')}.
    </p>
  ) : null

  if (!rep.rows.length) {
    return (
      <section className="panel">
        <div className="panel-head">What has changed</div>
        <p className="text-muted">Nothing here has a year and a quarter of history yet.</p>
        {excluded}
      </section>
    )
  }

  const flagged = rep.rows.filter((r) => r.flags.length).length
  const inGroup = new Set(rep.clusters.flatMap((c) => c.symbols))

  return (
    <section className="panel">
      <div className="panel-head">
        What has changed
        <span className="text-muted">
          {' '}
          since {rep.recent_from}, against the year before · {flagged ? `${flagged} flagged` : 'nothing flagged'}
        </span>
      </div>

      <table className="table">
        <thead>
          <tr>
            <th>Holding</th>
            <th className="num">Volatility, year → recent</th>
            <th className="num">With the rest, year → recent</th>
            <th className="num">Last day</th>
            <th className="num">5 days</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rep.rows.map((r) => (
            <tr key={r.symbol} className={r.flags.length ? 'breached' : ''}>
              <td>{r.symbol}</td>
              <td className="num">
                {pct(r.vol_baseline)} → {pct(r.vol_recent)}{' '}
                <span className="text-muted">×{r.vol_ratio.toFixed(2)}</span>
              </td>
              <td className="num">
                {corr(r.corr_baseline)} → {corr(r.corr_recent)}
              </td>
              <td className="num">
                {signed(r.move_1d)}% <span className="text-muted">{signed(r.z_1d)}σ</span>
              </td>
              <td className="num">
                {signed(r.move_5d)}% <span className="text-muted">{signed(r.z_5d)}σ</span>
              </td>
              <td className="num">
                {r.flags.map((f) => (
                  <span key={f} className="tag tag-outline">
                    {tag(r, f)}
                  </span>
                ))}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <p className="text-muted">
        Each holding is measured against its own past year, so a volatile stock is not flagged for
        being volatile. Flagged: volatility 1.5× or ⅔ of its own, a correlation with the rest of
        the portfolio that moved 0.3 or more, or a move 3σ out. σ is the holding's own daily
        swing over the year. Today's holdings priced back through time, through {rep.through}.
      </p>

      <div className="panel-head">Moving together</div>
      {rep.clusters.length ? (
        <>
          {rep.clusters.map((c) => (
            <p key={c.symbols.join()}>
              {c.symbols.join(', ')}{' '}
              <span className="text-muted">
                average correlation {c.avg_corr.toFixed(2)}: closer to one position than{' '}
                {c.symbols.length}
              </span>
            </p>
          ))}
          <p className="text-muted">
            {rep.rows
              .map((r) => r.symbol)
              .filter((s) => !inGroup.has(s))
              .join(', ') || 'Nothing else'}{' '}
            moves on its own.
          </p>
        </>
      ) : (
        <p className="text-muted">
          No two holdings move together at a correlation of 0.5 or more since {rep.from}: each is
          its own bet.
        </p>
      )}
      {excluded}
    </section>
  )
}
