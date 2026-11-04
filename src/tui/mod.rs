//! `idx stockbit monitor`: the live tape in the terminal — a candle chart, the
//! top gainers and losers, and the running trade of the focused ticker, all
//! refreshed on a clock.
//!
//! This is the one part of `idx` that is neither a table nor JSON: it owns the
//! screen, so it writes nothing to stdout and refuses to run without a terminal.
//! Everything it shows comes from Stockbit (see `providers::stockbit`), which is
//! why it lives under `idx stockbit` rather than at the top level.

pub mod app;
pub mod ui;

use std::io::IsTerminal;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use ratatui::crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use ratatui::DefaultTerminal;
use tokio::sync::{mpsc, watch};

use crate::error::{IdxError, Result};
use crate::model::monitor::MoverKind;
use crate::model::{Interval, Period, Symbol};
use crate::providers::stockbit::Stockbit;
use crate::providers::{Provider, Sourced};

use app::{App, Snapshot};

/// How the monitor was asked to run.
#[derive(Debug, Clone)]
pub struct MonitorOptions {
    /// The ticker the chart and the tape start on.
    pub symbol: Symbol,
    /// Rows per mover list.
    pub top: usize,
    /// Prints in the tape.
    pub tape: usize,
    /// Seconds between refreshes.
    pub interval_secs: u64,
    /// Honour `--no-color` / `NO_COLOR`; the terminal keeps its own palette
    /// otherwise.
    pub color: bool,
}

impl MonitorOptions {
    pub const DEFAULT_INTERVAL_SECS: u64 = 5;
}

/// A fetch the background task should do now.
#[derive(Debug, Clone)]
struct Request {
    symbol: Symbol,
}

/// Runs the monitor until the user quits. `--offline` works as everywhere else:
/// the panes fill from whatever Stockbit responses are still cached.
pub async fn run(sb: Stockbit, opts: MonitorOptions) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return Err(IdxError::Usage(
            "stockbit monitor draws a live terminal interface; run it in a terminal, or use \
             `idx stockbit orderbook` / `idx --provider stockbit stocks quote` for plain output"
                .into(),
        ));
    }

    let mut app = App::new(opts.symbol.clone(), opts.interval_secs, opts.color);
    let interval = Arc::new(AtomicU64::new(opts.interval_secs.max(1)));
    let (req_tx, req_rx) = watch::channel(Request {
        symbol: opts.symbol.clone(),
    });
    let (snap_tx, mut snap_rx) = mpsc::channel::<Snapshot>(2);
    let worker = tokio::spawn(fetch_loop(
        Arc::new(sb),
        req_rx,
        interval.clone(),
        opts.clone(),
        snap_tx,
    ));

    let _screen = Screen;
    let mut terminal = ratatui::init();
    let outcome = event_loop(&mut app, &mut terminal, &mut snap_rx, &req_tx, &interval).await;
    drop(terminal);
    worker.abort();
    outcome
}

/// Puts the alternate screen and the terminal modes back on drop — including on
/// the error paths, so a failed run never leaves the terminal unusable.
struct Screen;

impl Drop for Screen {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

async fn event_loop(
    app: &mut App,
    terminal: &mut DefaultTerminal,
    snap_rx: &mut mpsc::Receiver<Snapshot>,
    req_tx: &watch::Sender<Request>,
    interval: &AtomicU64,
) -> Result<()> {
    let mut events = EventStream::new();

    // Paint the frame once before waiting on the network, so the board is there
    // immediately instead of a blank alternate screen for the first second.
    terminal
        .draw(|f| ui::draw(f, app))
        .map_err(|e| IdxError::Other(format!("could not draw: {e}")))?;

    loop {
        tokio::select! {
            biased;
            maybe = events.next() => match maybe {
                Some(Ok(Event::Key(key))) => {
                    // Raw mode delivers Ctrl+C as a key, not a signal.
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c'))
                    {
                        app.quit = true;
                    } else {
                        app.on_key(key.code, key.kind);
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    return Err(IdxError::Other(format!("terminal input failed: {e}")));
                }
                None => break,
            },
            Some(snap) = snap_rx.recv() => app.apply(snap),
        }

        interval.store(app.interval_secs.max(1), Ordering::Relaxed);
        let refreshed = app.take_refresh();
        let picked = app.take_pick().is_some();
        if refreshed || picked {
            let _ = req_tx.send(Request {
                symbol: app.symbol.clone(),
            });
        }
        if app.quit {
            break;
        }

        terminal
            .draw(|f| ui::draw(f, app))
            .map_err(|e| IdxError::Other(format!("could not draw: {e}")))?;
    }
    Ok(())
}

/// Polls Stockbit on the monitor's clock and hands each pass to the UI. It never
/// returns an error: a failed pane becomes a warning line, and the screen keeps
/// showing the last good data.
async fn fetch_loop(
    sb: Arc<Stockbit>,
    mut req: watch::Receiver<Request>,
    interval: Arc<AtomicU64>,
    opts: MonitorOptions,
    tx: mpsc::Sender<Snapshot>,
) {
    loop {
        let symbol = req.borrow().symbol.clone();
        let snap = snapshot(&sb, &symbol, &opts).await;
        if tx.send(snap).await.is_err() {
            return;
        }
        if !sleep_until_next(&mut req, &interval).await {
            return;
        }
    }
}

/// Waits out the interval in short slices: a new ticker starts a fetch at once,
/// and lowering the interval shortens the wait already in progress. Raising it
/// applies from the next pass, so a burst of `+` never starves the screen.
/// Returns `false` when the UI is gone.
async fn sleep_until_next(req: &mut watch::Receiver<Request>, interval: &AtomicU64) -> bool {
    let start = Instant::now();
    let mut deadline = start + Duration::from_secs(interval.load(Ordering::Relaxed).max(1));
    loop {
        let now = Instant::now();
        let wanted = start + Duration::from_secs(interval.load(Ordering::Relaxed).max(1));
        deadline = deadline.min(wanted);
        if now >= deadline {
            return true;
        }
        let slice = (deadline - now).min(Duration::from_millis(400));
        tokio::select! {
            _ = tokio::time::sleep(slice) => {}
            changed = req.changed() => {
                if changed.is_err() {
                    return false;
                }
                return true;
            }
        }
    }
}

/// One pass: every pane at once, each failure kept as a warning.
async fn snapshot(sb: &Stockbit, symbol: &Symbol, opts: &MonitorOptions) -> Snapshot {
    let (gainers, losers, tape, book) = tokio::join!(
        sb.movers(MoverKind::Gainer, opts.top),
        sb.movers(MoverKind::Loser, opts.top),
        sb.running_trade(symbol, opts.tape),
        sb.orderbook(symbol),
    );

    let mut warnings = Vec::new();
    let gainers = unpack(gainers, "top gainers", &mut warnings)
        .map(|m| m.rows)
        .unwrap_or_default();
    let losers = unpack(losers, "top losers", &mut warnings)
        .map(|m| m.rows)
        .unwrap_or_default();
    let trades = unpack(tape, "running trade", &mut warnings)
        .map(|t| t.trades)
        .unwrap_or_default();
    let book = unpack(book, "quote", &mut warnings);

    // Today's one-minute bars; after the close Stockbit sends none, so fall back
    // to the hourly five-day window rather than leave the pane blank.
    let mut candles = match sb.history(symbol, Period::D1, Interval::Min5).await {
        Ok(h) => h.data.candles,
        Err(e) => {
            warnings.push(format!("chart: {e}"));
            Vec::new()
        }
    };
    let mut chart_note = None;
    if candles.is_empty() {
        match sb.history(symbol, Period::D5, Interval::Hour1).await {
            Ok(h) if !h.data.candles.is_empty() => {
                candles = h.data.candles;
                chart_note = Some("1h · 5d".to_string());
            }
            Ok(_) => {}
            Err(e) => warnings.push(format!("chart: {e}")),
        }
    }

    Snapshot {
        symbol: symbol.clone(),
        book,
        gainers,
        losers,
        trades,
        candles,
        chart_note,
        warnings,
        at: chrono::Local::now(),
    }
}

/// Fails soft: a pane whose call failed keeps its last value and the reason goes
/// in the banner. The source's own warnings ride along the same way.
fn unpack<T>(result: Result<Sourced<T>>, what: &str, warnings: &mut Vec<String>) -> Option<T> {
    match result {
        Ok(s) => {
            for w in s.warnings {
                warnings.push(format!("{what}: {w}"));
            }
            Some(s.data)
        }
        Err(e) => {
            warnings.push(format!("{what}: {e}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::monitor::Movers;

    fn movers() -> Movers {
        Movers {
            kind: MoverKind::Gainer,
            rows: vec![],
        }
    }

    #[test]
    fn unpack_keeps_the_data_and_folds_provider_warnings_into_the_banner() {
        let mut warnings = Vec::new();
        let mut s = Sourced::fresh(movers(), "stockbit");
        s.warnings.push("partial list".into());
        let data = unpack(Ok::<_, IdxError>(s), "top gainers", &mut warnings);
        assert!(data.is_some());
        assert_eq!(warnings, vec!["top gainers: partial list".to_string()]);
    }

    #[test]
    fn unpack_turns_a_failed_pane_into_a_warning_not_an_error() {
        let mut warnings = Vec::new();
        let data = unpack::<Movers>(
            Err(IdxError::Network("down".into())),
            "top losers",
            &mut warnings,
        );
        assert!(data.is_none());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("top losers: "));
    }
}
