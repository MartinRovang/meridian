/// Saved conditions over the figures the app already computes.
///
/// A rule is one field, one comparison and one number. Two conditions that must both hold are
/// two rules: an expression language is a parser, a precedence table and an error message for
/// every way of writing one wrong, and nobody asked for that.

import { useEffect, useState } from 'react'
import { get, post } from '../api'
import { SkelScreen } from '../Skeleton'
import { useApp } from '../store'

type Field =
  | 'weight'
  | 'drift'
  | 'day_move'
  | 'pl_pct'
  | 'price'
  | 'portfolio_day_move'
  | 'class_weight'

type Rule = {
  id: string
  name: string
  enabled: boolean
  notify: boolean
  field: Field
  op: 'above' | 'below'
  value: number
  ticker: string
  cls: string
}

type Hit = {
  rule_id: string
  rule_name: string
  portfolio_id: string
  subject: string
  value: number
  text: string
}

// Label, and the unit that follows the number. The order is the order they appear in the box.
const FIELDS: [Field, string, string][] = [
  ['weight', 'A holding’s weight', '%'],
  ['drift', 'A holding’s drift from target', 'pp'],
  ['day_move', 'A holding’s move today', '%'],
  ['pl_pct', 'A holding’s profit or loss', '%'],
  ['price', 'A holding’s price', ''],
  ['portfolio_day_move', 'The portfolio’s move today', '%'],
  ['class_weight', 'A class’s share of the portfolio', '%'],
]

const holdingField = (f: Field) => f !== 'portfolio_day_move' && f !== 'class_weight'

const blank = (): Rule => ({
  id: '',
  name: '',
  enabled: true,
  notify: false,
  field: 'weight',
  op: 'above',
  value: 40,
  ticker: '',
  cls: '',
})

export function Rules() {
  const { state } = useApp()
  const [rules, setRules] = useState<Rule[] | null>(null)
  const [hits, setHits] = useState<Hit[]>([])
  const [error, setError] = useState('')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    get<Rule[]>('/api/rules')
      .then(setRules)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
    get<Hit[]>('/api/rules/hits')
      .then(setHits)
      .catch(() => undefined)
  }, [])

  if (!state) return <SkelScreen />
  if (error && !rules) return <div className="banner bad">{error}</div>
  if (!rules) return <SkelScreen />

  // Every class the portfolios actually hold, so a class rule is picked rather than spelled.
  const classes = [...new Set(state.portfolios.flatMap((p) => p.by_class.map((c) => c.cls)))]

  const edit = (i: number, patch: Partial<Rule>) =>
    setRules(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)))

  const save = async (next: Rule[]) => {
    setBusy(true)
    setError('')
    setNote('')
    try {
      setRules(await post<Rule[]>('/api/rules', next))
      setHits(await get<Hit[]>('/api/rules/hits'))
      setNote('Saved.')
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
    setBusy(false)
  }

  return (
    <div className="screen">
      {error ? <div className="banner bad">{error}</div> : null}

      <section className="panel">
        <div className="panel-head">
          Firing now
          <span className="text-muted"> {hits.length === 0 ? 'nothing' : `${hits.length}`}</span>
        </div>
        {hits.length === 0 ? (
          <p className="text-muted">
            No rule is currently true. This list is what the rules below say about the figures as
            they stand, recomputed when you save.
          </p>
        ) : (
          <div className="rule-hits">
            {hits.map((h) => (
              <div key={`${h.rule_id}:${h.subject}`} className="rule-hit">
                <span>{h.rule_name || 'Unnamed rule'}</span>
                <span className="text-muted">{h.text}</span>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="panel">
        <div className="panel-head">Rules</div>
        {rules.length === 0 ? (
          <p className="text-muted">
            None yet. A rule watches one figure: a weight, a drift, a day move, a profit, a price.
          </p>
        ) : null}

        {rules.map((r, i) => (
          <div className="shock-inputs rule-row" key={r.id || i}>
            <label>
              <input
                type="checkbox"
                checked={r.enabled}
                onChange={(e) => edit(i, { enabled: e.target.checked })}
              />
              <span className="text-muted">On</span>
            </label>
            <input
              className="input wide"
              value={r.name}
              placeholder="Name it, or don’t"
              onChange={(e) => edit(i, { name: e.target.value })}
            />
            <select
              className="input pick field-pick"
              value={r.field}
              onChange={(e) => edit(i, { field: e.target.value as Field })}
            >
              {FIELDS.map(([f, label]) => (
                <option key={f} value={f}>
                  {label}
                </option>
              ))}
            </select>
            {r.field === 'class_weight' ? (
              <select
                className="input pick"
                value={r.cls}
                onChange={(e) => edit(i, { cls: e.target.value })}
              >
                <option value="">pick a class</option>
                {classes.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            ) : null}
            {holdingField(r.field) ? (
              <input
                className="input cell"
                value={r.ticker}
                placeholder="any ticker"
                onChange={(e) => edit(i, { ticker: e.target.value })}
              />
            ) : null}
            <select
              className="input pick"
              value={r.op}
              onChange={(e) => edit(i, { op: e.target.value as 'above' | 'below' })}
            >
              <option value="above">is above</option>
              <option value="below">is below</option>
            </select>
            <input
              className="input num"
              type="number"
              step={0.5}
              value={r.value}
              onChange={(e) => edit(i, { value: Number(e.target.value) })}
            />
            <span className="text-muted">
              {FIELDS.find(([f]) => f === r.field)?.[2] || 'in its own currency'}
            </span>
            <label>
              <input
                type="checkbox"
                checked={r.notify}
                onChange={(e) => edit(i, { notify: e.target.checked })}
              />
              <span className="text-muted">Notify</span>
            </label>
            <button
              className="btn btn-ghost"
              onClick={() => void save(rules.filter((_, j) => j !== i))}
            >
              Remove
            </button>
          </div>
        ))}

        <div className="shock-inputs">
          <button className="btn btn-secondary" onClick={() => setRules([...rules, blank()])}>
            Add a rule
          </button>
          <button className="btn btn-primary" onClick={() => void save(rules)} disabled={busy}>
            {busy ? 'Saving' : 'Save'}
          </button>
          {note ? <span className="text-muted">{note}</span> : null}
        </div>
      </section>

      <p className="warn-soft">
        A rule marked Notify rides the alert loop, so it needs alerts switched on and a topic set
        on the Alerts screen, and it buzzes once per holding when it starts being true rather than
        every quarter hour while it stays true. Like every other notification it carries a ticker
        and a figure and never an amount or a total.
      </p>
      <p className="text-muted">
        Holdings with no usable price are skipped by every rule. Their weight and price read as
        zero because they are unknown, and a rule comparing against that would fire on the wrong
        thing.
      </p>
    </div>
  )
}
