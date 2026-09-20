import { useApp } from './store'

// The eleven rows of the design's sidebar. The built screens are clickable; the rest are shown
// greyed so the shape of the finished app is visible from the start.
const NAV: [string, string, string, boolean][] = [
  ['dashboard', 'Dashboard', 'ph-squares-four', true],
  ['builder', 'Builder', 'ph-sliders-horizontal', true],
  ['analytics', 'Analytics', 'ph-chart-line-up', true],
  ['sentiment', 'Sentiment', 'ph-pulse', false],
  ['bonds', 'Fixed income', 'ph-bank', false],
  ['energy', 'Energy & shipping', 'ph-boat', false],
  ['discover', 'Discover', 'ph-compass', true],
  ['optimize', 'Optimize', 'ph-target', true],
  ['rebalance', 'Rebalance', 'ph-scales', true],
  ['stress', 'Stress test', 'ph-warning-diamond', true],
  ['backtest', 'Backtest', 'ph-clock-counter-clockwise', true],
  ['alerts', 'Alerts', 'ph-bell', false],
  ['rules', 'Rules', 'ph-funnel', false],
]

export function Sidebar() {
  const screen = useApp((s) => s.screen)
  const setScreen = useApp((s) => s.setScreen)
  return (
    <aside className="side">
      <div className="brand">
        <span className="brand-mark" />
        <span className="brand-name">Meridian</span>
      </div>
      <nav>
        {NAV.map(([id, label, icon, live]) => (
          <div
            key={id}
            className={`nav-row${screen === id ? ' on' : ''}${live ? '' : ' off'}`}
            onClick={live ? () => setScreen(id) : undefined}
            title={live ? undefined : 'Not in this version yet'}
          >
            <i className={`ph ${icon}`} />
            <span>{label}</span>
          </div>
        ))}
      </nav>
    </aside>
  )
}
