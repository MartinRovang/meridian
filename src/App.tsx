import { useEffect } from 'react'
import { apiBase, post } from './api'
import { Sidebar } from './Sidebar'
import { Splash } from './Splash'
import { Analytics } from './screens/Analytics'
import { Builder } from './screens/Builder'
import { Dashboard } from './screens/Dashboard'
import { Rebalance } from './screens/Rebalance'
import { useApp } from './store'

const TITLES: Record<string, [string, string]> = {
  dashboard: ['Overview', 'Portfolio overview'],
  analytics: ['Examine', 'Analytics'],
  builder: ['Construct', 'Portfolio builder'],
  rebalance: ['Act', 'Rebalance proposal'],
}

export function App() {
  const { ready, steps, error, screen, state, boot, load } = useApp()

  const retry = async () => {
    await post('/api/refresh?force=1').catch(() => undefined)
    await load()
  }

  useEffect(() => {
    void boot()
  }, [boot])

  useEffect(() => {
    // The screen lives in the hash, so the window's own back and forward must move it. Without
    // this the address changes and the UI does not.
    const onHash = () => useApp.setState({ screen: location.hash.slice(1) || 'dashboard' })
    addEventListener('hashchange', onHash)
    return () => removeEventListener('hashchange', onHash)
  }, [])

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
            Prices are {Math.round((state.quotes_age_secs ?? 0) / 60)} minutes old.
            <button className="btn btn-ghost" onClick={() => void retry()}>
              Retry
            </button>
          </div>
        ) : null}
        {screen === 'dashboard' ? (
          <Dashboard />
        ) : screen === 'analytics' ? (
          <Analytics />
        ) : screen === 'builder' ? (
          <Builder />
        ) : screen === 'rebalance' ? (
          <Rebalance />
        ) : (
          <p className="text-muted">{title} is not built yet.</p>
        )}
      </main>
    </div>
  )
}
