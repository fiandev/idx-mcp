//! Drawing for `idx stockbit monitor`. Every pane is a `Paragraph` of styled
//! lines except the chart, which is a braille `Canvas` so a price can land on a
//! fraction of a cell.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::model::monitor::{Mover, Trade, TradeSide};
use crate::model::Candle;
use crate::output::format;

use super::app::{App, Panel};

/// Terminal green, red and the muted grey for secondary text.
fn up(app: &App) -> Style {
    if app.color {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    }
}

fn down(app: &App) -> Style {
    if app.color {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    }
}

fn muted(app: &App) -> Style {
    if app.color {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}

fn accent(app: &App) -> Style {
    if app.color {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

fn sign_style(app: &App, v: Option<f64>) -> Style {
    match v {
        Some(x) if x > 0.0 => up(app),
        Some(x) if x < 0.0 => down(app),
        _ => Style::default(),
    }
}

fn block(app: &App, title: &str, active: bool) -> Block<'static> {
    let style = if active { accent(app) } else { muted(app) };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style)
        .title(Span::styled(format!(" {title} "), style))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Percentage(38),
            Constraint::Length(1),
        ])
        .split(area);

    header(f, app, rows[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(rows[1]);
    chart(f, app, cols[0]);
    movers(f, app, cols[1]);

    tape(f, app, rows[2]);
    footer(f, app, rows[3]);

    if app.help {
        help(f, app, area);
    }
}

fn header(f: &mut Frame, app: &App, area: Rect) {
    let book = app.book.as_ref();
    let mut line1: Vec<Span> = vec![
        Span::styled(
            app.symbol.as_str().to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];
    if let Some(name) = book.and_then(|b| b.name.clone()) {
        line1.push(Span::styled(name, muted(app)));
        line1.push(Span::raw("  "));
    }
    let last = book.and_then(|b| b.last);
    let pct = book.and_then(|b| b.change_pct);
    let style = sign_style(app, pct);
    line1.push(Span::styled(
        format::price(last),
        style.add_modifier(Modifier::BOLD),
    ));
    line1.push(Span::styled(
        format!(
            "  {} ({})",
            format::change(book.and_then(|b| b.change)),
            format::pct(pct)
        ),
        style,
    ));

    let mut line2: Vec<Span> = Vec::new();
    if let Some(b) = book {
        let mut bits: Vec<String> = Vec::new();
        if let (Some(h), Some(l)) = (b.day_high, b.day_low) {
            bits.push(format!(
                "H {} L {}",
                format::price(Some(h)),
                format::price(Some(l))
            ));
        }
        if let Some(v) = b.volume {
            bits.push(format!("vol {}", format::int(Some(v))));
        }
        if let Some(net) = b.foreign_net {
            bits.push(format!(
                "foreign {}{}",
                if net >= 0.0 { "+" } else { "-" },
                format::compact(Some(net.abs()))
            ));
        }
        line2.push(Span::styled(bits.join("   "), muted(app)));
    }
    let clock = match app.updated {
        Some(t) => format!("  ·  {}", t.format("%H:%M:%S")),
        None => "  ·  loading".to_string(),
    };
    let note = match &app.chart_note {
        Some(n) => format!("   chart {n}"),
        None => String::new(),
    };
    line2.push(Span::styled(
        format!("{note}   every {}s{clock}", app.interval_secs),
        muted(app),
    ));
    if !app.warnings.is_empty() {
        let chip = format!(" {} warning(s) ", app.warnings.len());
        line2.push(Span::styled(
            chip,
            if app.color {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().add_modifier(Modifier::REVERSED)
            },
        ));
    }

    f.render_widget(
        Paragraph::new(vec![Line::from(line1), Line::from(line2)]).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(muted(app))
                .title(Span::styled(" idx · stockbit monitor ", accent(app))),
        ),
        area,
    );
}

fn chart(f: &mut Frame, app: &App, area: Rect) {
    let title = match (&app.chart_note, app.candles.is_empty()) {
        (_, true) => " Chart ".to_string(),
        (None, false) => format!(" Chart · 1m · {} ", range_label(&app.candles)),
        (Some(n), false) => format!(" Chart · {n} · {} ", range_label(&app.candles)),
    };
    let block = block(app, &title, false);

    if app.candles.is_empty() {
        let text = if app.updated.is_some() {
            "no candles for this window — the market may be closed"
        } else {
            "loading…"
        };
        f.render_widget(
            Paragraph::new(text)
                .style(muted(app))
                .alignment(Alignment::Center)
                .block(block),
            area,
        );
        return;
    }

    let (lo, hi) = price_bounds(&app.candles);
    let n = app.candles.len() as f64;
    let canvas = Canvas::default()
        .block(block)
        .marker(Marker::Braille)
        .x_bounds([0.0, n.max(2.0)])
        .y_bounds([lo, hi])
        .paint(|ctx| {
            for (i, c) in app.candles.iter().enumerate() {
                let x = i as f64 + 0.5;
                let style = if c.close >= c.open {
                    up(app)
                } else {
                    down(app)
                };
                // High-low wick first, then a filled body over it.
                ctx.draw(&CanvasLine {
                    x1: x,
                    y1: c.low,
                    x2: x,
                    y2: c.high,
                    color: if app.color {
                        Color::DarkGray
                    } else {
                        Color::Reset
                    },
                });
                ctx.draw(&CanvasLine {
                    x1: x - 0.32,
                    y1: c.open,
                    x2: x + 0.32,
                    y2: c.open,
                    color: style.fg.unwrap_or(Color::Reset),
                });
                ctx.draw(&CanvasLine {
                    x1: x - 0.32,
                    y1: c.close,
                    x2: x + 0.32,
                    y2: c.close,
                    color: style.fg.unwrap_or(Color::Reset),
                });
                if (c.open - c.close).abs() > f64::EPSILON {
                    ctx.draw(&CanvasLine {
                        x1: x,
                        y1: c.open.min(c.close),
                        x2: x,
                        y2: c.open.max(c.close),
                        color: style.fg.unwrap_or(Color::Reset),
                    });
                }
            }
        });
    f.render_widget(canvas, area);
}

/// The chart has no axis, so its price range goes in the block title.
fn range_label(candles: &[Candle]) -> String {
    let (lo, hi) = price_bounds(candles);
    format!("{}–{}", format::price(Some(lo)), format::price(Some(hi)))
}

/// The chart's y range with a little air above and below, so a wick is never
/// glued to the frame. A flat series still gets a non-zero span.
fn price_bounds(candles: &[Candle]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for c in candles {
        lo = lo.min(c.low);
        hi = hi.max(c.high);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    let span = (hi - lo).max(hi.abs() * 0.01).max(1.0);
    (lo - span * 0.05, hi + span * 0.05)
}

fn movers(f: &mut Frame, app: &App, area: Rect) {
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let selected = app.sel;
    mover_list(f, app, split[0], " Top gainer ", &app.gainers, 0, selected);
    mover_list(
        f,
        app,
        split[1],
        " Top loser ",
        &app.losers,
        app.gainers.len(),
        selected,
    );
}

fn mover_list(
    f: &mut Frame,
    app: &App,
    area: Rect,
    title: &str,
    rows: &[Mover],
    offset: usize,
    selected: usize,
) {
    let active = app.panel == Panel::Movers;
    let mut lines: Vec<Line> = Vec::with_capacity(rows.len());
    for (i, m) in rows.iter().enumerate() {
        let picked = offset + i == selected;
        let style = sign_style(app, m.change_pct);
        let marker = if picked && active { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{:<6}", m.symbol.as_str()), {
                let mut s = style;
                if picked {
                    s = s.bg(if app.color {
                        Color::DarkGray
                    } else {
                        Color::Reset
                    });
                    s = s.add_modifier(Modifier::BOLD);
                }
                s
            }),
            Span::styled(format!("{:>9}", format::price(m.last)), style),
            Span::styled(format!(" {:>8}", format::pct(m.change_pct)), style),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("  —", muted(app))));
    }
    let inner = usize::from(area.height.saturating_sub(2));
    let scroll = if selected >= offset + inner && inner > 0 {
        (selected - offset + 1).saturating_sub(inner)
    } else {
        0
    };
    f.render_widget(
        Paragraph::new(lines)
            .block(block(app, title, active))
            .scroll((scroll as u16, 0)),
        area,
    );
}

fn tape(f: &mut Frame, app: &App, area: Rect) {
    let active = app.panel == Panel::Tape;
    let mut lines: Vec<Line> = Vec::with_capacity(app.trades.len() + 1);
    lines.push(Line::from(Span::styled(
        format!(
            "  {:<8}{:>9}{:>10}  {:<2} {}",
            "time", "price", "volume", "b/s", "brokers"
        ),
        muted(app),
    )));
    for t in app.trades.iter().skip(app.tape_scroll) {
        lines.push(trade_line(app, t));
    }
    if app.trades.is_empty() {
        lines.push(Line::from(Span::styled(
            if app.updated.is_some() {
                "  no prints — outside the session, or nothing traded yet"
            } else {
                "  loading…"
            },
            muted(app),
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(block(
            app,
            &format!(" Running trade · {} · {} ", app.symbol, app.trades.len()),
            active,
        )),
        area,
    );
}

fn trade_line(app: &App, t: &Trade) -> Line<'static> {
    let side = match t.side {
        TradeSide::Buy => ("B", up(app)),
        TradeSide::Sell => ("S", down(app)),
        TradeSide::Unknown => ("?", muted(app)),
    };
    let time = t
        .time
        .map(|x| x.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| format::DASH.to_string());
    let brokers = match (&t.buyer, &t.seller) {
        (Some(b), Some(s)) => format!("{b} ↔ {s}"),
        (Some(b), None) => format!("buyer {b}"),
        (None, Some(s)) => format!("seller {s}"),
        (None, None) => String::new(),
    };
    Line::from(vec![
        Span::styled(format!("  {time:<8}"), muted(app)),
        Span::styled(
            format!("{:>9}", format::price(Some(t.price))),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:>10}", format::int(Some(t.volume))),
            Style::default(),
        ),
        Span::raw("  "),
        Span::styled(format!("{:<2}", side.0), side.1),
        Span::raw(" "),
        Span::styled(brokers, muted(app)),
    ])
}

fn footer(f: &mut Frame, app: &App, area: Rect) {
    let keys = if app.help {
        "q quit · esc/? close"
    } else {
        "q quit · tab pane · ↑↓ select/scroll · enter focus · r refresh · +/- every Ns · ? help"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(keys, muted(app)))),
        area,
    );
}

fn help(f: &mut Frame, app: &App, area: Rect) {
    let w = 66.min(area.width.saturating_sub(4));
    let h = 24.min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Realtime from your own Stockbit session.",
            muted(app),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("↑ ↓  j k   ", accent(app)),
            Span::raw("move in the mover lists, or scroll the tape"),
        ]),
        Line::from(vec![
            Span::styled("tab  h l   ", accent(app)),
            Span::raw("switch which pane the arrows drive"),
        ]),
        Line::from(vec![
            Span::styled("enter      ", accent(app)),
            Span::raw("load the selected mover's chart and tape"),
        ]),
        Line::from(vec![
            Span::styled("pgup/pgdn  ", accent(app)),
            Span::raw("scroll the tape by ten prints"),
        ]),
        Line::from(vec![
            Span::styled("g / G      ", accent(app)),
            Span::raw("jump to the first / last row"),
        ]),
        Line::from(vec![
            Span::styled("r          ", accent(app)),
            Span::raw("refresh now"),
        ]),
        Line::from(vec![
            Span::styled("+ / -      ", accent(app)),
            Span::raw("slower / faster polling (1..120 s)"),
        ]),
        Line::from(vec![
            Span::styled("? / esc    ", accent(app)),
            Span::raw("close this sheet"),
        ]),
        Line::from(vec![
            Span::styled("q ctrl+c   ", accent(app)),
            Span::raw("quit"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "Chart: {} · tape: {} prints",
                if app.candles.is_empty() {
                    "none".to_string()
                } else {
                    format!("{} candles", app.candles.len())
                },
                app.trades.len()
            ),
            muted(app),
        )),
        Line::from(Span::styled(
            "Sizes are the numbers Stockbit sends; prices IDR.",
            muted(app),
        )),
    ];
    if !app.warnings.is_empty() {
        lines.push(Line::from(""));
        for w in app.warnings.iter().take(3) {
            lines.push(Line::from(Span::styled(
                format!("! {w}"),
                Style::default().fg(Color::Yellow),
            )));
        }
        if app.warnings.len() > 3 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", app.warnings.len() - 3),
                muted(app),
            )));
        }
    }
    while lines.len() < h.saturating_sub(2) as usize {
        lines.push(Line::from(""));
    }

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block(app, "Keys", true)),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::monitor::TradeSide;
    use crate::model::orderbook::{BookLevel, OrderBook};
    use crate::model::{wib_from_unix, Symbol};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn candle(hour: i64, open: f64, close: f64) -> Candle {
        Candle {
            time: wib_from_unix(1_792_140_000 + hour * 3600).unwrap(),
            open,
            high: open.max(close) + 15.0,
            low: open.min(close) - 15.0,
            close,
            adj_close: None,
            volume: 1_000,
        }
    }

    fn mover(sym: &str, pct: f64) -> Mover {
        Mover {
            symbol: Symbol::parse(sym).unwrap(),
            name: None,
            last: Some(5_175.0),
            change: Some(125.0),
            change_pct: Some(pct),
            volume: Some(1_241_098_200),
            value: None,
        }
    }

    fn app() -> App {
        let mut a = App::new(Symbol::parse("BBCA").unwrap(), 5, true);
        a.book = Some(OrderBook {
            symbol: Symbol::parse("BBCA").unwrap(),
            name: Some("Bank Central Asia Tbk.".into()),
            last: Some(5_175.0),
            change: Some(125.0),
            change_pct: Some(2.48),
            previous_close: Some(5_050.0),
            open: Some(5_100.0),
            day_high: Some(5_225.0),
            day_low: Some(5_050.0),
            volume: Some(1_241_098_200),
            value: None,
            bids: vec![BookLevel {
                price: 5_175.0,
                shares: 100,
                orders: 1,
            }],
            asks: vec![],
            foreign_buy: None,
            foreign_sell: None,
            foreign_net: Some(-2_329_493_087_500.0),
        });
        a.gainers = vec![mover("DMMX", 22.86), mover("BBCA", 2.48)];
        a.losers = vec![mover("GOTO", -10.0)];
        a.trades = vec![
            Trade {
                time: wib_from_unix(1_792_140_000).unwrap().into(),
                price: 5_175.0,
                volume: 2_644,
                side: TradeSide::Sell,
                buyer: Some("BB".into()),
                seller: Some("ZP".into()),
            },
            Trade {
                time: None,
                price: 5_175.0,
                volume: 100,
                side: TradeSide::Buy,
                buyer: None,
                seller: None,
            },
        ];
        a.candles = (0..30).map(|i| candle(i, 5_100.0, 5_175.0)).collect();
        a.warnings = vec!["running trade: outside the session".into()];
        a
    }

    fn render(app: &mut App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_board_names_the_ticker_price_and_both_mover_lists() {
        let mut app = app();
        let screen = render(&mut app, 120, 40);
        assert!(screen.contains("stockbit monitor"), "{screen}");
        assert!(screen.contains("BBCA"));
        assert!(screen.contains("5,175"));
        assert!(screen.contains("+2.48%"));
        assert!(screen.contains("Top gainer"));
        assert!(screen.contains("Top loser"));
        assert!(screen.contains("DMMX"));
        assert!(screen.contains("GOTO"));
        assert!(screen.contains("Running trade"));
        // The tape keeps an unknown side visible instead of dropping the print.
        assert!(screen.contains("BB ↔ ZP"));
        assert!(screen.contains("1 warning(s)"));
    }

    #[test]
    fn an_empty_board_still_draws_and_says_why() {
        let mut app = App::new(Symbol::parse("BBCA").unwrap(), 5, false);
        let screen = render(&mut app, 100, 30);
        assert!(screen.contains("loading"));
        assert!(screen.contains("no candles") || screen.contains("loading"));
    }

    #[test]
    fn a_tiny_terminal_does_not_panic() {
        let mut app = app();
        for (w, h) in [(1, 1), (12, 6), (40, 12), (200, 60)] {
            let _ = render(&mut app, w, h);
        }
    }

    #[test]
    fn the_help_sheet_lists_the_keys() {
        let mut app = app();
        app.help = true;
        let screen = render(&mut app, 120, 40);
        assert!(screen.contains("Keys"));
        assert!(screen.contains("refresh now"));
        assert!(screen.contains("quit"));
    }

    #[test]
    fn chart_bounds_keep_a_flat_series_readable() {
        let flat = vec![candle(0, 100.0, 100.0)];
        let (lo, hi) = price_bounds(&flat);
        assert!(hi > lo, "{lo}..{hi}");
        assert!(lo < 100.0 && hi > 100.0);
        assert_eq!(price_bounds(&[]), (0.0, 1.0));
    }
}
