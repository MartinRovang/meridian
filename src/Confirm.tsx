import { create } from 'zustand'

// window.confirm in the packaged webview is a native GTK dialog titled with the page's address,
// which reads as a stray link. This is the same question asked inside the app.

type Ask = { title: string; body: string; action: string; done: (yes: boolean) => void }

const useAsk = create<{ ask: Ask | null }>(() => ({ ask: null }))

/** Resolves true when the user confirms, false on Cancel, Escape or a click outside. */
export function ask(title: string, body: string, action = 'Continue'): Promise<boolean> {
  return new Promise((done) => useAsk.setState({ ask: { title, body, action, done } }))
}

export function ConfirmHost() {
  const a = useAsk((s) => s.ask)
  if (!a) return null
  const close = (yes: boolean) => {
    useAsk.setState({ ask: null })
    a.done(yes)
  }
  return (
    <div
      className="dialog-backdrop"
      onClick={(e) => e.target === e.currentTarget && close(false)}
      onKeyDown={(e) => e.key === 'Escape' && close(false)}
    >
      <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="ask-title">
        <div className="dialog-title" id="ask-title">
          {a.title}
        </div>
        <div className="dialog-body">{a.body}</div>
        <div className="dialog-actions">
          <button className="btn btn-secondary" onClick={() => close(false)}>
            Cancel
          </button>
          <button className="btn btn-primary" autoFocus onClick={() => close(true)}>
            {a.action}
          </button>
        </div>
      </div>
    </div>
  )
}
