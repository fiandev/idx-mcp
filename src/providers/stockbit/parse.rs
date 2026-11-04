//! Pure parsers for Stockbit's exodus responses, tested against the recorded
//! fixtures in `tests/fixtures/stockbit/`. No field here is invented: a value
//! the response omits, or carries as `""` / `N/A` / `NA`, becomes `None`.
//!
//! Units, measured and fixed by the exchange:
//! - orderbook `volume` and the depth ladder's `volume` are **shares**;
//! - chart candle `volume` is **lots**, converted to shares here (×100) so a
//!   Stockbit candle's volume matches Yahoo's and the orderbook's.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_json::Value;

use crate::error::{IdxError, Result};
use crate::model::monitor::{Mover, MoverKind, Movers, RunningTrade, Trade, TradeSide};
use crate::model::orderbook::{BookLevel, OrderBook};
use crate::model::{wib, wib_from_unix, Candle, History, Interval, Period, Quote, Symbol};

use super::NAME;

/// A number from a field that may be a JSON number or a numeric string, with
/// thousands separators and the source's various "missing" spellings handled.
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim().replace(',', "");
            if t.is_empty() || t.eq_ignore_ascii_case("n/a") || t.eq_ignore_ascii_case("na") {
                None
            } else {
                t.parse().ok()
            }
        }
        _ => None,
    }
}

fn nonneg_u64(v: &Value) -> Option<u64> {
    num(v).filter(|n| *n >= 0.0).map(|n| n as u64)
}

/// `true` when the body is Stockbit's not-found error (`error_type: NOT_FOUND`).
pub fn is_not_found(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error_type")
                .and_then(|e| e.as_str())
                .map(String::from)
        })
        .is_some_and(|t| t.eq_ignore_ascii_case("NOT_FOUND"))
}

/// The `message` of an error envelope, for surfacing a provider failure.
pub fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or_else(|| "unexpected response".into())
}

/// A quote from the orderbook endpoint (`/company-price-feed/v2/orderbook/...`),
/// which carries OHLC, volume, value and the foreign flow in one response.
pub fn parse_quote(symbol: &Symbol, body: &str) -> Result<Quote> {
    let v: Value = serde_json::from_str(body)?;
    let d = v
        .get("data")
        .ok_or_else(|| IdxError::provider(NAME, "quote response had no data"))?;
    let g = |k: &str| d.get(k).and_then(num);
    let mut q = Quote::empty(symbol.clone(), "stockbit:orderbook");
    q.name = d.get("name").and_then(|n| n.as_str()).map(String::from);
    q.currency = Some("IDR".into());
    q.price = g("lastprice").or_else(|| g("close"));
    q.change = g("change");
    q.change_pct = g("percentage_change");
    q.previous_close = g("previous");
    q.open = g("open");
    q.day_high = g("high");
    q.day_low = g("low");
    q.volume = d.get("volume").and_then(nonneg_u64);
    Ok(q)
}

/// The full depth ladder plus the session figures and foreign flow.
pub fn parse_orderbook(symbol: &Symbol, body: &str) -> Result<OrderBook> {
    let v: Value = serde_json::from_str(body)?;
    let d = v
        .get("data")
        .ok_or_else(|| IdxError::provider(NAME, "orderbook response had no data"))?;
    let g = |k: &str| d.get(k).and_then(num);
    let levels = |key: &str| -> Vec<BookLevel> {
        d.get(key)
            .and_then(|a| a.as_array())
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        Some(BookLevel {
                            price: r.get("price").and_then(num)?,
                            shares: r.get("volume").and_then(nonneg_u64).unwrap_or(0),
                            orders: r.get("que_num").and_then(nonneg_u64).unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(OrderBook {
        symbol: symbol.clone(),
        name: d.get("name").and_then(|n| n.as_str()).map(String::from),
        last: g("lastprice").or_else(|| g("close")),
        change: g("change"),
        change_pct: g("percentage_change"),
        previous_close: g("previous"),
        open: g("open"),
        day_high: g("high"),
        day_low: g("low"),
        volume: d.get("volume").and_then(nonneg_u64),
        value: g("value"),
        bids: levels("bid"),
        asks: levels("offer"),
        foreign_buy: g("fbuy"),
        foreign_sell: g("fsell"),
        foreign_net: g("fnet"),
    })
}

/// Maps an idx look-back window to a Stockbit chart timeframe. Stockbit's
/// vocabulary is windows, not bar sizes, and it chooses the granularity: `1w`
/// is hourly, `1m`/`3m`/`ytd`/`1y` are daily, `3y`/`5y` are weekly. Returns the
/// timeframe and whether the previous-period flag is needed.
pub fn timeframe_for(period: Period, interval: Interval) -> (&'static str, bool) {
    // An intraday request maps to the hourly (1w) or same-day (1d) windows.
    if interval.is_intraday() {
        return match period {
            Period::D1 => ("1d", false),
            _ => ("1w", true),
        };
    }
    match period {
        Period::D1 => ("1d", false),
        Period::D5 => ("1w", true),
        Period::M1 => ("1m", false),
        Period::M3 => ("3m", false),
        Period::M6 | Period::Ytd => ("ytd", true),
        Period::Y1 => ("1y", false),
        Period::Y2 => ("3y", false),
        Period::Y5 | Period::Y10 | Period::Max => ("5y", false),
    }
}

/// Candles from `/charts/{SYM}/daily`. `prices[]` rows carry `date` (epoch ms,
/// the bar start in WIB), string OHLC and a `value` that is the close; `volume`
/// is in lots and is converted to shares. Rows without a usable close or time
/// are dropped. Returns oldest first (the source already orders them so).
pub fn parse_history(
    symbol: &Symbol,
    period: Period,
    interval: Interval,
    body: &str,
) -> Result<History> {
    let v: Value = serde_json::from_str(body)?;
    let d = v
        .get("data")
        .ok_or_else(|| IdxError::provider(NAME, "chart response had no data"))?;
    let rows = d
        .get("prices")
        .and_then(|p| p.as_array())
        .ok_or_else(|| IdxError::provider(NAME, "chart response had no prices"))?;
    let mut candles: Vec<Candle> = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(close) = r.get("value").and_then(num) else {
            continue;
        };
        let Some(time) = candle_time(r) else { continue };
        // Line-type rows leave OHLC empty; fall back to the close so a candle is
        // never silently zero-filled.
        let open = r.get("open").and_then(num).unwrap_or(close);
        let high = r.get("high").and_then(num).unwrap_or(close);
        let low = r.get("low").and_then(num).unwrap_or(close);
        let lots = r.get("volume").and_then(nonneg_u64).unwrap_or(0);
        candles.push(Candle {
            time,
            open,
            high,
            low,
            close,
            adj_close: None,
            volume: lots.saturating_mul(100),
        });
    }
    let summary = History::summarise(&candles);
    Ok(History {
        symbol: symbol.clone(),
        name: None,
        currency: Some("IDR".into()),
        period,
        interval,
        candles,
        summary,
    })
}

/// A candle's start time: `date` is epoch milliseconds in most windows, but the
/// intraday (`1d`) window sends `date: "0"` and the clock in `formatted_date`
/// (`YYYY-MM-DD HH:MM:SS`, WIB). Both are read; a daily `formatted_date` is a
/// bare date.
fn candle_time(row: &Value) -> Option<DateTime<FixedOffset>> {
    if let Some(ms) = row.get("date").and_then(num) {
        if ms >= 1.0 {
            return wib_from_unix((ms / 1000.0) as i64);
        }
    }
    let f = row.get("formatted_date").and_then(|s| s.as_str())?.trim();
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(f, "%Y-%m-%d %H:%M:%S") {
        return dt.and_local_timezone(wib()).single();
    }
    let date = chrono::NaiveDate::parse_from_str(f, "%Y-%m-%d").ok()?;
    date.and_hms_opt(0, 0, 0)?
        .and_local_timezone(wib())
        .single()
}

/// The first key of a row that holds a number, in the caller's order of trust.
fn pick_num(row: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| row.get(*k).and_then(num))
}

/// The first key of a row that holds a non-empty string.
fn pick_str(row: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        row.get(*k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    })
}

/// The row array of a response, wherever the envelope buried it.
///
/// Stockbit wraps lists inconsistently: `data` is sometimes the array itself,
/// sometimes `data.result`, sometimes one named key per list. Rather than
/// hard-code one path and break on the next rename, walk the envelope up to a
/// few levels and take the first array found. Returns `None` for a body with no
/// list at all, which is how an empty after-hours hotlist can arrive.
fn row_array(v: &Value) -> Option<&Vec<Value>> {
    fn walk(v: &Value, depth: usize) -> Option<&Vec<Value>> {
        if let Some(a) = v.as_array() {
            return Some(a);
        }
        if depth == 0 {
            return None;
        }
        let o = v.as_object()?;
        for k in [
            "result",
            "rows",
            "items",
            "trades",
            "data",
            "companies",
            "topGainer",
            "topLoser",
            "mostActive",
            "running_trade",
        ] {
            if let Some(found) = o.get(k).and_then(|x| walk(x, depth - 1)) {
                return Some(found);
            }
        }
        o.values().find_map(|x| walk(x, depth - 1))
    }
    walk(v.get("data")?, 3)
}

/// Rows of `GET /emitten/hotlist/{topGainer|topLoser|mostActive}`.
///
/// A row without a usable symbol is dropped rather than guessed at: the ticker
/// is the one field every consumer keys off.
pub fn parse_movers(kind: MoverKind, body: &str) -> Result<Movers> {
    let v: Value = serde_json::from_str(body)?;
    let rows = row_array(&v).ok_or_else(|| {
        IdxError::provider(NAME, format!("{} response had no list", kind.as_str()))
    })?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(symbol) = pick_str(r, &["symbol", "code", "ticker"]) else {
            continue;
        };
        let Ok(symbol) = Symbol::parse(&symbol) else {
            continue;
        };
        out.push(Mover {
            symbol,
            name: pick_str(r, &["name", "company_name"]),
            last: pick_num(r, &["last", "close", "price", "lastprice"]),
            change: pick_num(r, &["change"]),
            change_pct: pick_num(
                r,
                &["percent", "percentage", "change_pct", "percentage_change"],
            ),
            volume: pick_num(r, &["volume"])
                .filter(|n| *n >= 0.0)
                .map(|n| n as u64),
            value: pick_num(r, &["value"]),
        });
    }
    Ok(Movers { kind, rows: out })
}

/// Rows of `GET /order-trade/running-trade`. A row without a price is dropped.
pub fn parse_running_trade(symbol: &Symbol, body: &str) -> Result<RunningTrade> {
    let v: Value = serde_json::from_str(body)?;
    let rows = row_array(&v)
        .ok_or_else(|| IdxError::provider(NAME, "running trade response had no list"))?;
    let mut trades = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(price) = pick_num(r, &["price"]) else {
            continue;
        };
        let volume = pick_num(r, &["volume", "lot", "lots"])
            .filter(|n| *n >= 0.0)
            .unwrap_or(0.0) as u64;
        trades.push(Trade {
            time: trade_time(r),
            price,
            volume,
            side: pick_str(r, &["action", "side", "type", "buy_sell"])
                .map(|s| side_of(&s))
                .unwrap_or(TradeSide::Unknown),
            buyer: pick_str(r, &["buyer", "buyer_code", "buy_code"]),
            seller: pick_str(r, &["seller", "seller_code", "sell_code"]),
        });
    }
    Ok(RunningTrade {
        symbol: symbol.clone(),
        trades,
    })
}

/// `B` / `BUY` / `ACTION_TYPE_BUY` / `S` / `SELL` -> a side. Anything else is
/// `Unknown`; the tape still shows the print, it just does not colour it.
fn side_of(raw: &str) -> TradeSide {
    let t = raw.trim().to_ascii_uppercase();
    match t.as_str() {
        "B" | "BUY" | "BID" | "ACTION_BUY" | "ACTION_TYPE_BUY" | "SIDE_BUY" => TradeSide::Buy,
        "S" | "SELL" | "OFFER" | "ACTION_SELL" | "ACTION_TYPE_SELL" | "SIDE_SELL" => {
            TradeSide::Sell
        }
        _ => TradeSide::Unknown,
    }
}

/// A print's clock. The route has been seen to send a bare `HH:MM:SS` clock
/// (the session day is then today in WIB), an epoch in ms or s, and a full
/// timestamp. An unrecognised spelling yields `None` rather than a wrong time.
fn trade_time(row: &Value) -> Option<DateTime<FixedOffset>> {
    for key in ["time", "trade_time", "datetime", "created_at"] {
        let Some(v) = row.get(key) else { continue };
        if let Some(raw) = v.as_str() {
            if let Some(t) = parse_clock(raw.trim()) {
                return Some(t);
            }
            continue;
        }
        if let Some(n) = num(v) {
            if n <= 0.0 {
                continue;
            }
            // Milliseconds are > 1e11 for any date this decade.
            let secs = if n > 1e11 { n / 1000.0 } else { n };
            if let Some(t) = wib_from_unix(secs as i64) {
                return Some(t);
            }
        }
    }
    None
}

/// `HH:MM[:SS]` against today's WIB date, then the timestamp spellings.
fn parse_clock(raw: &str) -> Option<DateTime<FixedOffset>> {
    let today: NaiveDate = wib_from_unix(chrono::Utc::now().timestamp())?.date_naive();
    for f in ["%H:%M:%S", "%H:%M"] {
        if let Ok(t) = chrono::NaiveTime::parse_from_str(raw, f) {
            return today.and_time(t).and_local_timezone(wib()).single();
        }
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&wib()));
    }
    for f in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, f) {
            return naive.and_local_timezone(wib()).single();
        }
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)?
        .and_local_timezone(wib())
        .single()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> Symbol {
        Symbol::parse(s).unwrap()
    }

    const ORDERBOOK: &str = include_str!("../../../tests/fixtures/stockbit/orderbook_bbca.json");
    const ORDERBOOK_IHSG: &str =
        include_str!("../../../tests/fixtures/stockbit/orderbook_ihsg.json");
    const NOT_FOUND: &str = include_str!("../../../tests/fixtures/stockbit/orderbook_zzzz.json");
    const CHART_1M: &str =
        include_str!("../../../tests/fixtures/stockbit/chart_candle_bbca_1m.json");
    const CHART_1W: &str =
        include_str!("../../../tests/fixtures/stockbit/chart_candle_bbca_1w.json");
    const CHART_1Y: &str =
        include_str!("../../../tests/fixtures/stockbit/chart_candle_bbca_1y.json");
    const CHART_EMPTY: &str =
        include_str!("../../../tests/fixtures/stockbit/chart_candle_bbca_1d.json");

    #[test]
    fn number_coercion() {
        assert_eq!(num(&serde_json::json!("5,175")), Some(5175.0));
        assert_eq!(num(&serde_json::json!(5175)), Some(5175.0));
        assert_eq!(num(&serde_json::json!("N/A")), None);
        assert_eq!(num(&serde_json::json!("NA")), None);
        assert_eq!(num(&serde_json::json!("")), None);
    }

    #[test]
    fn quote_from_orderbook() {
        let q = parse_quote(&sym("BBCA"), ORDERBOOK).unwrap();
        assert_eq!(q.name.as_deref(), Some("Bank Central Asia Tbk."));
        assert_eq!(q.price, Some(5175.0));
        assert_eq!(q.change, Some(-900.0));
        assert_eq!(q.change_pct, Some(-14.81));
        assert_eq!(q.previous_close, Some(6075.0));
        assert_eq!(q.open, Some(5825.0));
        assert_eq!(q.day_high, Some(5825.0));
        assert_eq!(q.day_low, Some(5175.0));
        assert_eq!(q.volume, Some(1_241_098_200));
        assert_eq!(q.currency.as_deref(), Some("IDR"));
    }

    #[test]
    fn index_quote_from_orderbook() {
        let q = parse_quote(&sym("^JKSE"), ORDERBOOK_IHSG).unwrap();
        assert!((q.price.unwrap() - 5185.105).abs() < 1e-3);
        assert_eq!(q.previous_close, Some(5863.78));
    }

    #[test]
    fn full_orderbook_depth_and_foreign() {
        let ob = parse_orderbook(&sym("BBCA"), ORDERBOOK).unwrap();
        assert!(ob.bids.is_empty(), "BBCA closed at ARB with no bids");
        assert_eq!(ob.asks.len(), 85);
        assert_eq!(ob.asks[0].price, 5175.0);
        assert_eq!(ob.asks[0].shares, 264_462_500);
        // The offer ladder sums to the reported total (shares).
        let sum: u64 = ob.asks.iter().map(|l| l.shares).sum();
        assert_eq!(sum, 331_908_100);
        assert_eq!(ob.foreign_net, Some(-2_329_493_087_500.0));
        assert_eq!(ob.last, Some(5175.0));
    }

    #[test]
    fn not_found_detected() {
        assert!(is_not_found(NOT_FOUND));
        assert!(!is_not_found(ORDERBOOK));
        assert!(parse_quote(&sym("ZZZZ"), NOT_FOUND).is_err());
    }

    #[test]
    fn daily_candles() {
        let h = parse_history(&sym("BBCA"), Period::M1, Interval::Day1, CHART_1M).unwrap();
        assert_eq!(h.candles.len(), 23);
        let last = h.candles.last().unwrap();
        assert_eq!(last.close, 5175.0);
        assert_eq!(last.open, 5825.0);
        // 12,410,982 lots -> shares.
        assert_eq!(last.volume, 1_241_098_200);
        assert_eq!(last.date().to_string(), "2026-10-16");
        // Oldest first.
        assert!(h.candles[0].time < last.time);
    }

    #[test]
    fn hourly_candles_keep_the_clock() {
        let h = parse_history(&sym("BBCA"), Period::D5, Interval::Hour1, CHART_1W).unwrap();
        assert_eq!(h.candles.len(), 34);
        let first = &h.candles[0];
        assert_eq!(
            first.time.format("%Y-%m-%d %H:%M").to_string(),
            "2026-10-12 09:00"
        );
    }

    #[test]
    fn yearly_series_summarises() {
        let h = parse_history(&sym("BBCA"), Period::Y1, Interval::Day1, CHART_1Y).unwrap();
        assert_eq!(h.candles.len(), 240);
        let s = h.summary.unwrap();
        assert_eq!(s.candles, 240);
    }

    #[test]
    fn empty_chart_is_no_candles_not_an_error() {
        let h = parse_history(&sym("BBCA"), Period::D1, Interval::Day1, CHART_EMPTY).unwrap();
        assert!(h.candles.is_empty());
        assert!(h.summary.is_none());
    }

    #[test]
    fn timeframe_mapping() {
        assert_eq!(timeframe_for(Period::M1, Interval::Day1), ("1m", false));
        assert_eq!(timeframe_for(Period::D5, Interval::Day1), ("1w", true));
        assert_eq!(timeframe_for(Period::Y5, Interval::Day1).0, "5y");
        assert_eq!(timeframe_for(Period::Y1, Interval::Hour1), ("1w", true));
        assert_eq!(timeframe_for(Period::D1, Interval::Min5), ("1d", false));
    }

    // The two fixtures below are CONSTRUCTED from the documented route shape,
    // not recorded from a live call (see tests/fixtures/stockbit/README.md).
    // They pin the parser's tolerance — envelope walking, dropped rows, unit
    // pass-through — and say nothing about whether Stockbit still sends these
    // keys. Replace them with a real capture before trusting the numbers.
    const HOTLIST_GAINERS: &str =
        include_str!("../../../tests/fixtures/stockbit/hotlist_gainers_constructed.json");
    const HOTLIST_LOSERS: &str =
        include_str!("../../../tests/fixtures/stockbit/hotlist_losers_constructed.json");
    const RUNNING_TRADE: &str =
        include_str!("../../../tests/fixtures/stockbit/running_trade_constructed.json");

    #[test]
    fn movers_read_a_bare_data_array() {
        let m = parse_movers(MoverKind::Gainer, HOTLIST_GAINERS).unwrap();
        assert_eq!(m.kind, MoverKind::Gainer);
        // The row with no symbol is dropped, not guessed at.
        assert_eq!(m.rows.len(), 2);
        let first = &m.rows[0];
        assert_eq!(first.symbol.as_str(), "DMMX");
        assert_eq!(first.last, Some(172.0));
        assert_eq!(first.change, Some(32.0));
        assert_eq!(first.change_pct, Some(22.857));
        assert_eq!(first.volume, Some(4_187_300));
        assert_eq!(first.name.as_deref(), Some("Digital Mediatama Maxima Tbk."));
    }

    #[test]
    fn movers_read_a_wrapped_data_result() {
        let m = parse_movers(MoverKind::Loser, HOTLIST_LOSERS).unwrap();
        assert_eq!(m.rows.len(), 2);
        assert_eq!(m.rows[0].symbol.as_str(), "GOTO");
        assert_eq!(m.rows[0].change_pct, Some(-10.0));
    }

    #[test]
    fn movers_accept_an_empty_list_after_hours() {
        let m = parse_movers(MoverKind::Gainer, r#"{"data":[]}"#).unwrap();
        assert!(m.rows.is_empty());
        assert!(parse_movers(MoverKind::Gainer, r#"{"message":"ok"}"#).is_err());
    }

    #[test]
    fn running_trade_reads_prints_sides_and_tags() {
        let rt = parse_running_trade(&sym("BBCA"), RUNNING_TRADE).unwrap();
        assert_eq!(rt.symbol.as_str(), "BBCA");
        assert_eq!(rt.trades.len(), 3);
        let sell = &rt.trades[0];
        assert_eq!(sell.price, 5175.0);
        assert_eq!(sell.volume, 2644);
        assert_eq!(sell.side, TradeSide::Sell);
        assert_eq!(sell.buyer.as_deref(), Some("BB"));
        assert_eq!(sell.seller.as_deref(), Some("ZP"));
        assert_eq!(
            sell.time.unwrap().format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-10-16 15:49:59"
        );
        assert_eq!(rt.trades[1].side, TradeSide::Buy);
        // A print with no side is kept, flagged unknown.
        assert_eq!(rt.trades[2].side, TradeSide::Unknown);
        assert!(rt.trades[2].time.is_none());
    }

    #[test]
    fn trade_side_spellings() {
        assert_eq!(side_of("B"), TradeSide::Buy);
        assert_eq!(side_of("buy"), TradeSide::Buy);
        assert_eq!(side_of("action_type_sell"), TradeSide::Sell);
        assert_eq!(side_of("side_sell"), TradeSide::Sell);
        assert_eq!(side_of(""), TradeSide::Unknown);
        assert_eq!(side_of("UNSPECIFIED"), TradeSide::Unknown);
    }

    #[test]
    fn trade_time_spellings() {
        let ts = |raw: &str| {
            trade_time(&serde_json::json!({ "time": raw }))
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        };
        assert_eq!(
            ts("2026-10-16 09:15:30").as_deref(),
            Some("2026-10-16 09:15")
        );
        assert_eq!(
            ts("2026-10-16T09:15:30+07:00").as_deref(),
            Some("2026-10-16 09:15")
        );
        // A bare clock lands on today, in WIB.
        let today = wib_from_unix(chrono::Utc::now().timestamp()).unwrap();
        let expect = format!("{} 09:15", today.format("%Y-%m-%d"));
        assert_eq!(ts("09:15:30").as_deref(), Some(expect.as_str()));
        assert_eq!(ts("nonsense"), None);
        assert_eq!(trade_time(&serde_json::json!({})), None);
    }
}
