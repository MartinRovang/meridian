import { useState } from 'react'
import { del, patch, post } from '../api'
import { pct } from '../format'
import { Import } from './Import'
import { SkelScreen } from '../Skeleton'
import { TickerSearch, type Hit } from '../TickerSearch'
import { useApp, type Holding } from '../store'

// Every mutation ends in load(): the screen shows what the server stored, never what the client
// hoped it stored.
const reload = () => useApp.getState().load()

type Draft = { ticker: string; name: string; cls: string; shares: string; cost: string; ccy: string; target: string }
const EMPTY: Draft = { ticker: '', name: '', cls: '', shares: '', cost: '', ccy: '', target: '' }

export function Builder() {
  const { state, pid, setPid } = useApp()
  const [draft, setDraft] = useState<Draft>(EMPTY)
  const [err, setErr] = useState('')
  // ponytail: an inline input rather than window.prompt, which is the one JS dialog the
  // packaged webview cannot be relied on to show. confirm() stays: it is implemented, and if it
  // ever were not, the failure is "the delete does not happen".
  const [naming, setNaming] = useState<{ mode: 'new' | 'rename'; value: string } | null>(null)
  if (!state) return <SkelScreen />

  const p = state.portfolios.find((x) => x.id === pid) ?? state.portfolios[0]
  const run = async (f: () => Promise<unknown>) => {
    setErr('')
    try {
      await f()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
      return
    }
    await reload()
  }

  const commitName = () => {
    const name = naming?.value.trim()
    if (!name) return
    const mode = naming!.mode
    setNaming(null)
    void run(async () => {
      if (mode === 'new') {
        const r = await post<{ id: string }>('/api/portfolio', { name, owner: '' })
        setPid(r.id)
      } else {
        await patch(`/api/portfolio/${p.id}`, { name })
      }
    })
  }

  const deletePortfolio = () => {
    // Named, and with the count, because the holdings go with it.
    const n = p.holdings.length
    const what = n === 1 ? 'its 1 holding' : `its ${n} holdings`
    if (!confirm(`Delete "${p.name}" and ${what}? This cannot be undone.`)) return
    void run(async () => {
      await patch(`/api/portfolio/${p.id}`, { delete: true })
      setPid('')
    })
  }

  const saveHolding = (h: Holding, field: string, raw: string) => {
    const n = Number(raw)
    if (!Number.isFinite(n)) return
    void run(() =>
      post('/api/holding', {
        // The id makes this an update. Without it the server would push a duplicate row.
        id: h.id,
        portfolio_id: p.id,
        ticker: h.ticker,
        name: h.name,
        cls: h.cls,
        shares: field === 'shares' ? n : h.shares,
        cost_basis: field === 'cost' ? n : h.cost_basis,
        cost_currency: h.cost_currency,
        target_pct: field === 'target' ? n : h.target_pct,
      }),
    )
  }

  const pick = (hit: Hit) =>
    setDraft({ ...draft, ticker: hit.symbol, name: hit.name, ccy: hit.currency })

  const addHolding = () => {
    // ponytail: the server accepts a second row for the same ticker, and a future "lots" feature
    // would want that. This app gives each row its own target_pct, so two rows for one ticker
    // means two targets for one position and drift stops meaning anything. Guard it here.
    const dup = p.holdings.find((h) => h.ticker === draft.ticker.toUpperCase())
    if (dup) {
      setErr(`${dup.ticker} is already in "${p.name}". Edit that row instead of adding a second.`)
      return
    }
    void run(async () => {
      await post('/api/holding', {
        portfolio_id: p.id,
        ticker: draft.ticker,
        name: draft.name,
        cls: draft.cls.trim() || 'Uncategorised',
        shares: Number(draft.shares || 0),
        cost_basis: Number(draft.cost || 0),
        cost_currency: draft.ccy || state.base_currency,
        target_pct: Number(draft.target || 0),
      })
      setDraft(EMPTY)
    })
  }

  const targetSum = p ? p.holdings.reduce((a, h) => a + h.target_pct, 0) : 0

  return (
    <div className="screen">
      <div className="stat-band">
        {naming ? (
          <div className="stat-actions">
            <input
              className="input pick"
              autoFocus
              placeholder="Portfolio name"
              value={naming.value}
              onChange={(e) => setNaming({ ...naming, value: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitName()
                if (e.key === 'Escape') setNaming(null)
              }}
            />
            <button className="btn btn-primary" disabled={!naming.value.trim()} onClick={commitName}>
              {naming.mode === 'new' ? 'Create' : 'Rename'}
            </button>
            <button className="btn btn-secondary" onClick={() => setNaming(null)}>
              Cancel
            </button>
          </div>
        ) : (
        <div className="stat-actions">
          <select className="input pick" value={p?.id ?? ''} onChange={(e) => setPid(e.target.value)}>
            {state.portfolios.map((x) => (
              <option key={x.id} value={x.id}>
                {x.name}
              </option>
            ))}
            {state.portfolios.length ? null : <option value="">No portfolios</option>}
          </select>
          <button className="btn btn-secondary" onClick={() => setNaming({ mode: 'new', value: '' })}>
            <i className="ph ph-plus" />
            New
          </button>
          {p ? (
            <>
              <button
                className="btn btn-secondary"
                onClick={() => setNaming({ mode: 'rename', value: p.name })}
              >
                Rename
              </button>
              <button className="btn btn-secondary" onClick={deletePortfolio}>
                Delete
              </button>
            </>
          ) : null}
        </div>
        )}
      </div>

      {err ? <div className="banner bad">{err}</div> : null}
      {!p ? <p className="text-muted">Create a portfolio to start adding holdings.</p> : null}

      {p ? (
        <>
          <div className="row-controls">
            <label className="field">
              <span>Drift band, ±%</span>
              <input
                className="input narrow"
                type="number"
                step="0.5"
                defaultValue={p.band_pct}
                onBlur={(e) => {
                  const n = Number(e.target.value)
                  if (Number.isFinite(n) && n !== p.band_pct) {
                    void run(() => patch(`/api/portfolio/${p.id}`, { band_pct: n }))
                  }
                }}
              />
            </label>
            {/* A warning, never a block: Rebalance is what refuses. */}
            <span className={Math.abs(targetSum - 100) < 0.05 ? 'text-muted' : 'warn'}>
              Targets sum to {pct(targetSum)}
              {Math.abs(targetSum - 100) < 0.05 ? '' : ', not 100%'}
            </span>
          </div>

          <table className="table">
            <thead>
              <tr>
                <th>Ticker</th>
                <th>Name</th>
                <th>Class</th>
                <th className="num">Shares</th>
                <th className="num">Cost basis</th>
                <th className="num">Target %</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {p.holdings.map((h) => (
                <tr key={h.id}>
                  <td>{h.ticker}</td>
                  <td className="text-muted">{h.name}</td>
                  <td className="text-muted">{h.cls}</td>
                  <td className="num">
                    <input
                      className="input cell"
                      type="number"
                      step="any"
                      defaultValue={h.shares}
                      onBlur={(e) => saveHolding(h, 'shares', e.target.value)}
                    />
                  </td>
                  <td className="num">
                    <input
                      className="input cell"
                      type="number"
                      step="any"
                      defaultValue={h.cost_basis}
                      onBlur={(e) => saveHolding(h, 'cost', e.target.value)}
                    />
                    <span className="text-muted unit">{h.cost_currency}</span>
                  </td>
                  <td className="num">
                    <input
                      className="input cell"
                      type="number"
                      step="any"
                      defaultValue={h.target_pct}
                      onBlur={(e) => saveHolding(h, 'target', e.target.value)}
                    />
                  </td>
                  <td className="num">
                    <button
                      className="btn btn-ghost"
                      title={`Remove ${h.ticker}`}
                      onClick={() => {
                        if (confirm(`Remove ${h.ticker} from "${p.name}"?`)) {
                          void run(() => del(`/api/holding/${h.id}`))
                        }
                      }}
                    >
                      <i className="ph ph-trash" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <section className="panel">
            <div className="panel-head">Add a holding · {state.scope} scope</div>
            <div className="add-row">
              <TickerSearch onPick={pick} />
              <input
                className="input"
                placeholder="Class"
                value={draft.cls}
                onChange={(e) => setDraft({ ...draft, cls: e.target.value })}
              />
              <input
                className="input narrow"
                type="number"
                step="any"
                placeholder="Shares"
                value={draft.shares}
                onChange={(e) => setDraft({ ...draft, shares: e.target.value })}
              />
              <input
                className="input narrow"
                type="number"
                step="any"
                placeholder="Cost basis"
                value={draft.cost}
                onChange={(e) => setDraft({ ...draft, cost: e.target.value })}
              />
              <input
                className="input narrow"
                type="number"
                step="any"
                placeholder="Target %"
                value={draft.target}
                onChange={(e) => setDraft({ ...draft, target: e.target.value })}
              />
              <button className="btn btn-primary" disabled={!draft.ticker} onClick={addHolding}>
                Add {draft.ticker}
              </button>
            </div>
            {draft.ticker ? (
              <p className="text-muted">
                {draft.name} · cost basis in {draft.ccy || state.base_currency}
              </p>
            ) : null}
          </section>

          <Import pid={p.id} onDone={() => void reload()} />
        </>
      ) : null}
    </div>
  )
}
