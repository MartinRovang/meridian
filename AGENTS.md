# meridian: agent notes

## The crate split is the design

`meridian-core` and `meridian-server` must never depend on `tauri`, `webkit2gtk`
or `gtk`. The server is meant to run on a box with no desktop libraries, and
CI's `server-isolation` job builds it in a bare `rust:1-slim` container with no
apt packages at all to prove it. A PR that breaks that job has broken the
architecture, not the build.

The dependency direction is one-way and does not bend:

```
meridian-core  ->  nothing
meridian-server ->  core
src-tauri      ->  server + core
```

The frontend makes no relative `/api/` request. Everything goes through
`src/api.ts`, which is handed an absolute base at boot. A relative fetch works
in local mode and fails in remote mode, which is the worst kind of bug to find
late.

## Version bumps

The version lives in `[workspace.package] version` in the root `Cargo.toml`.
Both binaries carry it.

**Bump it on every PR.** CI's `version-bumped` job fails a pull request whose
version equals the base branch's. Because `main` advances, re-check it after
syncing with `main`. Increment the patch (`0.1.0` -> `0.1.1`).

## All money math lives in Rust

`crates/core/src/calc.rs` is the only place portfolio arithmetic is allowed.
The frontend renders what `/api/state` sends and computes nothing. A percentage
calculated in TypeScript is a review comment.

The one rule that outranks the rest: **a missing price is never a zero.** A
holding with no quote, no FX rate, or no rate for its cost currency comes back
`priced: false` with its money fields zeroed, and every total excludes it and
counts it in `unpriced`. Anything that lets an unpriced holding into a sum is a
bug, however tidy the arithmetic looks.

## No test touches the network

Yahoo's endpoint is unofficial and will break one day. Its responses are
checked-in fixtures under `crates/core/tests/fixtures/`, captured from real
calls. A test that makes a real request is a test that fails for reasons
unrelated to the change.

`/api/refresh` and `/api/search` have no automated test for this reason: both
exist to reach the network. Their parsing halves are covered in core, and both
are verified by hand against live data before release.

## Importing a broker export

`crates/core/src/import.rs` parses the file and works out what it would change;
the server exposes preview, candidates and apply; the Builder shows the preview
and asks. Rules that are not negotiable:

- **An import never deletes.** One file is one account. Holdings the file omits
  are listed as information and left alone.
- **GAV is per share, `cost_basis` is the total.** Confusing them is silent.
- **A row without a ticker is never guessed into existence.** Exports name funds,
  not symbols. The user picks a listing or types one, and the answer is kept in
  `Store::aliases` so the next import of that account needs no clicks.
- **Currency and the broker's printed price choose the listing.** The same fund
  lists in several currencies; attaching a EUR holding to the London USD line
  misprices it by around 15% and never errors.
- Imported holdings start at `target_pct: 0`, so Rebalance keeps refusing until
  the user sets targets. That refusal is correct, not a bug to paper over.

The preview route takes raw bytes rather than JSON: these exports are commonly
UTF-16, and a JSON string would destroy the encoding the parser exists to cope
with.

## Price history and Analytics

`crates/core/src/history.rs` stores one JSON file per symbol under
`<store>/history/`. Adjusted close, not close: an unadjusted series reports
every dividend as a loss on the ex-date.

`calc::allocation_history` is **not** the portfolio's past performance, and the
screen says so out loud. Meridian stores no transactions, so it cannot know
what was held last year; every point values today's share count at that day's
prices. Anything that presents it as realised performance is a lie about
someone's money.

Three rules, each mutation-checked:

- A null close is a day that did not trade: dropped, never zeroed.
- Values forward-fill, because exchanges keep different holidays.
- A missing fx history drops the holding and names it. A rate of 1.0 would
  value a euro as a krone.

The statistics live in `src/stats.ts` with their own tests, because they are
money figures computed in TypeScript and the screen around them is not
testable without a browser.

## The Stress screen

Two panels, both arithmetic over numbers the app already has, and neither is a
risk model.

*What if* multiplies: a uniform fall applied to every holding, and a fall in
every currency other than the base one applied to what is held abroad. The two
compound rather than adding up. There are no betas: estimating each holding's
sensitivity to "the market" from five years of prices gives a number to two
decimals that is mostly noise, and it would obscure the honest part of the
screen, which is that the user chose the shock.

*Worst it has been* reports the worst move across any window of 1, 5, 21 and 63
trading days in the same series Analytics draws, with the days it ran between.
It carries Analytics' caveat, because it is the same series: today's holdings
priced back through time, not a record of what the portfolio held.

`src/stress.ts` computes money in TypeScript, which "All money math lives in
Rust" otherwise forbids. The exception is the same one `src/stats.ts` takes: a
what-if the user retypes has no business making a request per keystroke. It
buys the exception the same way, with its own tests, including one proving an
unpriced holding is named rather than shocked as though it were worth nothing.

## The Backtest screen

The same series Analytics draws, against a benchmark over the same days.

A benchmark is one share of one symbol, so it goes through
`calc::allocation_history` exactly as the portfolio does: the same forward
fill, the same conversion into base currency, the same refusal to value what it
has no rate for. `GET /api/history?portfolio=X&benchmark=SYM` returns both, and
without `benchmark` the response is unchanged, which is what Analytics reads.

`calc::align` cuts the two to the days they share and rebases both to 100 on
the first of them. A benchmark whose history starts later would otherwise be
drawn from its own first day, and the two lines would compare spans that do not
overlap.

The screen says it charges no fees, spreads or tax, because it does not: a real
account that merely keeps pace with an index has in fact fallen behind it.

## Optimizing weights

`crates/core/src/optimize.rs` holds the covariance of daily returns in base
currency and the two long-only weightings that follow from it: inverse
volatility, and minimum variance solved by projected gradient descent onto the
simplex. The closed form is not used, because it wants short positions this app
cannot hold.

Neither objective uses an expected return. That is not an omission. Covariance
is estimated far more reliably than mean return, and an optimiser fed five years
of measured returns concentrates the portfolio into whatever happened to go up.
The screen says so where the numbers are.

Two rules the arithmetic must keep:

- **Targets sum to 100.** Holdings with under a year of shared history are
  excluded and named, their current target is carried over untouched, and the
  optimised weights are scaled to fill exactly what remains. Rebalance refuses
  targets that do not sum to 100, so a suggestion that breaks this is a
  suggestion the user cannot apply.
- **Volatility before and after is measured over the same subset**, with current
  targets renormalised within it. Comparing a subset summing to 70 against one
  summing to 100 reports a difference in size as a difference in risk.

### Diagnostics

`risk_contributions` gives each holding's share of the portfolio's risk, which is
not its share of the money: on the demo portfolio Equinor is 40% of the value
and 49.4% of the risk, and no weight column will ever say so.
`diversification_ratio` is the weighted average of the parts' volatilities over
the whole's, so one means holding them together bought nothing.

One result looks like a bug and is not: **at a minimum-variance optimum every
holding's share of risk equals its share of the money exactly.** Equalising
marginal risk is the first-order condition of that objective, so the identity
holds by construction. Both screens say so where the columns agree, and there is
a test asserting it, which doubles as an independent check on the solver: a
weighting that misses the optimum breaks the identity.

`POST /api/targets` writes `target_pct` and nothing else, all holdings or none.
Applying an optimizer's proposal must not be able to touch a share count or a
cost basis, whatever the payload says.

## Discovering a basket

`crates/core/src/discover.rs` searches a market for N stocks. The arithmetic is
the easy part; the selection is where this screen could become the one dishonest
thing in the app, so three rules are load-bearing.

**Selection never reads the test window.** The window splits seven tenths to
training, a five-day gap, then the rest. The pre-filter, the weights and the
winner all see only the training rows. The test rows are read once, after the
basket is frozen. Lux's `find_best_n_stocks` picks the combination with the
highest test-window Sharpe, which spends the holdout on the search: the reported
out-of-sample figure is then the maximum of a hundred thousand draws, and it is
a better explanation of its 3.84 Sharpe than the look-ahead and cost issues its
own summary fixes. Do not reintroduce that.

**Equal weight is always reported.** The same N names, equally weighted, over
the same held-out window, after the same costs. When the optimiser loses to it
the screen says so in a callout above the numbers. On Oslo with five stocks it
does lose: 26.6% against 34.9%. Maximum Sharpe triples the in-sample figure to
214% and still loses out of sample, which is the clearest demonstration of
overfitting the app can offer.

**Quarterly, drifting, after costs.** Weights drift between rebalances and are
restored every 63 trading days, with costs charged on `rate * sum(|delta w|)`.
Holding weights constant would be a daily rebalance in disguise and would
collect a premium nobody can capture.

The search is exhaustive while `C(K, N)` is at or under 200,000 and greedy above
it, and the screen states which ran. Two performance rules keep the exhaustive
case usable: the covariance over a subset is a submatrix of the covariance over
all candidates, so it is computed once and sliced, and the solver's iteration
cap is sized for the handful of assets in a basket. Rebuilding the covariance
per combination took 124 seconds for 142,506 baskets; slicing it takes 8 in a
debug build and about 2 in a release one, which is what ships.

The result must not depend on which thread finished first, so `par::best_of`
breaks ties by the lower index. Two identical requests return the same basket to
the last decimal, which is the difference between a search you can argue with
and one you cannot.

### The forecast

Discover reports an expected return over a horizon you choose, and it is the
weakest number in the app. Three things keep it honest, and none of them is
optional.

**The interval carries both ignorances.** `sd = vol * sqrt(h + h^2 / T)`: the
first term is how far returns scatter over the horizon, the second is how badly
the mean itself is pinned down by a training window of `T` years. A band built
from volatility alone is too narrow and, worse, shrinks toward nothing as the
horizon shrinks, which is false. On the Nordic field over a year the interval
runs from -5.8% to +48.8% around a central +21.5%: it spans zero, and that is
the finding.

**A calibration the screen cannot fudge.** The same method is pointed at the
held-out window using only training data, and the prediction is shown beside
what actually happened. Most recently +31.0% predicted against +27.5% realised.

**Predictiveness is measured, not asserted.** The correlation across every
usable listing between its training-window return and its held-out return.
Across 112 Nordic listings it came out at 0.29. Measured over one split, so the
screen calls it luck until it repeats. It is computed over all usable listings
rather than the picked ones, because the picked ones were chosen for their past
and would answer the question with their own selection.

`All Nordic` searches all three exchanges at once, which is where the search has
most to work with: 112 usable listings, a diversification ratio of 1.76 against
Oslo's own, and correlations among the picks between 0.12 and 0.36. Three
currencies is not a complication, because everything is converted to base
currency before a single return is computed.

`crates/core/src/universe.rs` is the shipped list: Oslo's main board by
turnover from a Euronext export, plus Stockholm and Copenhagen large caps by
hand, since Euronext carries neither. A stale entry is visible rather than
silent: a delisted symbol has no usable history and is named on the screen.

## Alerts

`crates/core/src/alerts.rs` decides, `meridian-server` schedules, ntfy delivers.

**The loop lives in the server, not the app.** A desktop app is shut at exactly
the moment an alert matters. `serve` spawns a thread that every 15 minutes
refreshes prices and then evaluates, in that order: rules read the quote cache,
and a rule evaluated against yesterday's prices fires on yesterday's news.

**A rule notifies once.** `newly_firing` compares what is true now against
`alert-state.json` and sends only the difference, so a drift breach that lasts a
fortnight buzzes on the day it starts. A rule that stops firing is forgotten, so
the next breach is news again. That file is separate from `portfolios.json`
because it changes every quarter hour and the store is rewritten whole under a
lock.

**An ntfy topic is a password printed on every message.** There is no
authentication on a topic: anyone who knows the name receives everything. So the
screen generates a random topic rather than letting one be typed, the server
refuses a topic containing `/ ? #` or a space, it refuses a plain-http server,
and above all **no notification carries an amount, a share count or a portfolio
total.** A ticker and a percentage, and the app is one tap away for the rest.
There is a test asserting it, and it should stay that way.

**The portfolio has its own rule and its own threshold.** `portfolio_move`
reads `PortfolioView.day_pct` and fires at `portfolio_move_pct`, defaulting to
2% rather than the holding rule's 5%: a basket of a dozen names rarely moves as
far as any one of them, so sharing one number would mean the portfolio rule
never fired. Its message names the portfolio and the percentage, and nothing
else, for the same reason every other message does.

`evaluate` is a pure function over the views and drift rows the Dashboard
already computes, so every rule is testable without a network or a clock. Only
`notify` touches the wire.

## Indicators on the charts

`src/stats.ts` computes them, `Line` draws them, `useIndicators` holds the three
switches so Analytics and Backtest cannot drift apart. Nothing reaches the
server: the indexed series is already on the client.

`bollinger` uses the **population** deviation over its own window, while
`volatility` above it uses the **sample** one. That is deliberate. A Bollinger
window is the whole of what is being averaged; a run of daily returns is a
sample of a process. Both match their own convention, and neither should be
changed to match the other.

The warm-up days have no reading, so `bollinger` and `rsi` return arrays as long
as the series with nulls at the front. The chart starts the envelope later
rather than drawing it flat at the first value it can compute, which would
invent a band where there is no window to compute one from.

RSI gets its own picture under the chart rather than a second axis on it. Two
scales in one frame is the chart mistake that makes a flat line look like a
rally.

## Caching quotes

The cache is a `HashMap<String, Quote>` behind a mutex, mirrored to
`quotes.json` so it survives a restart. Three rules keep it cheap:

- `quotes::needs_fetch` decides what to ask for: anything missing, and anything
  older than `REFRESH_TTL` (60s). `POST /api/refresh?force=1` passes a ttl of 0,
  which is what the Refresh button sends.
- `quotes::fetch_many` runs the requests `MAX_PARALLEL` (8) at a time on scoped
  threads.
- `Ctx::refreshing` is held for the length of a refresh, so a second caller
  waits and then finds everything fresh instead of repeating the work.

Redis was considered and rejected: the working set is one quote per held symbol,
a few hundred bytes, already shared across requests and already durable. A cache
server would add a daemon, a network hop and a fallback path to guard that. It
becomes the right answer only if `meridian-server` ever runs as more than one
process, and that decision is not made here.

## Checks before pushing

```sh
pnpm lint && pnpm test && pnpm build
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo tree -p meridian-server | grep -Ei 'tauri|webkit|gtk'   # must print nothing
```
