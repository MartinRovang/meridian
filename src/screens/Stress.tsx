import { useEffect, useState } from 'react'
import { get } from '../api'
import { money, pct, signed } from '../format'
import { SkelScreen } from '../Skeleton'
import { Stat } from '../Stat'
import { useApp } from '../store'
import type { Point } from '../stats'
import { shock, worstWindow } from '../stress'

type History = { points: Point[]; missing: string[] }

// Trading days, not calendar days: the series has no weekends in it.
const SPANS: [number, string][] = [
  [1, 'Worst day'],
  [5, 'Worst week'],
  [21, 'Worst month'],
  [63, 'Worst quarter'],
]

export function Stress() {
  const { state, pid } = useApp()
  const [hist, setHist] = useState<History | null>(null)
  const [error, setError] = useState('')
  const [market, setMarket] = useState(20)
  const [fx, setFx] = useState(10)

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

  const base = state.base_currency
  const s = shock(p.holdings, base, market, fx)

  return (
    <div className="screen">
      <section className="panel">
        <div className="panel-head">What if</div>
        <div className="shock-inputs">
          <label>
            <span className="text-muted">Every holding falls</span>
            <input
              className="input num"
              type="number"
              value={market}
              onChange={(e) => setMarket(Number(e.target.value))}
            />
            <span className="text-muted">%</span>
          </label>
          <label>
            {/* Phrased as the foreign currency falling, not the base one rising: the two are not
                the same arithmetic, and only one of them is what this multiplies by. */}
            <span className="text-muted">Every currency other than {base} falls</span>
            <input
              className="input num"
              type="number"
              value={fx}
              onChange={(e) => setFx(Number(e.target.value))}
            />
            <span className="text-muted">%</span>
          </label>
        </div>
        <div className="stat-row">
          <Stat label="Now" value={money(s.before, base)} />
          <Stat label="After" value={money(s.after, base)} />
          <Stat
            label="Change"
            value={`${money(s.loss, base)} (${signed((s.loss / (s.before || 1)) * 100)}%)`}
            tone={s.loss < 0 ? 'down' : 'flat'}
          />
          <Stat
            label={`Held outside ${base}`}
            value={`${pct((s.foreign / (s.before || 1)) * 100)}`}
          />
        </div>
        {s.unpriced.length ? (
          <p className="text-muted">
            Left out entirely, because there is no quote to shock: {s.unpriced.join(', ')}.
          </p>
        ) : null}
      </section>

      <section className="panel">
        <div className="panel-head">Worst it has been</div>
        {error ? <div className="banner bad">{error}</div> : null}
        {!hist && !error ? <SkelScreen /> : null}
        {hist ? <Worst points={hist.points} missing={hist.missing} /> : null}
      </section>
    </div>
  )
}

function Worst({ points, missing }: { points: Point[]; missing: string[] }) {
  // flatMap rather than map+filter: filtering does not narrow the type, and the alternative is
  // a non-null assertion on every cell below.
  const rows = SPANS.flatMap(([span, label]) => {
    const w = worstWindow(points, span)
    return w ? [{ label, ...w }] : []
  })
  if (!rows.length) {
    return <p className="text-muted">Not enough history yet to say how bad it has been.</p>
  }
  return (
    <>
      <p className="warn-soft">
        Measured on today&apos;s holdings priced back through time, not on what the portfolio
        actually held. It answers &quot;what would this basket have done&quot;, which is the
        question a stress test asks, but it is not a record of past losses.
      </p>
      <table className="table">
        <tbody>
          {rows.map((w) => (
            <tr key={w.label}>
              <td>{w.label}</td>
              <td className={`num ${w.pct < 0 ? 'down' : 'flat'}`}>{signed(w.pct)}%</td>
              <td className="text-muted">
                {w.from} to {w.to}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {/* Without this, a quarter that looks milder than the month inside it reads as a bug. */}
      <p className="text-muted">
        A longer window can show a smaller fall: it may begin before the drop or end after the
        recovery.
      </p>
      {missing.length ? (
        <p className="text-muted">
          Left out, because nothing is known about their past: {missing.join(', ')}.
        </p>
      ) : null}
    </>
  )
}
