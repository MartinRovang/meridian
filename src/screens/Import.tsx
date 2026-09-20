import { useEffect, useRef, useState } from 'react'
import { get, post, postFile } from '../api'
import { money, price } from '../format'
import { SkelHits } from '../Skeleton'

type Action = 'new' | 'update' | 'unchanged' | 'unmatched'

type Change = {
  name: string; currency: string; last: number | null
  shares: number; cost_basis: number
  action: Action; ticker: string | null; holding_id: string | null
  was_shares: number | null; was_cost_basis: number | null
}
type Plan = { changes: Change[]; absent: string[] }
type Candidate = {
  symbol: string; name: string; exchange: string
  currency: string; price: number; currency_ok: boolean; exact: boolean
}

const VERBS: Record<Action, string> = {
  new: 'add',
  update: 'update',
  unchanged: 'no change',
  unmatched: 'needs a ticker',
}

export function Import({ pid, onDone }: { pid: string; onDone: () => void }) {
  const [plan, setPlan] = useState<Plan | null>(null)
  const [reading, setReading] = useState(false)
  const [error, setError] = useState('')
  // Broker name -> the ticker the user settled on, whether picked or typed.
  const [picked, setPicked] = useState<Record<string, string>>({})
  // Broker name -> candidates, or null while the lookup is in flight.
  const [cands, setCands] = useState<Record<string, Candidate[] | null>>({})
  const [applying, setApplying] = useState(false)
  const file = useRef<HTMLInputElement>(null)

  const take = async (f: File | undefined) => {
    if (!f) return
    setReading(true)
    setError('')
    setPlan(null)
    setPicked({})
    setCands({})
    try {
      setPlan(await postFile<Plan>(`/api/import/preview?portfolio=${encodeURIComponent(pid)}`, await f.arrayBuffer()))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
    setReading(false)
  }

  // One lookup per unmatched row, once the preview lands. The rows the file already resolves are
  // never looked up: that is the whole point of remembering them.
  useEffect(() => {
    if (!plan) return
    for (const row of plan.changes.filter((c) => c.action === 'unmatched')) {
      if (row.name in cands) continue
      setCands((s) => ({ ...s, [row.name]: null }))
      const q = new URLSearchParams({ name: row.name, currency: row.currency })
      if (row.last != null) q.set('last', String(row.last))
      get<{ candidates: Candidate[] }>(`/api/import/candidates?${q}`)
        .then((r) => {
          setCands((s) => ({ ...s, [row.name]: r.candidates }))
          // The broker printed a price and one listing matches it. Preselected, not applied: the
          // user still sees it and still presses the button.
          const best = r.candidates.find((x) => x.exact)
          if (best) setPicked((s) => (s[row.name] ? s : { ...s, [row.name]: best.symbol }))
        })
        .catch(() => setCands((s) => ({ ...s, [row.name]: [] })))
    }
  }, [plan, cands])

  const ticker = (c: Change) => c.ticker ?? picked[c.name] ?? ''
  // An unchanged row is skipped: applying it would rewrite the file with the numbers already in it.
  const writes = (plan?.changes ?? []).filter((c) => c.action !== 'unchanged' && ticker(c))
  const waiting = (plan?.changes ?? []).filter((c) => !ticker(c))

  const apply = async () => {
    setApplying(true)
    setError('')
    try {
      await post('/api/import/apply', {
        portfolio_id: pid,
        rows: writes.map((c) => ({
          ticker: ticker(c),
          name: c.name,
          shares: c.shares,
          cost_basis: c.cost_basis,
          cost_currency: c.currency,
        })),
        aliases: picked,
      })
      setPlan(null)
      onDone()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
    setApplying(false)
  }

  return (
    <section className="panel">
      <div className="panel-head">Import from a broker</div>

      <div
        className="drop"
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault()
          void take(e.dataTransfer.files[0])
        }}
        onClick={() => file.current?.click()}
      >
        <i className="ph ph-file-arrow-up" />
        <div>
          <div>Drop a positions export here, or click to choose one</div>
          <div className="text-muted">
            Nothing is written until you have seen what it would change.
          </div>
        </div>
        <input
          ref={file}
          type="file"
          hidden
          onChange={(e) => {
            void take(e.target.files?.[0])
            e.target.value = '' // so choosing the same file twice still fires
          }}
        />
      </div>

      {error ? <div className="banner bad">{error}</div> : null}
      {reading ? <SkelHits n={2} /> : null}

      {plan ? (
        <>
          <table className="table">
            <thead>
              <tr>
                <th>In the file</th>
                <th>Ticker</th>
                <th className="num">Shares</th>
                <th className="num">Cost</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {plan.changes.map((c) => (
                <tr key={c.name}>
                  <td>
                    {c.name}
                    {c.last != null ? (
                      <span className="text-muted"> at {price(c.last, c.currency)}</span>
                    ) : null}
                  </td>
                  <td>
                    {c.ticker ?? (
                      <Picker
                        found={cands[c.name]}
                        chosen={picked[c.name] ?? ''}
                        onChoose={(t) => setPicked((s) => ({ ...s, [c.name]: t.toUpperCase() }))}
                      />
                    )}
                  </td>
                  <td className="num">
                    {c.was_shares != null && c.was_shares !== c.shares ? (
                      <span className="text-muted">{c.was_shares} to </span>
                    ) : null}
                    {c.shares}
                  </td>
                  <td className="num">{money(c.cost_basis, c.currency)}</td>
                  <td className={c.action === 'unmatched' && !picked[c.name] ? 'warn' : 'text-muted'}>
                    {picked[c.name] && c.action === 'unmatched' ? 'add' : VERBS[c.action]}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {plan.absent.length ? (
            <p className="text-muted">
              Held here but not in this file, and left alone: {plan.absent.join(', ')}. One file is
              one account.
            </p>
          ) : null}

          <div className="row-controls">
            <button className="btn btn-primary" disabled={!writes.length || applying} onClick={() => void apply()}>
              {applying ? 'Writing' : `Import ${writes.length}`}
            </button>
            <button className="btn btn-ghost" onClick={() => setPlan(null)}>
              Cancel
            </button>
            <span className="text-muted">
              {waiting.length
                ? `${waiting.length} still need a ticker and will be skipped.`
                : 'Imported holdings start with no target, so set one before rebalancing.'}
            </span>
          </div>
        </>
      ) : null}
    </section>
  )
}

/// Pick a listing, or type one. The search is a help, not a gate: Yahoo returns nothing at all for
/// some fund names, and a user who knows the symbol should not be stuck behind a lookup.
function Picker({
  found,
  chosen,
  onChoose,
}: {
  found: Candidate[] | null | undefined
  chosen: string
  onChoose: (t: string) => void
}) {
  if (found === null || found === undefined) return <SkelHits n={1} />
  return (
    <div className="picker">
      {found.length ? (
        <select className="input" value={chosen} onChange={(e) => onChoose(e.target.value)}>
          <option value="">Pick a listing</option>
          {found.map((c) => (
            <option key={c.symbol} value={c.symbol}>
              {c.symbol} {c.exchange} {c.price.toFixed(2)} {c.currency}
              {c.exact ? ' (matches the price)' : c.currency_ok ? '' : ' (other currency)'}
            </option>
          ))}
        </select>
      ) : null}
      <input
        className="input cell"
        placeholder="or a symbol"
        defaultValue={found.length ? '' : chosen}
        onBlur={(e) => e.target.value.trim() && onChoose(e.target.value.trim())}
      />
    </div>
  )
}
