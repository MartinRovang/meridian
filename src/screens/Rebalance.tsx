import { useEffect, useState } from 'react'
import { get } from '../api'
import { money, pct, signed } from '../format'
import { useApp } from '../store'

type Trade = {
  id: string; ticker: string; name: string
  side: 'buy' | 'sell'; shares: number; amount: number
}

const DEBOUNCE_MS = 300

export function Rebalance() {
  const { state, pid, setScreen } = useApp()
  const [cash, setCash] = useState('0')
  const [trades, setTrades] = useState<Trade[] | null>(null)
  // A refusal is a state of its own, not an empty list: an empty list reads as "nothing to do",
  // which is the opposite of what a 409 means.
  const [refused, setRefused] = useState('')

  const p = state?.portfolios.find((x) => x.id === pid) ?? state?.portfolios[0]
  const id = p?.id

  useEffect(() => {
    if (!id) return
    const t = setTimeout(() => {
      const n = Number(cash)
      get<{ trades: Trade[] }>(
        `/api/trades?portfolio=${encodeURIComponent(id)}&cash=${Number.isFinite(n) ? n : 0}`,
      )
        .then((r) => {
          setTrades(r.trades)
          setRefused('')
        })
        .catch((e: unknown) => {
          setTrades(null)
          setRefused(e instanceof Error ? e.message : String(e))
        })
    }, DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [id, cash])

  if (!state) return null
  if (!p) return <p className="text-muted">No portfolios yet.</p>

  const rows = state.drift.find((d) => d.id === p.id)?.rows ?? []
  const base = state.base_currency

  return (
    <div className="screen">
      <div className="row-controls">
        <label className="field">
          <span>Cash to deploy, {base}</span>
          <input
            className="input narrow"
            type="number"
            step="any"
            value={cash}
            onChange={(e) => setCash(e.target.value)}
          />
        </label>
        <span className="text-muted">
          {Number(cash) > 0
            ? 'Cash is spent buying towards target. Nothing is sold.'
            : `Drift band is ±${pct(p.band_pct)}.`}
        </span>
      </div>

      <table className="table">
        <thead>
          <tr>
            <th>Holding</th>
            <th className="num">Target</th>
            <th className="num">Actual</th>
            <th className="num">Drift</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.id} className={r.breached ? 'breached' : ''}>
              <td>
                {r.ticker} <span className="text-muted">{r.name}</span>
              </td>
              <td className="num">{pct(r.target_pct)}</td>
              <td className="num">{pct(r.actual_pct)}</td>
              <td className="num">{signed(r.drift_pct)}pp</td>
              <td className="num">
                <span className={`tag ${r.breached ? 'tag-outline' : 'tag-neutral'}`}>
                  {r.breached ? 'breached' : 'in band'}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <section className="panel">
        <div className="panel-head">Proposed trades</div>
        {refused ? (
          <div className="refusal">
            <i className="ph ph-warning-diamond" />
            <div>
              <div>{refused}</div>
              <div className="text-muted">
                Rebalancing needs targets that add up.{' '}
                <a href="#builder" onClick={() => setScreen('builder')}>
                  Fix them in Builder
                </a>
                .
              </div>
            </div>
          </div>
        ) : trades === null ? (
          <p className="text-muted">Working it out.</p>
        ) : trades.length === 0 ? (
          <p className="text-muted">Nothing to do: every holding is within its band.</p>
        ) : (
          <div className="legend">
            {trades.map((t) => (
              <div key={t.id} className="trade">
                {/* The word, not just the colour: a red icon alone is not a distinction. */}
                <span className={t.side === 'buy' ? 'up' : 'down'}>
                  {t.side === 'buy' ? 'Buy' : 'Sell'}
                </span>
                <span className="num">{t.shares.toFixed(2)}</span>
                <span className="legend-name">
                  {t.ticker} <span className="text-muted">{t.name}</span>
                </span>
                <span className="num text-muted">about {money(t.amount, base)}</span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
