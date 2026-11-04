# Stockbit realtime (opt-in)

`idx` reads IDX data from free, keyless public endpoints. **Stockbit is the one
exception**: an opt-in provider that serves realtime quotes, history and a live
order book from [Stockbit](https://stockbit.com)'s own backend, carrying **your
own Stockbit account session**. It is never in the `auto` chain and is used only
when you ask for it with `--provider stockbit`.

> This uses an unofficial, undocumented API with your personal account. It may
> conflict with Stockbit's Terms of Use, and automated access could get an
> account flagged. Use your own account, at your own risk, at a human-ish
> request rate. `idx` is not affiliated with or endorsed by Stockbit.

## Log in once

```sh
idx stockbit login
```

This launches a Chromium-family browser (Chrome, Edge, Brave; set `IDX_BROWSER`
to choose one) pointed at Stockbit's login page. You sign in — password, OTP,
whatever Stockbit asks — and `idx` captures the session token off the network
automatically. If you are already signed in in that browser profile, it reuses
that session in a couple of seconds.

The session is stored locally (see **Where the session lives**) and refreshed
for you from then on. Check it any time:

```sh
idx stockbit status        # logged in? when do the tokens expire?
idx stockbit logout        # delete the stored session
```

No Chromium browser, or a login that will not drive? Paste the token yourself:

```sh
idx stockbit login --manual
```

It prints where to copy the `credentialStorage` cookie from (DevTools →
Application → Cookies → `https://stockbit.com`) and reads it from stdin.

## Use it

Once logged in, point any quote/history command at the provider:

```sh
idx --provider stockbit stocks quote BBCA BBRI      # realtime, not delayed
idx --provider stockbit stocks history BBCA --period 1mo
idx config set provider stockbit                    # make it the default
```

And read the live order book (Stockbit-only — the free sources have no depth):

```sh
idx stockbit orderbook BBCA            # depth ladder + session foreign flow
idx stockbit orderbook BBRI --depth 5
idx -o json stockbit orderbook TLKM | jq '.data.bids[0]'
```

Sizes in the order book are **shares** (one IDX lot is 100 shares); values are
IDR. History candle volumes are converted from Stockbit's lots to shares so they
match the other providers. Stockbit chooses the candle granularity from the
window — `1mo`/`3mo`/`1y` are daily, `5d`/intraday is hourly, `5y` is weekly —
and a warning says so when it is not the daily bars the interval nominally asks
for.

The Stockbit provider covers `quote`, `history`, `orderbook`, and the hotlist and
running-trade routes behind the monitor. The other commands (fundamentals,
financials, news, …) stay on the free providers; run them without
`--provider stockbit`.

## The monitor

`idx stockbit monitor` is a full-screen board for watching the session without
leaving the terminal:

```sh
idx stockbit monitor                    # starts on your watchlist's first ticker (else BBCA)
idx stockbit monitor BBRI --interval 2
idx stockbit monitor GOTO --top 20 --trades 100
```

```
╭ idx · stockbit monitor ───────────────────────────────────────────────╮
│ BBRI  Bank Rakyat Indonesia Tbk.  4,120  +30 (+0.73%)                 │
│ H 4,150 L 4,080   vol 812,004,100   foreign +182.4B   every 5s 12:24  │
╰───────────────────────────────────────────────────────────────────────╯
```

Four panes, refreshed together on one clock:

| pane | source | shows |
| --- | --- | --- |
| **Chart** | `/charts/{SYM}/daily?timeframe=1d` | today's one-minute candles, drawn with braille so a price lands inside a cell. After the close Stockbit sends none, so it falls back to five days of hourly bars and says `1h · 5d` in the title |
| **Top gainer** | `/emitten/hotlist/topGainer` | the movers list, one row per ticker: last price and percent change |
| **Top loser** | `/emitten/hotlist/topLoser` | as above, falling |
| **Running trade** | `/order-trade/running-trade?symbols[]=SYM` | newest print first: time, price, size, buy/sell, the buyer and seller broker codes |

Keys: `↑↓` (`j`/`k`) move in the mover lists or scroll the tape, `tab` (`h`/`l`)
switches which pane the arrows drive, `enter` loads the selected mover into the
chart and tape, `pgup`/`pgdn` scroll the tape ten prints, `g`/`G` jump to the
first/last row, `r` refreshes now, `+`/`-` change the polling interval (1–120 s),
`?` opens the key sheet and `q` (or `ctrl+c`) quits.

A pane that fails keeps its last good value and the reason appears as a
`N warning(s)` chip in the header; `?` lists them. `--offline` fills the panes
from whatever Stockbit responses are still cached, exactly as elsewhere.

`--interval` defaults to 5 s. The three realtime routes are cached for 5 s
whatever the interval, so a burst of key presses or a fast poll coalesces into
one upstream request instead of one per pane. Keep it human either way: this is
your own account on an undocumented endpoint.

### Field names are unverified

The hotlist and running-trade routes are the two Stockbit endpoints `idx` calls
but has **not** recorded a real response from. Their paths come from community
reverse-engineering (`INo-xious/stockbit-mcp`, `satriyop/ai-saham`), which
documents routes but not field names. Rather than hard-code one guess, the
parsers in `providers/stockbit/parse.rs` read the first key they find among a
small candidate list — `last`/`close`/`price`, `percent`/`percentage`/
`change_pct`, `action`/`side`/`type` — and walk the envelope
(`data`, `data.result`, …) before giving up, so an empty after-hours list is a
blank pane and not an error.

That is tolerance, not verification. Before trusting the numbers, record one
response per route into `tests/fixtures/stockbit/` and replace the
`*_constructed.json` fixtures the parser tests currently use. The same applies
to units: a print's size is shown exactly as Stockbit sends it, and Stockbit's
own UI quotes running trade in **lots** while the order book quotes **shares**.

The monitor writes nothing to stdout and refuses to run without a terminal — it
is the one `idx` command with no table or JSON form, and it has no MCP tool or
web-terminal equivalent for the same reason.

## MCP

The MCP server exposes three Stockbit tools alongside the rest:

- `idx_stockbit_status` — is a session stored, and when do the tokens expire
  (never the token values). Call it before `idx_orderbook`.
- `idx_orderbook` — the live depth ladder and foreign flow for one ticker.
- `idx_stockbit_login` — opens a browser **on the machine running the server**
  for the user to sign in; only meaningful for a local stdio server the user is
  sitting at, never a remote HTTP one. Prefer telling the user to run
  `idx stockbit login` in a terminal themselves.

To serve realtime quotes/history over MCP, run the server with the Stockbit
provider: `idx --provider stockbit mcp`.

## Where the session lives

The session file holds the rotating tokens and is written with `0600`
permissions on Unix. By default it sits beside the config file (e.g.
`%APPDATA%\idx\stockbit-session.json` on Windows,
`~/.config/idx/stockbit-session.json` on Linux). Override it:

- `--config`-relative default, or `stockbit.session` in `config.toml`;
- `IDX_STOCKBIT_SESSION=/path/to/session.json`.

For an unattended deployment (a server, the web worker) seed the tokens through
the environment instead of logging in interactively:

- `IDX_STOCKBIT_REFRESH_TOKEN` — the refresh token (required);
- `IDX_STOCKBIT_ACCESS_TOKEN` — an access token to start with (optional).

The file always wins over the env seeds; the seeds only fill an empty session.

## One refresher per session

Stockbit's `POST /login/refresh` **rotates** the refresh token: every refresh
mints a new one and retires the token it was given. So a single session must
have exactly one process refreshing it. If you point two tools (say `idx` and
another bot) at the *same* Stockbit session, they will retire each other's token
and both get logged out. Give each its own `idx stockbit login` / account, or
let one own the refresh and feed the other its current token.

## What `idx` talks to

All on `https://exodus.stockbit.com`, with `Authorization: Bearer <access JWT>`
and browser-shaped `origin`/`referer` headers:

| endpoint | used for |
| --- | --- |
| `GET /company-price-feed/v2/orderbook/companies/{SYM}` | quote (OHLC, volume, value, foreign flow) and the full depth ladder |
| `GET /charts/{SYM}/daily?timeframe=..&chart_type=PRICE_CHART_TYPE_CANDLE` | OHLCV candles |
| `POST /login/refresh` | rotate the session (bearer = refresh token, empty body) |

Indices use Stockbit's own code (`IHSG`, `LQ45`), mapped from idx's caret form
(`^JKSE`, `^JKLQ45`). The token is never written to a log, an error message or a
cache key.
