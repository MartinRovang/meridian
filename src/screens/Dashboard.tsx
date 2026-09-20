import { useState } from 'react'
import { post } from '../api'
import { money, pct, price, signed } from '../format'
import { SkelScreen } from '../Skeleton'
import { useApp } from '../store'
import { Donut, hue } from './Donut'

// Gain and loss are the one thing Nocturne has no token for, so these come straight from the
// design file's own UP and DOWN constants. Everything else on this screen is a token.
const tone = (n: number) => (n > 0 ? 'up' : n < 0 ? 'down' : 'flat')

export function Dashboard() {
  const { state, pid, setPid, setScreen, load } = useApp()
  const [busy, setBusy] = useState(false)
  if (!state) return <SkelScreen />

  const p = state.portfolios.find((x) => x.id === pid) ?? state.portfolios[0]
  if (!p) {
    return (
      <p className="text-muted">
        No portfolios yet.{' '}
        <a href="#builder" onClick={() => setScreen('builder')}>
          Build one
        </a>
        .
      </p>
    )
  }

  const base = state.base_currency
  const breached = (state.drift.find((d) => d.id === p.id)?.rows ?? []).filter((r) => r.breached)

  const refresh = async () => {
    setBusy(true)
    await post('/api/refresh').catch(() => undefined)
    await load()
    setBusy(false)
  }

  return (
    <div className="screen">
      <div className="stat-band">
        <div className="stat-head">
          <div className="stat-total">{money(p.value, base)}</div>
          <div className={`stat-day ${tone(p.day_pct)}`}>
            {signed(p.day_pct)}% today
          </div>
          <div className={`stat-pl ${tone(p.pl)}`}>
            {money(p.pl, base)} total P/L
          </div>
        </div>
        <div className="stat-actions">
          {state.portfolios.length > 1 ? (
            <select className="input pick" value={p.id} onChange={(e) => setPid(e.target.value)}>
              {state.portfolios.map((x) => (
                <option key={x.id} value={x.id}>
                  {x.name}
                </option>
              ))}
            </select>
          ) : (
            <span className="text-muted">{p.owner}</span>
          )}
          <button className="btn btn-secondary" onClick={refresh} disabled={busy}>
            <i className="ph ph-arrows-clockwise" />
            {busy ? 'Fetching' : 'Refresh prices'}
          </button>
        </div>
      </div>

      {breached.length ? (
        <div className="callout">
          <i className="ph ph-scales" />
          <div>
            <div>
              Rebalance suggested — {breached.length}{' '}
              {breached.length === 1 ? 'holding is' : 'holdings are'} outside their ±
              {pct(p.band_pct)} band
            </div>
            <div className="text-muted">
              {breached.map((r) => r.ticker).join(', ')}
            </div>
          </div>
          <button className="btn btn-primary" onClick={() => setScreen('rebalance')}>
            Review trades
          </button>
        </div>
      ) : null}

      <div className="split">
        <section className="panel">
          <div className="panel-head">Allocation by class</div>
          <div className="alloc">
            <Donut slices={p.by_class} />
            <div className="legend">
              {p.by_class.map((s, i) => (
                <div key={s.cls} className="legend-row">
                  <span className="swatch" style={{ background: hue(i) }} />
                  <span className="legend-name">{s.cls}</span>
                  <span className="num">{pct(s.pct)}</span>
                  <span className="num text-muted">{money(s.value, base)}</span>
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">Drift against target</div>
          <div className="legend">
            {(state.drift.find((d) => d.id === p.id)?.rows ?? []).map((r) => (
              <div key={r.id} className="legend-row drift">
                <span className="legend-name">{r.ticker}</span>
                <span className="num">{pct(r.actual_pct)}</span>
                <span className="num text-muted">vs {pct(r.target_pct)}</span>
                <span className={`tag ${r.breached ? 'tag-outline' : 'tag-neutral'}`}>
                  {signed(r.drift_pct)}pp
                </span>
              </div>
            ))}
            {p.holdings.length === 0 ? <span className="text-muted">No holdings yet.</span> : null}
          </div>
        </section>
      </div>

      {p.unpriced > 0 ? (
        <p className="text-muted">
          {p.unpriced} {p.unpriced === 1 ? 'holding is' : 'holdings are'} unpriced and excluded from
          every total above.
        </p>
      ) : null}

      <table className="table">
        <thead>
          <tr>
            <th>Ticker</th>
            <th>Name</th>
            <th>Class</th>
            <th className="num">Shares</th>
            <th className="num">Price</th>
            <th className="num">Value</th>
            <th className="num">Day</th>
            <th className="num">P/L</th>
            <th className="num">Weight</th>
          </tr>
        </thead>
        <tbody>
          {p.holdings.map((h) => (
            <tr key={h.id}>
              <td>{h.ticker}</td>
              <td className="text-muted">{h.name}</td>
              <td className="text-muted">{h.cls}</td>
              <td className="num">{h.shares}</td>
              {h.priced ? (
                <>
                  <td className="num">{price(h.price, h.currency)}</td>
                  <td className="num">{money(h.value, base)}</td>
                  <td className={`num ${tone(h.day_pct)}`}>{signed(h.day_pct)}%</td>
                  <td className={`num ${tone(h.pl)}`}>{money(h.pl, base)}</td>
                  <td className="num">
                    {pct(h.weight_pct)} <span className="text-muted">vs {pct(h.target_pct)}</span>
                  </td>
                </>
              ) : (
                // Never zeros: a missing quote is not a price of nothing. Left-aligned so it
                // sits under Price, where the eye is already looking for a number.
                <td className="text-muted" colSpan={5}>
                  no quote
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
