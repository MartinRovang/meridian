# Meridian: portfolio management and surveillance

Design spec, 2026-09-20. Status: approved, pre-implementation.

## 1. What this is

A desktop app for managing and watching a personal investment portfolio, built
as a single self-contained binary. Positions are entered by hand; prices come
from Yahoo Finance; all portfolio math runs in Rust and the frontend only
renders it.

Focus market is Norway and Scandinavia, with a scope setting that widens to
Europe or global.

The visual design is the "Meridian Portfolio" Claude Design project
(`6728ffec-673a-47eb-a46b-76e0eccf62c7`) on the Nocturne design system: a dense,
dark, low-chroma interface with a single blurple accent, Inter, 8px radii and
Phosphor icons.

## 2. Scope

### In scope for v1

Three of the design's eleven screens:

- **Dashboard**: totals, allocation, holdings table, drift cards.
- **Builder**: portfolio and holding CRUD, target weights, drift band.
- **Rebalance**: drift per holding and the trade list that corrects it.

Plus the supporting machinery: manual position entry, Yahoo quotes with an
on-disk cache, multi-currency with FX conversion to a base currency, market
scope, and the self-update and release pipeline.

### Deliberately out of scope for v1

The remaining eight screens stay visible in the sidebar, greyed and
non-clickable, so the shape of the product is legible without fake data:
Analytics, Sentiment, Fixed income, Energy & shipping, Stress test, Backtest,
Alerts, Rules.

Also out: broker API integration, CSV import, Nordnet, Oslo Børs API, Norwegian
tax reporting (skattemelding, skjermingsfradrag), and any background daemon.
Each is its own sub-project with its own spec.

CSV import is the expected second sub-project. It writes to the same store the
manual path writes to, so it is additive rather than a rework.

## 3. Repository and stack

New public GitHub repo `MartinRovang/meridian`, local checkout
`~/Desktop/meridian`. Cargo package, binary, and product name are all
`meridian`. Bundle identifier `com.martinrovang.meridian`.

The stack is deliberately identical to `github-dashy`, because it is known to
work and the release machinery is being ported from it:

- Rust, Tauri 2, `tiny_http`, `ureq`, `serde`, `clap`, `include_dir`, `fs4`.
- Vite 8, React 19, TypeScript, oxlint, vitest, pnpm.
- Added beyond gitdashy: Recharts (charts) and Zustand (state). No router;
  screen selection is a URL hash.
- Phosphor icons are installed as an npm package and bundled, not CDN-linked,
  because the Tauri CSP blocks external scripts.

Nocturne's `styles.css` and `nocturne.css` are vendored verbatim into `src/`.
They are a token sheet plus a component layer (`.btn`, `.card`, `.table`,
`.field`, `.dialog`) and need no framework. The `<x-dc>` / `sc-for` markup in
the design file is a preview runtime and is not shipped; screens are translated
to React by hand.

Charts in the design are hand-built SVG and CSS bars. Those are ported as
markup. Recharts is for the time series that arrive with Analytics; if a v1
chart is less code hand-rolled, it stays hand-rolled.

### Bootstrapping from gitdashy

A fresh Tauri scaffold, with exactly four things ported:

1. `.github/workflows/ci.yml`, with names swapped.
2. `install.sh`, with names swapped.
3. `src-tauri/src/update.rs`, with `REPO` pointed at `MartinRovang/meridian`.
4. The static-serving and token-guard skeleton from `web.rs`
   (`include_dir!`, `serve`, `handle`, `guard`, `same_token`, `new_token`),
   roughly 150 lines out of that file's 4,913.

Everything else is written new. gitdashy's remaining 34,000 lines are
GitHub, PR and LLM logic with no bearing here, and its config schema is wrong
for this app.

## 4. Architecture

```
meridian (one binary)
├─ Rust
│  ├─ tiny_http bound to 127.0.0.1 on a random port
│  ├─ serves the Vite bundle embedded by include_dir!("../dist")
│  └─ /api/* JSON, guarded by a session token
└─ Tauri window pointed at that local URL
```

`meridian --no-open --port 7777` runs the server headless. The Vite dev server
on :1420 proxies `/api` to it, so UI work hot-reloads without rebuilding Rust.
This mirrors gitdashy's dev loop exactly.

### Modules

| Module | Responsibility | Rough size |
|---|---|---|
| `store.rs` | read and write `~/.meridian/portfolios.json`; atomic replace under an `fs4` lock | 150 |
| `quotes.rs` | Yahoo quote, search and FX fetch; on-disk cache; staleness | 250 |
| `calc.rs` | value, P/L, weights, FX conversion, drift, rebalance trades | 200 |
| `web.rs` | API routes, static serving, token guard | 400 |
| `config.rs` | paths, port, base currency, market scope | 80 |
| `update.rs` | ported from gitdashy | 469 |
| `main.rs`, `lib.rs` | clap parsing and Tauri boot | 50 |

**All portfolio arithmetic lives in `calc.rs`.** The frontend fetches one
`/api/state` blob and renders it. There is no money math in TypeScript. This
gives one place to test the money path and one place for it to be wrong.

### API

| Route | Purpose |
|---|---|
| `GET /api/state` | the whole computed view: portfolios, holdings, derived figures, quote freshness, warnings |
| `POST /api/portfolio` | create |
| `PATCH /api/portfolio/:id` | rename, set owner, set drift band, delete |
| `POST /api/holding` | create or update a holding |
| `DELETE /api/holding/:id` | remove |
| `POST /api/refresh` | force a quote refresh |
| `GET /api/search?q=` | ticker search, filtered by market scope |

## 5. Data model

### `~/.meridian/portfolios.json`

The whole persisted state, one file.

```json
{
  "version": 1,
  "base_currency": "NOK",
  "portfolios": [
    {
      "id": "p_a1b2",
      "name": "Balanced Growth",
      "owner": "Personal, taxable",
      "band_pct": 3.0,
      "holdings": [
        {
          "id": "h_9f3c",
          "ticker": "EQNR.OL",
          "name": "Equinor",
          "cls": "Equity, energy",
          "shares": 142.5,
          "cost_basis": 38210.0,
          "cost_currency": "NOK",
          "target_pct": 12.0
        }
      ]
    }
  ]
}
```

`cost_basis` is the total paid for the position, in the currency it was paid
in, not a per-share figure. Averaging down is then a matter of adding to two
numbers rather than recomputing an average.

### `~/.meridian/quotes.json`

A pure cache. Deleting it costs one refresh and nothing else.

```json
{
  "EQNR.OL": { "price": 271.4, "prev_close": 269.1, "currency": "NOK", "ts": 1758000000 },
  "USDNOK=X": { "price": 10.62, "prev_close": 10.59, "currency": "NOK", "ts": 1758000000 }
}
```

FX pairs live in the same map as ordinary tickers, because they are ordinary
Yahoo tickers.

### Derived, never stored

Value, P/L, weight, drift, and trades are computed on every `/api/state` from
shares plus the quote cache. Nothing derived can go stale on disk and nothing
derived needs migrating.

## 6. Market scope

A config setting, not an abstraction layer. One of:

| Scope | Yahoo exchange suffixes | Default base currency |
|---|---|---|
| `norway` | `.OL` | NOK |
| `scandinavia` (default) | `.OL`, `.ST`, `.CO` | NOK |
| `nordics` | `.OL`, `.ST`, `.CO`, `.HE`, `.IC` | NOK |
| `europe` | the above plus `.DE`, `.PA`, `.AS`, `.L`, `.MI` | EUR |
| `global` | no filter | USD |

Implementation is a `&[&str]` per scope and one match arm.

What it buys:

**Ticker search.** You type "Equinor", not "EQNR.OL". Yahoo's
`/v1/finance/search?q=` returns symbol, name, exchange and currency; the scope
filters those results to the relevant exchanges. Unfiltered, a search for a
Nordic name returns mostly US ADRs and OTC listings, which makes the feature
close to unusable. This is the main reason scope exists.

**Currency.** Scandinavian scope means NOK, SEK and DKK positions in one
portfolio as a matter of course, so FX conversion is a core path, not an
edge case.

**Day change.** Oslo Børs closes 16:20 CET and Stockholm 17:30, so a day change
computed against a US session would be wrong. v1 uses Yahoo's own
`previousClose` per ticker, which is already exchange-correct. No market
calendar is implemented, deliberately.

Scope is per-install and held in config, not per-portfolio. Widening it later is
a settings toggle with no migration, because holdings already store the full
Yahoo symbol.

## 7. Screens

### Dashboard

Sidebar portfolio switcher. Top band: total value, day change, total P/L, all
converted to base currency. Allocation donut by asset class. Holdings table
with ticker, name, class, shares, price, value, day percent, P/L, and weight
against target. A row of drift cards at the top, limited to the two v1 can
compute honestly: band breached, and cash to deploy.

### Builder

Create, rename and delete portfolios. Add, edit and delete holdings, with the
scope-filtered ticker search on the add path. Target weight per holding with a
running sum indicator ("sums to 97.0%") that warns but does not block. Drift
band per portfolio, defaulting to the design's 3 percent.

### Rebalance

One table per holding: target percent, actual percent, drift, and whether the
band is breached. Below it the generated trade list, for example "buy 12 EQNR.OL
(about 14,200 NOK)". An optional cash-to-deploy input biases the result toward
buys only.

### The other eight

Rendered in the sidebar, greyed, not clickable. No mock data.

## 8. Error handling

This is a money surface, so failure behaviour is specified rather than left to
the implementation.

- **Yahoo unreachable or blocked.** Serve the cached quotes, each stamped with
  its age. The UI shows a banner and dims stale figures. Never zero, never a
  blank screen, never a crash. Yahoo's endpoint is unofficial and is expected to
  break eventually; this is the path that keeps the app usable when it does.
- **Unknown or delisted ticker.** The holding renders as "no quote" and is
  excluded from totals *with an explicit count*, for example "2 holdings
  unpriced". It is never valued at 0, because a silently wrong total is worse
  than a visibly incomplete one.
- **Corrupt `portfolios.json`.** Copy to `.bak`, refuse all writes, surface the
  error. Losing positions is the one unacceptable outcome.
- **Writes.** Temp file plus rename, under an `fs4` lock. A crash mid-save
  leaves the previous file intact.
- **Targets not summing to 100.** Permitted while editing and flagged in
  Builder. Rebalance declines to generate trades until it is fixed, rather than
  producing trades from an incoherent target.
- **Missing FX rate.** Treated exactly like a missing quote: the holding is
  unpriced and counted in the warning, not converted at 1.0.

## 9. Testing

- `calc.rs`: table-driven Rust unit tests covering weights, drift, trade
  generation, FX conversion and cost-basis P/L. This is the money path and gets
  real coverage.
- `quotes.rs`: parse a checked-in Yahoo JSON fixture, plus the staleness and
  missing-ticker branches. No test touches the network.
- `store.rs`: roundtrip and the corrupt-file path, using `tempfile`.
- Frontend: vitest over the formatting helpers only. No component tests in v1.

## 10. Release and update

Ported wholesale from gitdashy:

- `ci.yml` jobs: `test`, `version-bumped`, `release`, `binaries`, `publish`.
  A draft release is created first and published only once every binary and its
  `.sha256` are attached, so the updater never sees an assetless release.
- The matrix ships `ubuntu-22.04` only to start. The macOS and Windows rows are
  present but commented out, as in gitdashy. 22.04 rather than latest, because
  the binary's glibc floor is whatever it was built against.
- `version-bumped` fails any PR whose `src-tauri/Cargo.toml` version matches the
  base branch.
- `update.rs` finds the newest `vX.Y.Z` tag, downloads the asset for the current
  platform over the running executable, verifies the sha256, and re-execs.
- `install.sh` downloads the same assets from `releases/latest`.
- A manual `workflow_dispatch` produces a `-rcN` prerelease, which the updater's
  numeric-anchored tag regex never matches, so testers take it by name.

The version in `src-tauri/Cargo.toml` is the single source of truth. An
`AGENTS.md` carrying the bump rule and the pre-push checks is copied from
gitdashy.

## 11. Open questions

None blocking. Decisions taken rather than deferred:

- Scope default is `scandinavia`, base currency NOK.
- Recharts and Zustand are in from the start; no router.
- Storage is JSON files. SQLite is reconsidered when Analytics brings daily
  history, which is the first thing JSON genuinely cannot carry.
