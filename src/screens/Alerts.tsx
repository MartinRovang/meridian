import { useEffect, useState } from 'react'
import { get, post } from '../api'
import { SkelScreen } from '../Skeleton'
import { useApp } from '../store'

type Level = { id: string; ticker: string; price: number; above: boolean }
type Config = {
  enabled: boolean
  topic: string
  server: string
  drift: boolean
  big_move: boolean
  big_move_pct: number
  portfolio_move: boolean
  portfolio_move_pct: number
  stale: boolean
  levels: Level[]
}
type Firing = { key: string; text: string }

// Long enough that nobody finds it by guessing. An ntfy topic is the only thing standing between
// a stranger and your notifications, so it is generated rather than typed.
function makeTopic(): string {
  const raw = new Uint8Array(9)
  crypto.getRandomValues(raw)
  const hex = Array.from(raw, (b) => b.toString(16).padStart(2, '0')).join('')
  return `meridian-${hex}`
}

export function Alerts() {
  const { state } = useApp()
  const [cfg, setCfg] = useState<Config | null>(null)
  const [firing, setFiring] = useState<Firing[]>([])
  const [error, setError] = useState('')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    get<Config>('/api/alerts')
      .then(setCfg)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
    get<Firing[]>('/api/alerts/preview')
      .then(setFiring)
      .catch(() => undefined)
  }, [])

  if (!state) return <SkelScreen />
  if (error && !cfg) return <div className="banner bad">{error}</div>
  if (!cfg) return <SkelScreen />

  const set = (patch: Partial<Config>) => setCfg({ ...cfg, ...patch })

  const save = async () => {
    setBusy(true)
    setError('')
    setNote('')
    try {
      setCfg(await post<Config>('/api/alerts', cfg))
      setFiring(await get<Firing[]>('/api/alerts/preview'))
      setNote('Saved.')
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const test = async () => {
    setBusy(true)
    setError('')
    setNote('')
    try {
      await post('/api/alerts/test')
      setNote('Sent. It should be on your phone within a second or two.')
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const setLevel = (i: number, patch: Partial<Level>) =>
    set({ levels: cfg.levels.map((l, j) => (i === j ? { ...l, ...patch } : l)) })

  return (
    <div className="screen">
      <section className="panel">
        <div className="panel-head">Where alerts go</div>
        <div className="shock-inputs">
          <label>
            <input
              type="checkbox"
              checked={cfg.enabled}
              onChange={(e) => set({ enabled: e.target.checked })}
            />
            <span>Send alerts to my phone</span>
          </label>
        </div>
        <div className="shock-inputs">
          <label>
            <span className="text-muted">ntfy topic</span>
            <input
              className="input wide"
              value={cfg.topic}
              placeholder="meridian-..."
              onChange={(e) => set({ topic: e.target.value })}
            />
          </label>
          <button className="btn btn-secondary" onClick={() => set({ topic: makeTopic() })}>
            Generate
          </button>
          <label>
            <span className="text-muted">Server</span>
            <input
              className="input wide"
              value={cfg.server}
              placeholder="https://ntfy.sh"
              onChange={(e) => set({ server: e.target.value })}
            />
          </label>
        </div>
        <p className="warn-soft">
          An ntfy topic has no password. Anyone who knows or guesses the name receives everything
          sent to it, so treat the topic as the secret it is: generate one rather than picking a
          memorable word, and do not paste it anywhere public. For the same reason these
          notifications carry a ticker and a percentage and never an amount, a share count or a
          portfolio total.
        </p>
        <p className="text-muted">
          Install ntfy on your phone, subscribe to this exact topic, then press Send a test.
          Checks run on the server every 15 minutes, which is why they keep working while the app
          is closed. A desktop app that is shut cannot warn you about anything.
        </p>
      </section>

      <section className="panel">
        <div className="panel-head">What to watch</div>
        <div className="rules">
          <label>
            <input
              type="checkbox"
              checked={cfg.drift}
              onChange={(e) => set({ drift: e.target.checked })}
            />
            <span>
              A holding leaves its target band
              <span className="text-muted"> the one that maps to a decision you already made</span>
            </span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={cfg.big_move}
              onChange={(e) => set({ big_move: e.target.checked })}
            />
            <span>A holding moves more than</span>
            <input
              className="input num"
              type="number"
              min={0.5}
              step={0.5}
              value={cfg.big_move_pct}
              onChange={(e) => set({ big_move_pct: Number(e.target.value) })}
            />
            <span>% in a day</span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={cfg.portfolio_move}
              onChange={(e) => set({ portfolio_move: e.target.checked })}
            />
            <span>The whole portfolio moves more than</span>
            <input
              className="input num"
              type="number"
              min={0.5}
              step={0.5}
              value={cfg.portfolio_move_pct}
              onChange={(e) => set({ portfolio_move_pct: Number(e.target.value) })}
            />
            <span>
              % in a day
              <span className="text-muted"> a basket moves less than anything in it, so this
              wants a smaller number than the one above</span>
            </span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={cfg.stale}
              onChange={(e) => set({ stale: e.target.checked })}
            />
            <span>
              Prices go stale, or a holding loses its price
              <span className="text-muted"> the dull one that tells you the app is lying</span>
            </span>
          </label>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">Price levels</div>
        {cfg.levels.length === 0 ? (
          <p className="text-muted">None. A level fires once when the price first crosses it.</p>
        ) : null}
        {cfg.levels.map((l, i) => (
          <div className="shock-inputs" key={l.id || i}>
            <input
              className="input"
              value={l.ticker}
              placeholder="EQNR.OL"
              onChange={(e) => setLevel(i, { ticker: e.target.value })}
            />
            <select
              className="input pick"
              value={l.above ? 'above' : 'below'}
              onChange={(e) => setLevel(i, { above: e.target.value === 'above' })}
            >
              <option value="above">rises to</option>
              <option value="below">falls to</option>
            </select>
            <input
              className="input num"
              type="number"
              min={0}
              step={0.01}
              value={l.price}
              onChange={(e) => setLevel(i, { price: Number(e.target.value) })}
            />
            <button
              className="btn btn-ghost"
              onClick={() => set({ levels: cfg.levels.filter((_, j) => j !== i) })}
            >
              Remove
            </button>
          </div>
        ))}
        <button
          className="btn btn-secondary"
          onClick={() =>
            set({ levels: [...cfg.levels, { id: '', ticker: '', price: 0, above: true }] })
          }
        >
          <i className="ph ph-plus" />
          Add a level
        </button>
      </section>

      {error ? <div className="banner bad">{error}</div> : null}
      {note ? <div className="banner">{note}</div> : null}

      <div className="stat-actions">
        <button className="btn btn-primary" onClick={() => void save()} disabled={busy}>
          Save
        </button>
        <button
          className="btn btn-secondary"
          onClick={() => void test()}
          disabled={busy || !cfg.enabled || !cfg.topic}
        >
          Send a test
        </button>
      </div>

      <section className="panel">
        <div className="panel-head">Firing right now</div>
        {firing.length === 0 ? (
          <p className="text-muted">
            Nothing. This is what the server would send on its next check, so it is also the way
            to see a rule is too loose before it starts buzzing every quarter hour.
          </p>
        ) : (
          <ul className="firing">
            {firing.map((f) => (
              <li key={f.key}>{f.text}</li>
            ))}
          </ul>
        )}
      </section>
    </div>
  )
}
