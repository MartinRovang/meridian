import { useEffect } from 'react'
import { apiBase } from './api'
import { Sidebar } from './Sidebar'
import { Splash } from './Splash'
import { useApp } from './store'

const TITLES: Record<string, [string, string]> = {
  dashboard: ['Overview', 'Portfolio overview'],
  builder: ['Construct', 'Portfolio builder'],
  rebalance: ['Act', 'Rebalance proposal'],
}

export function App() {
  const { ready, steps, error, screen, state, boot } = useApp()

  useEffect(() => {
    void boot()
  }, [boot])

  if (!ready) return <Splash steps={steps} error={error} />

  const [kicker, title] = TITLES[screen] ?? TITLES.dashboard
  return (
    <div className="app">
      <Sidebar />
      <main className="main">
        <header className="page-head">
          <div>
            <div className="page-kicker">{kicker}</div>
            <h2>{title}</h2>
          </div>
        </header>
        {error ? (
          <div className="banner bad">
            {error} (API at {apiBase() || 'no address'})
          </div>
        ) : null}
        {state?.stale ? (
          <div className="banner">
            Prices are stale. The last refresh was {Math.round((state.quotes_age_secs ?? 0) / 60)}{' '}
            minutes ago.
          </div>
        ) : null}
        {/* Screens land in Tasks 12 to 14. */}
        <p className="text-muted">{title} is not built yet.</p>
      </main>
    </div>
  )
}
