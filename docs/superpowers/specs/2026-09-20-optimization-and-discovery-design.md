# Portfolio optimization and discovery

**Date:** 2026-09-20
**Status:** approved in chat, not yet built

## Goal

Two things Meridian cannot do today: suggest better weights for the holdings you
already own, and search a market for a basket of N stocks worth owning. Both
rest on one new piece of arithmetic, the covariance of daily returns in base
currency, so they share a core and differ only in the question asked of it.

## Prior art

`~/Documents/github/Lux/stonks` solves this in Python with skfolio. Its
universe file, pre-selection pipeline, rebalancing lag and cost model are all
carried over. One thing is deliberately not carried over: see "Selection never
touches the test window" below.

## Project-wide rules this work must obey

These outrank convenience everywhere in the design.

1. **A missing price is never a zero.** A symbol with no history is excluded and
   named, never valued at zero and never silently dropped from a total.
2. **Quarterly, not daily and not never.** Rebalancing happens, at roughly
   quarterly cadence: 63 trading days. Nothing may assume daily or monthly
   trading. A constant-weight backtest is a disguised daily rebalance and is
   therefore forbidden. Held-out evaluation lets weights drift with prices and
   restores them to target every 63 trading days, charging the cost of the
   turnover each time. In live use the Rebalance screen's drift bands are the
   trigger and the quarter is the rhythm, so the modelled cadence is an upper
   bound on how often an account actually trades.
3. **Selection never touches the test window.** Any search that picks a winner
   using held-out data has destroyed that data. Select on train, freeze, then
   read the test window exactly once.
4. **Costs are charged, not assumed away.** Formation costs are applied to every
   held-out figure, and the rate is visible and editable.
5. **No number without its caveat on the same screen.** Covariance measured over
   the past is not a promise about the future, and the screens say so where the
   number is, not in a footnote.

## Part 1: the returns core

New module `crates/core/src/optimize.rs`.

Input is what the Analytics and Backtest routes already build: a map of
`history::Series` per symbol plus the FX series they need. No new fetching.

- Convert each symbol's closes to base currency with the same FX handling
  `calc::allocation_history` uses.
- Restrict to the days every symbol shares, so the covariance is computed on
  aligned observations rather than on a ragged matrix.
- Daily simple returns; sample covariance with the `n-1` correction, annualised
  by 252.
- Portfolio volatility as `sqrt(w' * Sigma * w)`, annualised.
- Correlation derived from the covariance, for display and for the 0.95
  de-duplication filter.

Symbols with too few shared observations are excluded and named. The floor is
252 observations, one year: below that a covariance is noise with a decimal
point.

### Estimators

Both objectives use shrinkage, because the unshrunk versions are what make
optimisers behave absurdly:

- Covariance: shrunk toward a diagonal target (Ledoit-Wolf style constant
  correlation is the intent; a fixed shrinkage intensity is acceptable in v1 if
  the estimator is honest about being fixed).
- Mean: shrunk toward the grand mean, used only by max-Sharpe.

## Part 2: the optimizer, over holdings you own

Two objectives, toggled on the screen, both long-only and both summing to 1:

- **Inverse volatility.** Weight proportional to `1/sigma`. No solver, no
  failure modes, and no return forecast.
- **Minimum variance.** Minimise `w' * Sigma * w`. The closed form
  `Sigma^-1 * 1 / (1' * Sigma^-1 * 1)` is not usable because it produces short
  positions, which this app cannot hold. Solved instead by projected gradient
  descent onto the probability simplex: gradient `2 * Sigma * w`, step from a
  Gershgorin bound on the largest eigenvalue, Euclidean projection by the
  sorting method. Roughly forty lines and exactly testable, since on a diagonal
  covariance the answer is provably proportional to `1/sigma^2`.

Neither uses an expected return, which is the reason to trust them.

### Holdings with no history

Optimise over the holdings that have history. Holdings without it keep their
current `target_pct` untouched, and the suggested weights are scaled to fill
exactly what remains of 100, so Rebalance's requirement that targets sum to 100
still holds. The screen names the excluded holdings and states what share of the
portfolio they are.

### Write-back

`POST /api/targets` with `{portfolio, targets: [{id, pct}]}` writes `target_pct`
and nothing else. Suggested weights sit beside current ones and nothing is
written until the button is pressed. Rebalance then produces the trades.

## Part 3: discovery, over a market

### Universe

Vendored in the repo, derived from Lux's
`Euronext_Equities_2025-10-05_norway.csv` (302 Oslo equities;
`Symbol + ".OL"` is the Yahoo ticker), filtered to the main list. Euronext does
not cover Stockholm or Copenhagen, so OMXS30 and OMXC25 members are added by
hand, giving roughly 120 names across three exchanges and three currencies.
Currency is not a complication: base-currency conversion already exists.

A Euronext CSV can be dropped in to replace the vendored list, through the
parser the broker import already uses.

### Windows

Y years split into train, a five-day gap, then test. The gap exists because a
test window starting the day training ends can see the training window's last
prices; Lux's February summary measures the effect at 0.2 to 0.4 of Sharpe.

Default split: 70% train, 30% test.

### Search

On the training window only:

1. Score every candidate by Sharpe; keep the top K (default 30).
2. Drop any candidate correlated above 0.95 with one already kept, so the search
   does not spend itself on two share classes of one company.
3. Search combinations of N. Exhaustive while `C(K,N)` is at or under 200,000,
   which covers the common cases in Rust in seconds; greedy forward selection
   above that. The screen states which ran.

The winner is the best training-window objective. The test window is not read
until the basket is frozen.

### What the screen reports

Four figures, never one:

- the chosen basket in-sample,
- the same basket out-of-sample, rebalanced quarterly, after costs,
- equal weight over the same N stocks out-of-sample, same treatment,
- the market index over the test window.

When the optimiser fails to beat equal weight out of sample, the screen says so.
That is the usual outcome, and concealing it would make this the one dishonest
screen in the app.

### Costs

Default 25 bps one way, editable. Rationale: Oslo commission around 5 bps plus
half the spread on a liquid name. Thin names cost far more, which is part of why
the universe is filtered to the main list.

Charged on traded notional, as `rate * sum(|delta w|)`. Moving 5% of the
portfolio from one holding to another is a 5% sell and a 5% buy, so it is
charged on 10%, not on 5%. Formation from cash is all buys, `sum(|delta w|) = 1`,
so it costs the rate once. A quarterly rebalance that barely moves costs almost
nothing, which is the behaviour that makes the figure believable.

## Screens

Two new sidebar rows, Optimize and Discover, next to Rebalance. The design's
eleven rows become thirteen; none of the greyed rows is a home for these.

## Testing

No test touches the network, as everywhere else in this repo.

- Covariance and correlation against hand-computed small matrices.
- Minimum variance on a diagonal covariance against the closed form
  `1/sigma^2`, normalised.
- Minimum variance on a degenerate covariance: weights still non-negative and
  still summing to 1.
- Long-only enforcement: an asset the unconstrained solution would short gets
  zero, not a negative weight.
- The holdings-without-history rule: suggested plus untouched targets sum to
  exactly 100.
- Quarterly-rebalanced evaluation differs from constant-weight on a series where
  they must differ, so rule 2 cannot silently regress.
- Cost is charged on turnover: a rebalance that moves nothing costs nothing, and
  moving 5% between two holdings is charged on 10%.
- Selection uses only training observations: a test constructed so that a
  train-selected winner and a test-selected winner differ, asserting the code
  picks the former.

Each behaviour is mutation-checked: break it, watch the test fail, restore.

## Out of scope for v1

Hierarchical risk parity, CVaR objectives, maximum diversification, the
efficient frontier plot, and walk-forward with rolling re-selection. The last
one re-picks the basket every window, which is a different and more expensive
strategy than holding one basket and rebalancing it; it remains the right tool
for measuring a strategy and may return as a measurement-only screen.
