/// What the portfolio is a bet on.
///
/// Oslo Børs is an oil, gas and shipping index wearing a national flag, so the useful question is
/// not what the holdings did but what they are exposed to. One driver, one window, one slope per
/// listing, with the correlation beside it so a slope fitted to noise can be seen for what it is.

import { useEffect, useState } from 'react'
import { get } from '../api'
import { pct, signed } from '../format'
import { SkelScreen } from '../Skeleton'
import { Stat } from '../Stat'
import { useApp } from '../store'

type Driver = { symbol: string; name: string; kind: string }
type Row = {
  ticker: string
  name: string
  kind: string
  beta: number
  correlation: number
  vol_pct: number
  total_pct: number
  owned: boolean
}
type Report = {
  driver: string
  driver_name: string
  driver_kind: string
  driver_total_pct: number
  driver_vol_pct: number
  from: string
  to: string
  observations: number
  rows: Row[]
  unusable: string[]
}

const YEARS = [1, 3, 5, 10]

// Anything under this is not a relationship, it is a slope fitted to noise.
const WEAK = 0.3

export function Energy() {
  const { state, pid } = useApp()
  const [drivers, setDrivers] = useState<Driver[]>([])
  const [driver, setDriver] = useState('BZ=F')
  const [years, setYears] = useState(3)
  const [report, setReport] = useState<Report | null>(null)
  const [error, setError] = useState('')
  const [mine, setMine] = useState(false)

  const p = state?.portfolios.find((x) => x.id === pid) ?? state?.portfolios[0]
  const id = p?.id

  useEffect(() => {
    get<{ drivers: Driver[] }>('/api/drivers')
      .then((d) => setDrivers(d.drivers))
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    if (!id) return
    setReport(null)
    setError('')
    get<Report>(
      `/api/energy?portfolio=${encodeURIComponent(id)}&driver=${encodeURIComponent(driver)}&years=${years}`,
    )
      .then(setReport)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [id, driver, years])

  if (!state) return <SkelScreen />
  if (!p) return <p className="text-muted">No portfolios yet.</p>

  const rows = report ? report.rows.filter((r) => !mine || r.owned) : []
  const held = report ? report.rows.filter((r) => r.owned) : []
  // The portfolio's own exposure: each holding's beta weighted by what it is worth. Holdings
  // with no measurable history are simply absent, which understates it rather than inventing it.
  const weighted = held.reduce((a, r) => {
    const h = p.holdings.find((x) => x.ticker === r.ticker)
    return a + (h && h.priced && p.value > 0 ? (h.value / p.value) * r.beta : 0)
  }, 0)

  return (
    <div className="screen">
      <section className="panel">
        <div className="panel-head">Measured against</div>
        <div className="shock-inputs">
          {drivers.map((d) => (
            <button
              key={d.symbol}
              className={`btn ${driver === d.symbol ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setDriver(d.symbol)}
            >
              {d.name}
            </button>
          ))}
        </div>
        <div className="shock-inputs">
          <span className="text-muted">over</span>
          {YEARS.map((y) => (
            <button
              key={y}
              className={`btn ${years === y ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setYears(y)}
            >
              {y} {y === 1 ? 'year' : 'years'}
            </button>
          ))}
          <label>
            <input type="checkbox" checked={mine} onChange={(e) => setMine(e.target.checked)} />
            <span className="text-muted">Only what I hold</span>
          </label>
        </div>
      </section>

      {error ? <div className="banner bad">{error}</div> : null}
      {!report && !error ? <SkelScreen /> : null}

      {report && report.rows.length === 0 ? (
        <p className="text-muted">
          Nothing could be measured against {report.driver_name}.
          {report.unusable.includes(report.driver)
            ? ' Its own history is missing or too short for this window, so there is nothing to measure against.'
            : ''}
        </p>
      ) : null}

      {report && report.rows.length > 0 ? (
        <>
          <div className="stat-row">
            <Stat
              label={`${report.driver_name} over the window`}
              value={`${signed(report.driver_total_pct)}%`}
              tone={report.driver_total_pct < 0 ? 'down' : 'up'}
            />
            <Stat label="Its volatility, annualised" value={pct(report.driver_vol_pct)} />
            <Stat
              label="This portfolio's exposure"
              value={held.length ? weighted.toFixed(2) : 'nothing measurable'}
            />
            <Stat
              label="Measured over"
              value={`${report.observations} days`}
            />
          </div>

          <p className="warn-soft">
            Beta is the slope: 1.40 means this listing moved 1.4% for each 1% in{' '}
            {report.driver_name}, on average, over {report.from} to {report.to}. The correlation
            beside it says how much of the movement that slope explains. A large beta with a
            correlation under {WEAK} is a line fitted through noise and should be read as no
            relationship rather than a big one. The portfolio figure above is each holding&apos;s
            beta weighted by what it is worth, so it is what a 1% move in {report.driver_name}{' '}
            has historically meant for the whole account.
          </p>

          <table className="table">
            <thead>
              <tr>
                <th>Ticker</th>
                <th>Name</th>
                <th>Carries</th>
                <th className="num">Beta</th>
                <th className="num">Correlation</th>
                <th className="num">Volatility</th>
                <th className="num">Over the window</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.ticker} className={r.owned ? 'breached' : undefined}>
                  <td>{r.ticker}</td>
                  <td className="text-muted">{r.name}</td>
                  <td className="text-muted">{r.kind}</td>
                  <td className="num">{r.beta.toFixed(2)}</td>
                  <td
                    className={`num ${Math.abs(r.correlation) < WEAK ? 'text-muted' : ''}`}
                  >
                    {r.correlation.toFixed(2)}
                  </td>
                  <td className="num">{pct(r.vol_pct)}</td>
                  <td className={`num ${r.total_pct < 0 ? 'down' : 'up'}`}>
                    {signed(r.total_pct)}%
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="text-muted">
            Rows marked down the left are ones this portfolio holds.
            {report.unusable.length
              ? ` Left out, because their history does not reach back ${years} ${
                  years === 1 ? 'year' : 'years'
                }: ${report.unusable.join(', ')}.`
              : ''}
          </p>
          <p className="text-muted">
            The freight drivers are funds holding freight futures, not the Baltic Exchange&apos;s
            indices, which are licensed and not in this app. A fund carries its own costs and
            rolls its contracts, so it tracks the rate it follows loosely rather than exactly.
          </p>
        </>
      ) : null}
    </div>
  )
}
