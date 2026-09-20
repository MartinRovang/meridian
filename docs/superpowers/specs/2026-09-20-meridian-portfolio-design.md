# Meridian: portfolio management and surveillance

Design spec, 2026-09-20. Status: approved, pre-implementation.
Revision 2: server and app split into separate crates and separate release binaries.

## 1. What this is

A desktop app for managing and watching a personal investment portfolio, talking
to a JSON API that can run either in-process or on a server you host. Positions
are entered by hand; prices come from Yahoo Finance; all portfolio math runs in
Rust and the frontend only renders it.

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
scope, the client/server split, and the release and self-update pipeline.

### Deliberately out of scope for v1

The remaining eight screens stay visible in the sidebar, greyed and
non-clickable, so the shape of the product is legible without fake data:
Analytics, Sentiment, Fixed income, Energy & shipping, Stress test, Backtest,
Alerts, Rules.

Also out: broker API integration, CSV import, Nordnet, Oslo Børs API, Norwegian
tax reporting (skattemelding, skjermingsfradrag), any background daemon, and
multi-user accounts. Each is its own sub-project with its own spec.

CSV import is the expected second sub-project. It writes to the same store the
manual path writes to, so it is additive rather than a rework.

## 3. Repository and stack

New public GitHub repo `MartinRovang/meridian`, local checkout
`~/Desktop/meridian`. Bundle identifier `com.martinrovang.meridian`.

The stack follows `github-dashy`, because it is known to work and the release
machinery is being ported from it:

- Rust, Tauri 2, `tiny_http`, `ureq`, `serde`, `clap`, `fs4`.
- Vite 8, React 19, TypeScript, oxlint, vitest, pnpm.
- Added beyond gitdashy: Recharts and Zustand. No router; screen selection is a
  URL hash. Both are known to have no v1 consumer: Recharts waits for
  Analytics, Zustand holds the API base, token and fetched state.
- Phosphor icons are installed as an npm package and bundled, not CDN-linked,
  because the Tauri CSP blocks external scripts.

Nocturne's `styles.css` and `nocturne.css` are vendored verbatim into `src/`.
They are a token sheet plus a component layer (`.btn`, `.card`, `.table`,
`.field`, `.dialog`) and need no framework. The `<x-dc>` / `sc-for` markup in
the design file is a preview runtime and is not shipped; screens are translated
to React by hand.

Charts in the design are hand-built SVG and CSS bars. Those are ported as
markup. If a v1 chart is less code hand-rolled than configured, it stays
hand-rolled.

### Bootstrapping from gitdashy

A fresh scaffold, with three things ported:

1. `.github/workflows/ci.yml`, extended to build two binaries.
2. `install.sh`, with names swapped and a component selector.
3. `src-tauri/src/update.rs`, with `REPO` pointed at `MartinRovang/meridian`.

The `include_dir!` static-serving half of gitdashy's `web.rs` is **not** ported.
The Tauri app serves its own UI through `frontendDist` on the asset protocol,
and the server is pure JSON. That deletes the whole static-asset path.

Everything else is written new. gitdashy's remaining code is GitHub, PR and LLM
logic with no bearing here, and its config schema is wrong for this app.

## 4. Architecture

### Workspace layout

```
meridian/
├─ Cargo.toml              workspace root
├─ crates/
│  ├─ core/                meridian-core
│  │                       types, store, quotes, calc, config
│  │                       No HTTP, no Tauri, no UI. Pure logic plus its tests.
│  └─ server/              meridian-server
│                          lib.rs, embeddable by the app
│                          main.rs, the deployable binary
├─ src-tauri/              meridian, the Tauri app
│                          depends on meridian-server as a library
└─ src/                    React UI, bundled into the app
```

The dependency direction is one way: `core` knows nothing about `server`,
`server` knows nothing about `src-tauri`. `core` must never gain a Tauri,
`tiny_http` or webkit dependency; that constraint is what keeps the server
deployable.

### Two binaries

- **`meridian-server`**: links no Tauri, no WebKitGTK, no GTK. Runs on any
  Linux box with no desktop libraries.
  `meridian-server --store /var/lib/meridian --token $SECRET --port 8080`.
- **`meridian`**: the desktop app. It bundles `meridian-server` as a library,
  so local mode needs no second process and no second file installed.

### Two modes, one code path

| Mode | Behaviour |
|---|---|
| Local (default) | The app starts the server in-process on `127.0.0.1:0` with a freshly generated token, and points the UI at it. |
| Remote | `meridian --api-url https://box.example --token SECRET`. No in-process server; the UI talks to the hosted one. |

The frontend cannot tell the modes apart. It reads `api` and `token` from its
launch URL query, stores them in Zustand, and every request goes through one
`api.ts` helper. There are no relative `/api/` fetches anywhere in the
frontend. This is what makes the split real rather than notional.

### Auth and origins

Auth is a shared secret: `X-Meridian-Token` on every request, compared in
constant time. There is exactly one store per server and one token that opens
it. No accounts, no per-user data, no TLS termination in-process; a hosted
deployment sits behind Caddy or nginx.

gitdashy's `Host` allowlist of `127.0.0.1`/`localhost` is **not** ported. It is
meaningless once the server is legitimately remote, and keeping it would break
the mode this whole split exists to enable.

The server sends CORS headers, because the app's asset-protocol origin
(`tauri://localhost`) and the API origin are different in both modes. The
allowed origin is configurable and defaults to permissive, since the token is
what actually guards the API.

The token reaches the frontend in its launch URL query. That URL is the asset
protocol in local mode and never leaves the machine. From the frontend onward
the token travels only as a header.

### Modules

| Crate | Module | Responsibility | Rough size |
|---|---|---|---|
| core | `types.rs` | serde types shared by every layer | 120 |
| core | `config.rs` | store path, market scope, base currency | 80 |
| core | `store.rs` | read and write `portfolios.json`; atomic replace under an `fs4` lock | 150 |
| core | `quotes.rs` | Yahoo quote, search and FX fetch; on-disk cache; staleness | 250 |
| core | `calc.rs` | value, P/L, weights, FX conversion, drift, rebalance trades | 200 |
| server | `lib.rs` | routes, token guard, CORS, `serve()` | 350 |
| server | `main.rs` | clap: `--store`, `--token`, `--port`, `--scope` | 60 |
| src-tauri | `main.rs` | clap: `--api-url`, `--token`, `--port`; local or remote boot | 80 |
| src-tauri | `shell.rs` | the Tauri window | 100 |
| src-tauri | `update.rs` | ported from gitdashy | 469 |

**All portfolio arithmetic lives in `core/calc.rs`.** The frontend fetches one
`/api/state` blob and renders it. There is no money math in TypeScript. One
place to test the money path, one place for it to be wrong.

### API

Every route requires `X-Meridian-Token`.

| Route | Purpose |
|---|---|
| `GET /api/state` | the whole computed view: portfolios, holdings, derived figures, quote freshness, warnings |
| `POST /api/portfolio` | create |
| `PATCH /api/portfolio/:id` | rename, set owner, set drift band, delete |
| `POST /api/holding` | create or update a holding |
| `DELETE /api/holding/:id` | remove |
| `POST /api/refresh` | force a quote refresh |
| `GET /api/search?q=` | ticker search, filtered by market scope |
| `GET /api/health` | version and store path; no token required |

## 5. Data model

### `<store>/portfolios.json`

The whole persisted state, one file. The store directory is a parameter,
defaulting to `~/.meridian` locally and required explicitly on a server.

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

### `<store>/quotes.json`

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

Scope lives on the server, since that is what talks to Yahoo. Widening it later
is a settings change with no migration, because holdings already store the full
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
- **Server unreachable from the app.** The app shows a connection error naming
  the API base it tried, with a retry. It does not fall back to a local server,
  because silently showing a different store's numbers is worse than an error.

## 9. Testing

- `core/calc.rs`: table-driven Rust unit tests covering weights, drift, trade
  generation, FX conversion and cost-basis P/L. This is the money path and gets
  real coverage.
- `core/quotes.rs`: parse a checked-in Yahoo JSON fixture, plus the staleness
  and missing-ticker branches. No test touches the network.
- `core/store.rs`: roundtrip and the corrupt-file path, using `tempfile`.
- `server`: routes tested against a real `serve()` on an ephemeral port with a
  temp store, covering the token guard (accepted, rejected, missing) and one
  round trip per route.
- Frontend: vitest over the formatting helpers and the `api.ts` base-URL and
  header construction. No component tests in v1.

## 10. Release and update

Ported from gitdashy and extended for the second binary.

- `ci.yml` jobs: `test`, `version-bumped`, `release`, `binaries`, `publish`.
  A draft release is created first and published only once every asset and its
  `.sha256` are attached, so the updater never sees an assetless release.
- **Each release carries two assets per platform:**
  `meridian-linux-x86_64` (the app) and `meridian-server-linux-x86_64`
  (the server), each with a `.sha256`.
- The server binary builds without the desktop apt dependencies, so its job does
  not install `libwebkit2gtk`. That job failing to build on a plain container is
  the check that the crate separation is real.
- The matrix ships `ubuntu-22.04` only to start. The macOS and Windows rows are
  present but commented out, as in gitdashy. 22.04 rather than latest, because
  the binary's glibc floor is whatever it was built against.
- `version-bumped` fails any PR whose workspace version matches the base branch.
- `update.rs` updates the app binary only. A server you host is updated the way
  you deploy anything else; the desktop updater must not reach across and
  replace a remote deployment.
- `install.sh` installs the app by default. `COMPONENT=server sh install.sh`
  fetches the server binary instead, for the host.
- A manual `workflow_dispatch` produces a `-rcN` prerelease, which the updater's
  numeric-anchored tag regex never matches, so testers take it by name.

The workspace version is the single source of truth and both binaries carry it.
An `AGENTS.md` with the bump rule and the pre-push checks is adapted from
gitdashy.

## 11. Decisions taken

No open questions. Recorded so they are not relitigated:

- Scope default is `scandinavia`, base currency NOK.
- Recharts and Zustand are in from the start; no router.
- Storage is JSON files. SQLite is reconsidered when Analytics brings daily
  history, which is the first thing JSON genuinely cannot carry.
- Hosted mode is single-user behind a shared secret. Accounts are a separate
  sub-project.
- The server does not serve the UI. Browser access is a later, small job.
- Cost basis is stored as a position total, not a per-share average.
- The drift band is per portfolio, not per holding.
