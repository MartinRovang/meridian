import { apiBase } from './api'
import type { Step } from './store'
// 2x the 148px it renders at. assets/logo.png is the 1254px original, kept for
// `cargo tauri icon`; shipping it here would put 2.3 MB in the bundle for a thumbnail.
import logo from '../assets/logo-splash.png'

// Ported from gitdashy's src-tauri/ui/splashscreen.html: the artwork, the wordmark, a striped
// progress fill with a percentage, and one step line that shimmers while it runs. Unlike
// gitdashy's, this is a component in the app bundle, so the Nocturne tokens have already loaded
// and the palette comes from them rather than being hardcoded.
// A connection error already names the base; anything else (a bad token, a 500) does not, and
// without the address you cannot tell which server refused you.
function withAddress(error: string): string {
  const at = apiBase() || 'no API address'
  return error.includes(at) ? error : `${error} (from ${at})`
}

export function Splash({ steps, error }: { steps: Step[]; error: string }) {
  const done = steps.filter((s) => s.done).length
  const pct = Math.round((done / steps.length) * 100)
  const current = steps.find((s) => !s.done)
  const finished = !current
  return (
    <div className={`splash${error ? ' failed' : ''}`}>
      <div className="splash-tooth" />
      <div className="splash-grain" />
      <div className="splash-vignette" />
      <div className="splash-card">
        <div className="splash-mark">
          <img src={logo} alt="" width={148} height={148} />
        </div>
        <h1>Meridian</h1>
        <div className="splash-rule" />
        <div className="splash-bar">
          <div className="splash-track">
            <div className="splash-fill" style={{ width: `${pct}%` }} />
          </div>
          <span className="splash-pct">{String(pct).padStart(3, '0')}</span>
        </div>
        <p className={`splash-steps${finished ? ' ok' : ''}`}>{current?.label ?? 'Ready'}</p>
        <p className="splash-err">{error ? withAddress(error) : ''}</p>
      </div>
    </div>
  )
}
