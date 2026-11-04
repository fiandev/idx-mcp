//! State and key handling for `idx stockbit monitor`. No rendering here: that
//! is `ui.rs`. Keeping the two apart lets the state machine be read (and
//! tested) without a terminal.

use chrono::{DateTime, Local};

use crate::model::monitor::{Mover, Trade};
use crate::model::orderbook::OrderBook;
use crate::model::{Candle, Symbol};

/// Which pane the arrow keys drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Movers,
    Tape,
}

/// One pass over the sources, as the background task delivers it.
#[derive(Debug)]
pub struct Snapshot {
    pub symbol: Symbol,
    pub book: Option<OrderBook>,
    pub gainers: Vec<Mover>,
    pub losers: Vec<Mover>,
    pub trades: Vec<Trade>,
    pub candles: Vec<Candle>,
    /// How the chart window was obtained, when it is not the intraday default.
    pub chart_note: Option<String>,
    pub warnings: Vec<String>,
    pub at: DateTime<Local>,
}

/// What the user is looking at.
pub struct App {
    /// The ticker every pane is about.
    pub symbol: Symbol,
    pub gainers: Vec<Mover>,
    pub losers: Vec<Mover>,
    pub trades: Vec<Trade>,
    pub candles: Vec<Candle>,
    pub book: Option<OrderBook>,
    pub chart_note: Option<String>,
    pub warnings: Vec<String>,
    pub updated: Option<DateTime<Local>>,
    pub interval_secs: u64,
    pub panel: Panel,
    /// Selection across the two mover lists, gainers first.
    pub sel: usize,
    /// First tape row shown; the tape is scrolled, not selected.
    pub tape_scroll: usize,
    pub help: bool,
    pub quit: bool,
    /// The user asked for another ticker; the loop turns it into a fetch.
    pub pick: Option<Symbol>,
    /// The user asked for a refresh now.
    pub refresh: bool,
    /// Honour `--no-color` / `NO_COLOR`.
    pub color: bool,
}

impl App {
    pub fn new(symbol: Symbol, interval_secs: u64, color: bool) -> Self {
        App {
            symbol,
            gainers: Vec::new(),
            losers: Vec::new(),
            trades: Vec::new(),
            candles: Vec::new(),
            book: None,
            chart_note: None,
            warnings: Vec::new(),
            updated: None,
            interval_secs: interval_secs.max(1),
            panel: Panel::Movers,
            sel: 0,
            tape_scroll: 0,
            help: false,
            quit: false,
            pick: None,
            refresh: false,
            color,
        }
    }

    /// Every mover row, gainers then losers, with its position in the board.
    pub fn movers(&self) -> impl Iterator<Item = &Mover> {
        self.gainers.iter().chain(self.losers.iter())
    }

    pub fn mover_count(&self) -> usize {
        self.gainers.len() + self.losers.len()
    }

    /// The ticker the selection sits on.
    pub fn selected(&self) -> Option<&Mover> {
        self.movers().nth(self.sel)
    }

    pub fn apply(&mut self, snap: Snapshot) {
        self.symbol = snap.symbol;
        self.gainers = snap.gainers;
        self.losers = snap.losers;
        self.trades = snap.trades;
        self.candles = snap.candles;
        self.book = snap.book;
        self.chart_note = snap.chart_note;
        self.warnings = snap.warnings;
        self.updated = Some(snap.at);
        self.sel = self.sel.min(self.mover_count().saturating_sub(1));
        self.clamp_tape();
    }

    /// `rows` is how many tape lines the pane can show; the scroll position is
    /// kept inside `[0, len - rows]` so the list never scrolls past its end.
    pub fn clamp_tape(&mut self) {
        let max = self.trades.len().saturating_sub(1);
        self.tape_scroll = self.tape_scroll.min(max);
    }

    fn move_sel(&mut self, delta: isize) {
        let count = self.mover_count();
        if count == 0 {
            return;
        }
        let next = self.sel as isize + delta;
        self.sel = next.clamp(0, count as isize - 1) as usize;
    }

    fn scroll_tape(&mut self, delta: isize) {
        let next = self.tape_scroll as isize + delta;
        self.tape_scroll = next.max(0) as usize;
        self.clamp_tape();
    }

    fn bump_interval(&mut self, delta: i64) {
        let next = (self.interval_secs as i64 + delta).clamp(1, 120) as u64;
        self.interval_secs = next;
    }

    /// Takes the pending symbol, if the user picked one.
    pub fn take_pick(&mut self) -> Option<Symbol> {
        self.pick.take()
    }

    pub fn take_refresh(&mut self) -> bool {
        std::mem::take(&mut self.refresh)
    }
}

/// What a key does. Kept separate from [`App::on_key`] so the mapping can be
/// asserted in a test without a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    ToggleHelp,
    CloseHelp,
    NextPanel,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Select,
    Refresh,
    Faster,
    Slower,
    None,
}

/// `crossterm` keys to actions. `Enter` is `Select`, `Esc` closes the help
/// overlay if it is open and quits otherwise.
pub fn action_for(
    code: crossterm::event::KeyCode,
    kind: crossterm::event::KeyEventKind,
    help: bool,
) -> Action {
    use crossterm::event::KeyCode as K;
    if kind == crossterm::event::KeyEventKind::Release {
        return Action::None;
    }
    match code {
        K::Char('q') => Action::Quit,
        K::Esc => {
            if help {
                Action::CloseHelp
            } else {
                Action::Quit
            }
        }
        K::Char('?') => Action::ToggleHelp,
        K::Tab | K::BackTab | K::Char('h') | K::Char('l') => Action::NextPanel,
        K::Up | K::Char('k') => Action::Up,
        K::Down | K::Char('j') => Action::Down,
        K::PageUp => Action::PageUp,
        K::PageDown => Action::PageDown,
        K::Home | K::Char('g') => Action::Top,
        K::End | K::Char('G') => Action::Bottom,
        K::Enter => Action::Select,
        K::Char('r') => Action::Refresh,
        K::Char('+') | K::Char('=') => Action::Faster,
        K::Char('-') | K::Char('_') => Action::Slower,
        _ => Action::None,
    }
}

impl App {
    /// Applies one key press. Returns `true` when the app wants to exit.
    pub fn on_key(
        &mut self,
        code: crossterm::event::KeyCode,
        kind: crossterm::event::KeyEventKind,
    ) -> bool {
        let action = action_for(code, kind, self.help);
        // The help sheet is modal for navigation: anything but refresh dismisses
        // it, and only `q` leaves the app from inside it.
        if self.help {
            match action {
                Action::Quit => {
                    self.quit = true;
                    return true;
                }
                Action::Refresh => {
                    self.refresh = true;
                    return false;
                }
                _ => {
                    self.help = false;
                    return false;
                }
            }
        }
        match action {
            Action::Quit => self.quit = true,
            Action::ToggleHelp => self.help = !self.help,
            Action::CloseHelp => self.help = false,
            Action::NextPanel => {
                self.panel = match self.panel {
                    Panel::Movers => Panel::Tape,
                    Panel::Tape => Panel::Movers,
                }
            }
            Action::Up => match self.panel {
                Panel::Movers => self.move_sel(-1),
                Panel::Tape => self.scroll_tape(-1),
            },
            Action::Down => match self.panel {
                Panel::Movers => self.move_sel(1),
                Panel::Tape => self.scroll_tape(1),
            },
            Action::PageUp => self.scroll_tape(-10),
            Action::PageDown => self.scroll_tape(10),
            Action::Top => match self.panel {
                Panel::Movers => self.sel = 0,
                Panel::Tape => self.tape_scroll = 0,
            },
            Action::Bottom => match self.panel {
                Panel::Movers => self.sel = self.mover_count().saturating_sub(1),
                Panel::Tape => self.tape_scroll = self.trades.len().saturating_sub(1),
            },
            Action::Select => {
                if let Some(m) = self.selected() {
                    let symbol = m.symbol.clone();
                    if symbol != self.symbol {
                        self.symbol = symbol.clone();
                        self.pick = Some(symbol);
                        // The tape is about to be someone else's.
                        self.trades.clear();
                        self.candles.clear();
                        self.tape_scroll = 0;
                    }
                }
            }
            Action::Refresh => self.refresh = true,
            Action::Faster => self.bump_interval(1),
            Action::Slower => self.bump_interval(-1),
            Action::None => {}
        }
        self.quit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::monitor::TradeSide;
    use crossterm::event::{KeyCode, KeyEventKind};

    fn mover(sym: &str) -> Mover {
        Mover {
            symbol: Symbol::parse(sym).unwrap(),
            name: None,
            last: Some(100.0),
            change: Some(1.0),
            change_pct: Some(1.0),
            volume: None,
            value: None,
        }
    }

    fn app() -> App {
        let mut a = App::new(Symbol::parse("BBCA").unwrap(), 5, true);
        a.gainers = vec![mover("DMMX"), mover("GOTO")];
        a.losers = vec![mover("ASII")];
        a
    }

    const PRESS: KeyEventKind = KeyEventKind::Press;

    #[test]
    fn selection_walks_both_mover_lists_and_clamps() {
        let mut a = app();
        assert_eq!(a.sel, 0);
        a.on_key(KeyCode::Down, PRESS);
        a.on_key(KeyCode::Down, PRESS);
        assert_eq!(a.sel, 2);
        assert_eq!(a.selected().unwrap().symbol.as_str(), "ASII");
        // Past the end stays put instead of wrapping.
        a.on_key(KeyCode::Down, PRESS);
        assert_eq!(a.sel, 2);
        a.on_key(KeyCode::Up, PRESS);
        a.on_key(KeyCode::Up, PRESS);
        a.on_key(KeyCode::Up, PRESS);
        assert_eq!(a.sel, 0);
    }

    #[test]
    fn enter_picks_the_selected_ticker_and_clears_its_tape() {
        let mut a = app();
        a.trades.push(Trade {
            time: None,
            price: 100.0,
            volume: 1,
            side: TradeSide::Buy,
            buyer: None,
            seller: None,
        });
        a.on_key(KeyCode::Down, PRESS);
        a.on_key(KeyCode::Enter, PRESS);
        assert_eq!(a.take_pick().unwrap().as_str(), "GOTO");
        assert_eq!(a.symbol.as_str(), "GOTO");
        assert!(a.trades.is_empty());
        // Picking the ticker already shown is not a fetch.
        a.on_key(KeyCode::Enter, PRESS);
        assert!(a.take_pick().is_none());
    }

    #[test]
    fn tab_moves_the_arrows_between_panels() {
        let mut a = app();
        assert_eq!(a.panel, Panel::Movers);
        a.on_key(KeyCode::Tab, PRESS);
        assert_eq!(a.panel, Panel::Tape);
        // Down now scrolls the tape, leaving the selection alone.
        a.on_key(KeyCode::Down, PRESS);
        assert_eq!(a.sel, 0);
        a.on_key(KeyCode::Tab, PRESS);
        assert_eq!(a.panel, Panel::Movers);
    }

    #[test]
    fn tape_scroll_is_clamped_to_the_last_print() {
        let mut a = app();
        a.panel = Panel::Tape;
        a.trades = (0..3)
            .map(|i| Trade {
                time: None,
                price: 100.0 + i as f64,
                volume: 1,
                side: TradeSide::Buy,
                buyer: None,
                seller: None,
            })
            .collect();
        for _ in 0..10 {
            a.on_key(KeyCode::Down, PRESS);
        }
        assert_eq!(a.tape_scroll, 2);
        a.on_key(KeyCode::Home, PRESS);
        assert_eq!(a.tape_scroll, 0);
    }

    #[test]
    fn interval_stays_in_range() {
        let mut a = App::new(Symbol::parse("BBCA").unwrap(), 1, true);
        a.on_key(KeyCode::Char('-'), PRESS);
        assert_eq!(a.interval_secs, 1);
        for _ in 0..200 {
            a.on_key(KeyCode::Char('+'), PRESS);
        }
        assert_eq!(a.interval_secs, 120);
        a.on_key(KeyCode::Char('-'), PRESS);
        assert_eq!(a.interval_secs, 119);
    }

    #[test]
    fn escape_closes_help_before_quitting() {
        let mut a = app();
        a.on_key(KeyCode::Char('?'), PRESS);
        assert!(a.help);
        a.on_key(KeyCode::Esc, PRESS);
        assert!(!a.help);
        assert!(!a.quit);
        a.on_key(KeyCode::Esc, PRESS);
        assert!(a.quit);
    }

    #[test]
    fn q_quits_from_the_help_sheet_too() {
        let mut a = app();
        a.on_key(KeyCode::Char('?'), PRESS);
        assert!(a.on_key(KeyCode::Char('q'), PRESS));
        assert!(a.quit);
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut a = app();
        a.on_key(KeyCode::Down, KeyEventKind::Release);
        assert_eq!(a.sel, 0);
    }

    #[test]
    fn apply_keeps_the_selection_inside_the_new_lists() {
        let mut a = app();
        a.sel = 2;
        a.apply(Snapshot {
            symbol: Symbol::parse("BBCA").unwrap(),
            book: None,
            gainers: vec![mover("AAAA")],
            losers: vec![],
            trades: vec![],
            candles: vec![],
            chart_note: None,
            warnings: vec![],
            at: Local::now(),
        });
        assert_eq!(a.sel, 0);
        assert_eq!(a.selected().unwrap().symbol.as_str(), "AAAA");
    }

    #[test]
    fn actions_cover_the_documented_keys() {
        use KeyCode as K;
        assert_eq!(action_for(K::Char('q'), PRESS, false), Action::Quit);
        assert_eq!(action_for(K::Tab, PRESS, false), Action::NextPanel);
        assert_eq!(action_for(K::Char('r'), PRESS, false), Action::Refresh);
        assert_eq!(action_for(K::Char('?'), PRESS, false), Action::ToggleHelp);
        assert_eq!(action_for(K::Char('x'), PRESS, false), Action::None);
    }
}
