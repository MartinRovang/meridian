import { useEffect } from 'react'
import { apiBase, post } from './api'
import { Sidebar } from './Sidebar'
import { Splash } from './Splash'
import { Builder } from './screens/Builder'
import { Dashboard } from './screens/Dashboard'
import { Rebalance } from './screens/Rebalance'
import { Stress } from './screens/Stress'
import { Backtest } from './screens/Backtest'
import { Optimize } from './screens/Optimize'
import { Discover } from './screens/Discover'
import { Alerts } from './screens/Alerts'
import { Rules } from './screens/Rules'
import { Energy } from './screens/Energy'
import { useApp } from './store'
import { ConfirmHost } from './Confirm'

const TITLES: Record<string, [string, string]> = {
  dashboard: ['Overview', 'Portfolio overview'],
  builder: ['Construct', 'Portfolio builder'],
  rebalance: ['Act', 'Rebalance proposal'],
  stress: ['Examine', 'Stress test'],
  backtest: ['Examine', 'Backtest'],
  optimize: ['Act', 'Optimize weights'],
  discover: ['Examine', 'Discover'],
  alerts: ['Act', 'Alerts'],
  rules: ['Act', 'Rules'],
  energy: ['Examine', 'Energy & shipping'],
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
        {/* The chart moved onto the Dashboard, so an old #analytics link lands there rather
            than on a screen that no longer exists. */}
        {screen === 'dashboard' || screen === 'analytics' ? (
          <Dashboard />
        ) : screen === 'builder' ? (
          <Builder />
        ) : screen === 'rebalance' ? (
          <Rebalance />
        ) : screen === 'stress' ? (
          <Stress />
        ) : screen === 'backtest' ? (
          <Backtest />
        ) : screen === 'optimize' ? (
          <Optimize />
        ) : screen === 'discover' ? (
          <Discover />
        ) : screen === 'alerts' ? (
          <Alerts />
        ) : screen === 'rules' ? (
          <Rules />
        ) : screen === 'energy' ? (
          <Energy />
        ) : (
          <p className="text-muted">{title} is not built yet.</p>
        )}
      </main>
      <ConfirmHost />
    </div>
  )
}
