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
