![Meridian](assets/banner.png)

# Meridian

Portfolio management and surveillance, built for Nordic markets first.

Hold Oslo, Stockholm and Copenhagen tickers alongside anything else, entered by
hand, priced from Yahoo's public chart endpoint, and converted into one base
currency. Meridian tells you what you own, what it is worth today, how far each
position has drifted from its target, and the trades that would put it back.

## Two binaries, and why

| | |
|---|---|
| `meridian` | the desktop app. Runs the API in-process on a loopback port by default, so there is one file to install and nothing to configure. |
| `meridian-server` | the same API as a standalone service, for a box you host. Links no desktop libraries at all: CI builds it in a bare `rust:1-slim` container to keep it that way. |

The app talks to its API over HTTP whether that API is in its own process or on
another machine. Starting a hosted deployment is a flag, not a rewrite.

## Install the desktop app

```sh
curl -fsSL https://raw.githubusercontent.com/MartinRovang/meridian/main/install.sh | sh
```

Then run `meridian`, or launch it from your app menu. On Linux it needs the
system webview (`libwebkit2gtk-4.1-0`, `libgtk-3-0`).

Your portfolios live in `~/.meridian/portfolios.json`.

## Install the server on a host

```sh
COMPONENT=server curl -fsSL https://raw.githubusercontent.com/MartinRovang/meridian/main/install.sh | sh
```

```sh
MERIDIAN_TOKEN=$(openssl rand -hex 24) meridian-server --port 8080 --scope scandinavia
```

Given no token it generates one and prints it to stderr. Prefer the environment
variable: an argument is visible to every user on the box through `ps`.

A systemd unit:

```ini
[Unit]
Description=Meridian API
After=network.target

[Service]
ExecStart=/usr/local/bin/meridian-server --port 8080 --store /var/lib/meridian
Environment=MERIDIAN_TOKEN=your-shared-secret
User=meridian
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

**It binds `127.0.0.1` only.** Put it behind a reverse proxy that terminates
TLS; the token is a bearer secret and must not cross the network in the clear.

Then point the app at it:

```sh
meridian --api-url https://meridian.example.com --token your-shared-secret
```

Or set `MERIDIAN_TOKEN` and leave `--token` off.

### Hosted mode is single-user

There are no accounts. One shared secret grants full access to every portfolio
in the store. That is a deliberate v1 decision, not an oversight: it is meant
for one person reaching their own data from more than one machine. Do not hand
the token to someone you would not hand the whole store to.

## Market scope

Ticker search is filtered to the markets you work in, which is what makes it
usable: searching "equinor" globally returns the NYSE listing first, then
Frankfurt, then pink sheets, then Düsseldorf twice.

| `--scope` | Exchanges | Default currency |
|---|---|---|
| `norway` | Oslo | NOK |
| `scandinavia` *(default)* | Oslo, Stockholm, Copenhagen | NOK |
| `nordics` | + Helsinki, Reykjavík | NOK |
| `europe` | + Frankfurt, Paris, Amsterdam, London, Milan | EUR |
| `global` | everything Yahoo returns | USD |

## What v1 does

**Dashboard** — total value, day change and P/L in your base currency,
allocation by class, drift against target, and the holdings table.

**Builder** — create portfolios, search tickers within your scope, add and edit
holdings, set target weights and the drift band.

**Rebalance** — the drift table and the trades that would close it. Give it
cash to deploy and it buys only, spending exactly what you said you had.

The other eight screens are visible in the sidebar and greyed out. They are the
shape of where this is going, not a promise about when.

## Multiple currencies

A portfolio can hold anything. Meridian fetches the FX pairs it needs on its own
(`USDNOK=X`, `SEKNOK=X`) and converts into your base currency.

**A missing price is never a zero.** If a quote or an FX rate cannot be had, the
holding is marked unpriced, excluded from every total, and named above the
table. A portfolio that is quietly worth less than it is would be worse than one
that admits it does not know.

## Updating

```sh
meridian --check-update    # is there a newer release
meridian --update          # install it over this binary and restart
```

Both check that the release actually carries a binary for your machine before
offering it, and refuse a download whose sha256 does not match the one published
beside it.

`meridian-server` has no self-updater. A service you host is updated the way you
deploy anything else.

## Building from source

```sh
pnpm install && pnpm build
cargo build --release              # both binaries
cargo build --release -p meridian-server   # just the server, no webview needed
```

See `AGENTS.md` for the rules that keep the crate split honest.

## License

MIT
