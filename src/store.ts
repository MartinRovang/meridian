import { create } from 'zustand'
import { configure, get, post } from './api'

export type Holding = {
  id: string; ticker: string; name: string; cls: string; shares: number
  price: number; currency: string; value: number; day_pct: number
  pl: number; pl_pct: number; weight_pct: number; target_pct: number
  cost_basis: number; cost_currency: string; priced: boolean
}
export type ClassSlice = { cls: string; value: number; pct: number }
export type Portfolio = {
  id: string; name: string; owner: string; band_pct: number
  value: number; day_pct: number; pl: number
  holdings: Holding[]; unpriced: number; by_class: ClassSlice[]
}
export type DriftRow = {
  id: string; ticker: string; name: string
  target_pct: number; actual_pct: number; drift_pct: number; breached: boolean
}
export type State = {
  version: string; base_currency: string; scope: string
  portfolios: Portfolio[]
  drift: { id: string; rows: DriftRow[] }[]
  quotes_age_secs: number | null; stale: boolean
}

export type Step = { label: string; done: boolean }

type App = {
  ready: boolean
  state: State | null
  error: string
  steps: Step[]
  screen: string
  pid: string
  boot: () => Promise<void>
  load: () => Promise<void>
  setScreen: (s: string) => void
  setPid: (id: string) => void
}

export const useApp = create<App>((set, getState) => ({
  ready: false,
  state: null,
  error: '',
  screen: location.hash.slice(1) || 'dashboard',
  pid: '',
  steps: [
    { label: 'Connecting', done: false },
    { label: 'Reading portfolios', done: false },
    { label: 'Fetching prices', done: false },
  ],
  boot: async () => {
    // The app hands us its API base and token in the launch URL. Local and remote look identical
    // from here, which is what makes the two modes one code path.
    const q = new URLSearchParams(location.search)
    configure(q.get('api') ?? '', q.get('token') ?? '')
    const tick = (i: number) =>
      set((s) => ({ steps: s.steps.map((st, n) => (n === i ? { ...st, done: true } : st)) }))
    try {
      await get('/api/health')
      tick(0)
      await getState().load()
      tick(1)
      // ponytail: the splash exists because THIS call is slow, one network round trip per held
      // symbol plus its fx pair. A failure here is not fatal: cached prices still render, and the
      // dashboard's stale banner explains itself.
      await post('/api/refresh').catch(() => undefined)
      await getState().load()
      tick(2)
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) })
      return
    }
    // Let the last tick be seen before the handover, rather than flashing past it.
    await new Promise((r) => setTimeout(r, 400))
    set({ ready: true })
  },
  load: async () => {
    try {
      set({ state: await get<State>('/api/state'), error: '' })
      const s = getState()
      if (!s.pid && s.state?.portfolios.length) set({ pid: s.state.portfolios[0].id })
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) })
    }
  },
  setScreen: (screen) => {
    location.hash = screen
    set({ screen })
  },
  setPid: (pid) => set({ pid }),
}))
