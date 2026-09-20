import { useEffect, useRef, useState } from 'react'
import { get } from './api'

export type Hit = { symbol: string; name: string; exchange: string; currency: string }

const DEBOUNCE_MS = 250

export function TickerSearch({ onPick }: { onPick: (hit: Hit) => void }) {
  const [q, setQ] = useState('')
  const [hits, setHits] = useState<Hit[]>([])
  const [busy, setBusy] = useState(false)
  // Every keystroke starts a request; without this the fourth letter's answer can land after the
  // third letter's and repaint the list with stale rows.
  const seq = useRef(0)

  useEffect(() => {
    if (q.trim().length < 2) {
      setHits([])
      return
    }
    const mine = ++seq.current
    setBusy(true)
    const t = setTimeout(() => {
      get<{ hits: Hit[] }>(`/api/search?q=${encodeURIComponent(q)}`)
        .then((r) => {
          if (mine === seq.current) setHits(r.hits)
        })
        .catch(() => {
          if (mine === seq.current) setHits([])
        })
        .finally(() => {
          if (mine === seq.current) setBusy(false)
        })
    }, DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [q])

  return (
    <div className="search">
      <input
        className="input"
        placeholder="Search a ticker or company"
        value={q}
        onChange={(e) => setQ(e.target.value)}
      />
      {q.trim().length >= 2 ? (
        <div className="hits">
          {hits.map((h) => (
            <div
              key={h.symbol}
              className="hit"
              onClick={() => {
                onPick(h)
                setQ('')
                setHits([])
              }}
            >
              <span className="hit-sym">{h.symbol}</span>
              <span className="hit-name">{h.name}</span>
              <span className="text-muted">{h.exchange}</span>
            </div>
          ))}
          {!hits.length ? (
            <div className="hit text-muted">
              {busy ? 'Searching' : 'Nothing in this market scope'}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
