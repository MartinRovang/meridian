import { useEffect, useState } from 'react'
import { get, post } from '../api'
import { pct, signed } from '../format'
import { SkelScreen } from '../Skeleton'
import { Corr } from '../Corr'
import { Stat } from '../Stat'
import { useApp } from '../store'

type Row = {
  id: string
  ticker: string
  name: string
  target_pct: number
  suggested_pct: number
  risk_now_pct: number
  risk_pct: number
  optimised: boolean
}
type Proposal = {
  rows: Row[]
  excluded: string[]
  vol_current: number
  vol_suggested: number
  method: string
  budget_pct: number
  observations: number
  symbols: string[]
  correlation: number[][]
  div_current: number
  div_suggested: number
}

// A move smaller than this is a rounding artefact, not a trade: without the floor a -0.04pp
// change renders as "-0.0pp", which reads as a bug and invites a trade worth nothing.
const FLOOR = 0.05

const METHODS: [string, string][] = [
  ['minvar', 'Minimum variance'],
  ['invvol', 'Inverse volatility'],
]

export function Optimize() {
  const { state, pid, setScreen, load } = useApp()
  const [method, setMethod] = useState('minvar')
  const [plan, setPlan] = useState<Proposal | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  const p = state?.portfolios.find((x) => x.id === pid) ?? state?.portfolios[0]
  const id = p?.id

  useEffect(() => {
    if (!id) return
    setPlan(null)
    setError('')
    get<Proposal>(`/api/optimize?portfolio=${encodeURIComponent(id)}&method=${method}`)
      .then(setPlan)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [id, method])

  if (!state) return <SkelScreen />
  if (!p) return <p className="text-muted">No portfolios yet.</p>

  const apply = async () => {
    if (!plan || !id) return
    setBusy(true)
    try {
      await post('/api/targets', {
        portfolio: id,
        targets: plan.rows.map((r) => ({ id: r.id, pct: r.suggested_pct })),
      })
      await load()
      setScreen('rebalance')
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const measured = plan?.rows.filter((r) => r.optimised) ?? []
  // A proposal identical to what is already set is not worth a trade, and the button should not
  // imply otherwise.
  const moves = measured.some((r) => Math.abs(r.suggested_pct - r.target_pct) > FLOOR)

  return (
    <div className="screen">
      <section className="panel">
        <div className="panel-head">Objective</div>
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
        <p className="warn-soft">
          Neither objective uses a forecast of what anything will return. They use only how these
          holdings have moved together, which is measured far more reliably than return is: an
          optimiser fed five years of measured returns will put the portfolio into whatever
          happened to go up. The covariance is still the past, and the past is not a promise.
        </p>
      </section>

      {error ? <div className="banner bad">{error}</div> : null}
      {!plan && !error ? <SkelScreen /> : null}

      {plan ? (
        measured.length < 2 ? (
          <p className="text-muted">
            Not enough measurable holdings to optimise: at least two need a year of history.
            {plan.excluded.length ? ` Nothing is known about ${plan.excluded.join(', ')}.` : ''}
          </p>
        ) : (
          <>
            <div className="stat-row">
              <Stat label="Volatility now" value={pct(plan.vol_current)} />
              <Stat
                label="Volatility suggested"
                value={pct(plan.vol_suggested)}
                tone={plan.vol_suggested < plan.vol_current ? 'up' : 'down'}
              />
              <Stat label="Measured over" value={`${plan.observations} days`} />
              <Stat
                label="Diversification"
                value={`${plan.div_current.toFixed(2)} to ${plan.div_suggested.toFixed(2)}`}
              />
            </div>

            <section className="panel">
              <div className="panel-head">Suggested targets</div>
              <table className="table">
                <thead>
                  <tr>
                    <th>Ticker</th>
                    <th>Name</th>
                    <th className="num">Target now</th>
                    <th className="num">Suggested</th>
                    <th className="num">Change</th>
                    <th className="num">Risk now</th>
                    <th className="num">Risk after</th>
                  </tr>
                </thead>
                <tbody>
                  {plan.rows.map((r) => (
                    <tr key={r.id}>
                      <td>{r.ticker}</td>
                      <td className="text-muted">{r.name}</td>
                      <td className="num">{pct(r.target_pct)}</td>
                      <td className="num">{pct(r.suggested_pct)}</td>
                      {r.optimised ? (
                        <td className="num">
                          {Math.abs(r.suggested_pct - r.target_pct) < FLOOR
                            ? '--'
                            : `${signed(r.suggested_pct - r.target_pct)}pp`}
                        </td>
                      ) : (
                        // Not zero: its target was never considered, which is a different thing
                        // from having been considered and left alone.
                        <td className="text-muted">no history, kept as is</td>
                      )}
                      {r.optimised ? (
                        <>
                          <td className="num">{pct(r.risk_now_pct)}</td>
                          <td className="num">{pct(r.risk_pct)}</td>
                        </>
                      ) : (
                        <td className="text-muted" colSpan={2} />
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="stat-actions">
                <button
                  className="btn btn-primary"
                  onClick={() => void apply()}
                  disabled={busy || !moves}
                >
                  <i className="ph ph-check" />
                  {busy ? 'Writing' : moves ? 'Apply targets and review trades' : 'Already set'}
                </button>
              </div>
            </section>

            <section className="panel">
              <div className="panel-head">How these move together</div>
              <Corr symbols={plan.symbols} matrix={plan.correlation} />
              <p className="text-muted">
                Red is moving together, blue is moving apart. Two holdings at 0.9 are close to one
                holding at twice the size, whatever the allocation chart shows. The risk columns
                above say the same thing per holding: a fifth of the money can be a third of the
                risk.
                {method === 'minvar'
                  ? ' Under minimum variance the two right-hand columns always agree, because equalising the marginal risk of every holding is exactly what this objective solves for. The left one, measured on the targets you hold now, is the one with something to say.'
                  : ''}
              </p>
            </section>

            <p className="text-muted">
              Applying writes target weights only. It buys and sells nothing: Rebalance turns the
              new targets into a trade list, and moving there is where the cost of the change
              becomes visible.
            </p>

            {plan.excluded.length ? (
              <p className="text-muted">
                Left out for want of a year of history: {plan.excluded.join(', ')}. Their targets
                are carried over untouched, which is why the optimizer had{' '}
                {pct(plan.budget_pct)} to work with rather than all of it.
              </p>
            ) : null}
          </>
        )
      ) : null}
    </div>
  )
}
