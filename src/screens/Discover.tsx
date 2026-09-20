import { useEffect, useState } from 'react'
import { get } from '../api'
import { pct, signed } from '../format'
import { Corr } from '../Corr'
import { Stat } from '../Stat'

type Pick = { symbol: string; name: string; weight_pct: number; risk_pct: number }
type Outcome = {
  total_pct: number
  vol: number
  drawdown: number
  cost_pct: number
  days: number
}
type Result = {
  picks: Pick[]
  train: Outcome
  test: Outcome
  equal_weight: Outcome
  listed: number
  usable: number
  prefiltered: number
  combos: number
  exhaustive: boolean
  method: string
  cost_bps: number
  unusable: string[]
  correlation: number[][]
  div_ratio: number
}

const METHODS: [string, string][] = [
  ['minvar', 'Minimum variance'],
  ['sharpe', 'Maximum Sharpe'],
]

export function Discover() {
  const [markets, setMarkets] = useState<string[]>([])
  const [market, setMarket] = useState('')
  const [n, setN] = useState(5)
  const [years, setYears] = useState(5)
  const [method, setMethod] = useState('minvar')
  const [cost, setCost] = useState(25)
  const [out, setOut] = useState<Result | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    get<{ markets: string[] }>('/api/markets')
      .then((r) => {
        setMarkets(r.markets)
        setMarket(r.markets[0] ?? '')
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [])

  const run = async () => {
    setBusy(true)
    setError('')
    setOut(null)
    try {
      const q = new URLSearchParams({
        market,
        n: String(n),
        years: String(years),
        method,
        cost_bps: String(cost),
      })
      setOut(await get<Result>(`/api/discover?${q.toString()}`))
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="screen">
      <section className="panel">
        <div className="panel-head">Search</div>
        <div className="shock-inputs">
          {markets.map((m) => (
            <button
              key={m}
              className={`btn ${market === m ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setMarket(m)}
            >
              {m}
            </button>
          ))}
        </div>
        <div className="shock-inputs">
          {METHODS.map(([m, label]) => (
            <button
              key={m}
              className={`btn ${method === m ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setMethod(m)}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="shock-inputs">
          <label>
            <span className="text-muted">Stocks</span>
            <input
              className="input num"
              type="number"
              min={1}
              max={12}
              value={n}
              onChange={(e) => setN(Number(e.target.value))}
            />
          </label>
          <label>
            <span className="text-muted">Years of history</span>
            <input
              className="input num"
              type="number"
              min={2}
              max={10}
              value={years}
              onChange={(e) => setYears(Number(e.target.value))}
            />
          </label>
          <label>
            <span className="text-muted">Cost per trade, bps</span>
            <input
              className="input num"
              type="number"
              min={0}
              max={500}
              value={cost}
              onChange={(e) => setCost(Number(e.target.value))}
            />
          </label>
          <button className="btn btn-primary" onClick={() => void run()} disabled={busy || !market}>
            <i className="ph ph-magnifying-glass" />
            {busy ? 'Searching' : 'Search'}
          </button>
        </div>
        <p className="warn-soft">
          The basket is chosen on the earlier seven tenths of the window and then frozen. The rest
          of the window is read once, afterwards, and reported below next to an equal-weight
          portfolio of the same names. Choosing the best few out of a hundred is a reliable way to
          find something that looks excellent by luck, and the held-out figure is the only thing
          here that can tell you whether that is what happened.
        </p>
        <p className="text-muted">
          A market searched for the first time fetches five years of prices for every listing in
          it, which takes a minute. It is cached afterwards.
        </p>
      </section>

      {error ? <div className="banner bad">{error}</div> : null}
      {out ? <Found out={out} /> : null}
    </div>
  )
}

function Found({ out }: { out: Result }) {
  if (!out.picks.length) {
    return (
      <p className="text-muted">
        Nothing to report. {out.usable} of {out.listed} listings had usable history over this
        window, and a search needs more than a year of it either side of the split.
      </p>
    )
  }

  const beat = out.test.total_pct - out.equal_weight.total_pct

  return (
    <>
      <section className="panel">
        <div className="panel-head">
          {out.picks.length} of {out.usable}, by {out.exhaustive ? 'exhaustive search' : 'greedy selection'}
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>Ticker</th>
              <th>Name</th>
              <th className="num">Weight</th>
              <th className="num">Share of risk</th>
            </tr>
          </thead>
          <tbody>
            {out.picks.map((p) => (
              <tr key={p.symbol}>
                <td>{p.symbol}</td>
                <td className="text-muted">{p.name}</td>
                <td className="num">{pct(p.weight_pct)}</td>
                <td className="num">{pct(p.risk_pct)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="panel">
        <div className="panel-head">
          How the picks move together, diversification {out.div_ratio.toFixed(2)}
        </div>
        <Corr symbols={out.picks.map((p) => p.symbol)} matrix={out.correlation} />
        <p className="text-muted">
          Red is moving together, blue is moving apart. A basket of five names that all sit above
          0.8 is one bet in five pieces, and the ratio above says how much holding them together
          bought: one means nothing at all.
          {out.method === 'minvar'
            ? ' Share of risk matches weight exactly here, and will under any minimum-variance result: equalising marginal risk is what the objective solves for. Under maximum Sharpe the two columns come apart.'
            : ''}
        </p>
      </section>

      {/* The verdict comes before the numbers, because it is the thing most likely to be skipped. */}
      <div className="callout">
        <i className={`ph ${beat < 0 ? 'ph-warning' : 'ph-check-circle'}`} />
        <div>
          <div>
            {beat < 0
              ? `Equal weight over the same ${out.picks.length} names did better on the held-out window, by ${(-beat).toFixed(1)} percentage points.`
              : `The chosen weights beat equal weight over the same names by ${beat.toFixed(1)} percentage points on the held-out window.`}
          </div>
          <div className="text-muted">
            {beat < 0
              ? 'The weights are not earning their keep here. The names may still be worth something; the optimisation of them is not.'
              : 'A margin this size over a single held-out window is evidence, not proof.'}
          </div>
        </div>
      </div>

      <section className="panel">
        <div className="panel-head">Chosen window against held-out window</div>
        <table className="table">
          <thead>
            <tr>
              <th>Window</th>
              <th className="num">Total</th>
              <th className="num">Volatility</th>
              <th className="num">Worst drawdown</th>
              <th className="num">Costs</th>
              <th className="num">Days</th>
            </tr>
          </thead>
          <tbody>
            <Row label="Chosen on (in sample)" o={out.train} />
            <Row label="Held out" o={out.test} />
            <Row label="Held out, equal weight" o={out.equal_weight} />
          </tbody>
        </table>
      </section>

      <div className="stat-row">
        <Stat label="Listings searched" value={`${out.usable} of ${out.listed}`} />
        <Stat label="After pre-filter" value={String(out.prefiltered)} />
        <Stat
          label="Combinations"
          value={out.exhaustive ? out.combos.toLocaleString('nb-NO') : 'greedy'}
        />
        <Stat label="Cost per trade" value={`${out.cost_bps} bps`} />
      </div>

      <p className="text-muted">
        Rebalanced quarterly back to these weights, with costs charged on what each rebalance
        actually trades. Between rebalances the weights drift, as they would in a real account.
        {out.exhaustive
          ? ' Every combination of these candidates was evaluated.'
          : ' Too many combinations to try them all, so candidates were added one at a time, keeping whichever helped most. That is not guaranteed to be the best basket.'}
      </p>

      {out.unusable.length ? (
        <p className="text-muted">
          No usable history over this window, so they were never candidates:{' '}
          {out.unusable.join(', ')}. Recent listings and delistings both land here.
        </p>
      ) : null}
    </>
  )
}

function Row({ label, o }: { label: string; o: Outcome }) {
  return (
    <tr>
      <td>{label}</td>
      <td className={`num ${o.total_pct >= 0 ? 'up' : 'down'}`}>{signed(o.total_pct)}%</td>
      <td className="num">{pct(o.vol)}</td>
      <td className="num">-{pct(o.drawdown)}</td>
      <td className="num text-muted">-{pct(o.cost_pct)}</td>
      <td className="num text-muted">{o.days}</td>
    </tr>
  )
}
