# Stockbit fixtures

Recorded 2026-10-18 (a Sunday, so the market was closed and every number is Friday
2026-10-16's close) against `https://exodus.stockbit.com`, GET only, with the headers
`authorization: Bearer <access JWT>`, `accept: application/json`,
`origin: https://stockbit.com`, `referer: https://stockbit.com/` and a Chrome user agent.
The token belongs to a real account. Responses were checked for its username, email,
full name and user id, and for per-user flags such as `is_holding_exist`, before they
were copied here. Nothing in this directory is a credential.

| file | request | notes |
| --- | --- | --- |
| orderbook_bbca.json | `/company-price-feed/v2/orderbook/companies/BBCA` | quote fields (`lastprice`, `open`, `high`, `low`, `previous`, `volume` in SHARES, `value` IDR) plus the depth: `bid[]` / `offer[]` of `{price, que_num, volume}` as strings, volume in SHARES (the offer levels sum to `total_bid_offer.offer.raw_lot`, which is therefore shares too despite its name). BBCA closed at ARB with no bids, so `bid` is `[]` |
| orderbook_ihsg.json | `/company-price-feed/v2/orderbook/companies/IHSG` | indices answer on the same route: OHLC, volume in shares, empty book |
| orderbook_zzzz.json | same, `ZZZZ` | HTTP 404 `{"message":"Company not found","error_type":"NOT_FOUND"}` |
| chart_candle_bbca_1m.json | `/charts/BBCA/daily?timeframe=1m&chart_type=PRICE_CHART_TYPE_CANDLE` | daily bars; `prices[]` of `{date (epoch ms, bar start WIB), formatted_date, open, high, low, value (= close), volume (LOTS)}` as strings |
| chart_candle_bbca_1w.json | `timeframe=1w&is_include_previous_historical=true`, candle | HOURLY bars (`interval_in_minutes: 60`), 09:00 .. 16:00 plus the 16:14 closing print; the hourly volumes of 2026-10-16 sum to that day's daily volume (12,410,982 lots) |
| chart_candle_bbca_1y.json | `timeframe=1y`, candle | daily bars for a year (240) |
| chart_candle_bbca_3y.json | `timeframe=3y`, candle | WEEKLY bars dated by the week's first session; `5y` is weekly too |
| chart_candle_bbca_1d.json | `timeframe=1d`, candle | `prices: []` outside the session |
| chart_candle_zzzz_1m.json | `/charts/ZZZZ/daily?...` | HTTP 404 `NOT_FOUND` |
| market_session.json | `/company-price-feed/market-time/session` | `detail.regular.state_name`, e.g. `STATE_NAME_MARKET_CLOSED` |
| unauthorized.json | any route with an invalid bearer | HTTP 401 `{"error_type":"UNAUTHORIZED","message":"Unauthorized"}` |

Constructed, not recorded (the tokens inside are dummies signed by nobody):

| file | stands for | why constructed |
| --- | --- | --- |
| hotlist_gainers_constructed.json | `/emitten/hotlist/topGainer?limit=N` | the route is documented but no response has been recorded; this pins the parser's envelope walking and its dropping of a row with no symbol |
| hotlist_losers_constructed.json | `/emitten/hotlist/topLoser?limit=N` | as above, wrapped in `data.result` to cover the other envelope shape |
| running_trade_constructed.json | `/order-trade/running-trade?symbols[]=BBCA&…` | as above; covers a sell print, a buy print, a print with no side and one with no clock |
| login_refresh_constructed.json | `POST /login/refresh` (bearer = refresh token, body `{}`) | recording one spends a real refresh token; the shape is the one a long-running bot reads every day (`data.access.token`, `data.refresh.token`). The parser searches structurally, so a wrapper level more or less still parses |
| credential_storage_cookie_constructed.txt | the `credentialStorage` cookie stockbit.com keeps after a login | URL-encoded `{"state":{"access":{"token","expired_at"},"refresh":{...},"user":{...}},"version":0}`, the layout documented by github.com/ino-xious/stockbit-mcp; the parser also accepts bare JWT strings in `access` / `refresh` |
