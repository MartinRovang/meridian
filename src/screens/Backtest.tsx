import { useEffect, useState } from 'react'
import { get } from '../api'
import { pct, signed } from '../format'
import { SkelScreen } from '../Skeleton'
import { Stat } from '../Stat'
import { TickerSearch } from '../TickerSearch'
import { useApp } from '../store'
import { annualised, drawdown, rebase, volatility } from '../stats'
import { useIndicators, useRange } from '../Indicators'
import { Line, Rsi, type Point } from './Line'

type Bench = { symbol: string; points: Point[] }
type History = { points: Point[]; missing: string[]; benchmark?: Bench }

// One click for the usual questions: the home index, and the one everything is measured against.
const PRESETS: [string, string][] = [
  ['OSEBX.OL', 'Oslo Børs'],
  ['^GSPC', 'S&P 500'],
  ['URTH', 'MSCI World'],
]

const last = (ps: Point[]) => (ps.length ? ps[ps.length - 1].index - 100 : 0)

export function Backtest() {
  const { state, pid } = useApp()
  const [bench, setBench] = useState('OSEBX.OL')
  const [hist, setHist] = useState<History | null>(null)
  const [error, setError] = useState('')

  const p = state?.portfolios.find((x) => x.id === pid) ?? state?.portfolios[0]
  const id = p?.id

  useEffect(() => {
    if (!id) return
    setHist(null)
    setError('')
    get<History>(
      `/api/history?portfolio=${encodeURIComponent(id)}&benchmark=${encodeURIComponent(bench)}`,
    )
      .then(setHist)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [id, bench])

  if (!state) return <SkelScreen />
  if (!p) return <p className="text-muted">No portfolios yet.</p>

  return (
    <div className="screen">
      <section className="panel">
        <div className="panel-head">Measured against</div>
        <div className="shock-inputs">
          {PRESETS.map(([sym, label]) => (
            <button
              key={sym}
              className={`btn ${bench === sym ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setBench(sym)}
            >
              {label}
            </button>
          ))}
          <span className="text-muted">{bench}</span>
        </div>
        <TickerSearch onPick={(h) => setBench(h.symbol)} />
      </section>

      {error ? <div className="banner bad">{error}</div> : null}
      {!hist && !error ? <SkelScreen /> : null}
      {hist ? <Result hist={hist} /> : null}
    </div>
  )
}

function Result({ hist }: { hist: History }) {
  const { missing, benchmark } = hist
  const [shown, switches] = useIndicators()
  const [days, rangeUi] = useRange()
  // Both lines are cut and rebased together, or the comparison would start them on different
  // days and give one of them a head start.
  const cut = (ps: Point[]) => rebase(days ? ps.slice(-days) : ps)
  const points = cut(hist.points)
  const bench = benchmark ? { ...benchmark, points: cut(benchmark.points) } : undefined
  if (points.length < 2) {
    return (
      <p className="text-muted">
        Nothing to compare over.{' '}
        {benchmark?.points.length === 0
          ? `Nothing is known about ${benchmark.symbol}, or its history does not overlap this portfolio's.`
          : ''}
        {missing.length ? ` Nothing is known about ${missing.join(', ')}.` : ''}
      </p>
    )
  }

  const mine = last(points)
  const theirs = bench ? last(bench.points) : 0
  const ann = annualised(points)
  const annB = bench ? annualised(bench.points) : null

  return (
    <>
      <p className="warn-soft">
        Today&apos;s holdings priced back through time, against the benchmark over the same days.
        It is not a record of what this portfolio did, and it charges no fees, spreads or tax: a
        real account keeping pace with an index has in fact fallen behind it.
      </p>

      <section className="panel">
        <div className="panel-head">
          Indexed to 100 on {points[0].day}
        </div>
        {rangeUi}
        {switches}
        <Line
          points={points}
          compare={bench?.points}
          bands={shown.bands}
          ma={shown.ma}
          span={shown.span}
          k={shown.k}
        />
        {shown.rsi ? <Rsi points={points} span={shown.rsiSpan} /> : null}
        <div className="legend-line">
          <span className={mine >= 0 ? 'up' : 'down'}>
            <i /> This portfolio
          </span>
          <span className="text-muted">
            <i /> {benchmark?.symbol}
          </span>
        </div>
      </section>

      <div className="stat-row">
        <Stat label="Portfolio" value={`${signed(mine)}%`} />
        <Stat label={benchmark?.symbol ?? 'Benchmark'} value={`${signed(theirs)}%`} />
        <Stat
          label="Difference"
          value={`${signed(mine - theirs)}pp`}
          tone={mine - theirs < 0 ? 'down' : 'up'}
        />
        <Stat
          label="Annualised, both"
          value={
            ann === null || annB === null
              ? 'needs a year'
              : `${signed(ann)}% vs ${signed(annB)}%`
          }
        />
      </div>

      <div className="stat-row">
        <Stat label="Volatility" value={pct(volatility(points))} />
        <Stat label="Volatility, benchmark" value={bench ? pct(volatility(bench.points)) : '--'} />
        <Stat label="Worst drawdown" value={`-${pct(drawdown(points))}`} />
        <Stat
          label="Worst drawdown, benchmark"
          value={bench ? `-${pct(drawdown(bench.points))}` : '--'}
        />
      </div>

      {missing.length ? (
        <p className="text-muted">
          Left out of the portfolio line, because nothing is known about their past:{' '}
          {missing.join(', ')}.
        </p>
      ) : null}
    </>
  )
}
