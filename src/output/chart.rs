//! Bar charts for the numeric `stocks` commands. Table mode only: with
//! `--chart bar|stacked` the table is replaced by a chart drawn with block
//! characters; `-o json` and `-o csv` are untouched.
//!
//! Every command only describes *what* to plot, as a [`Chart`] of labels and
//! series; the layout (vertical/horizontal, grouped/stacked) is decided here.

use crate::analysis::screen::Key;
use crate::model::{
    DividendHistory, Earnings, Financials, ForeignFlow, Fundamentals, History, Performance, Quote,
    ReturnWindows, ScreenReport, StatementPeriod,
};
use crate::output::format as f;

/// How the numbers are printed on the axis, bar labels and legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// IDR price: whole rupiah unless the value has cents.
    Price,
    /// Signed percentage.
    Percent,
    /// Plain number, two decimals.
    Number,
    /// Compact magnitude: 1.23B, 4.5T.
    Compact,
    /// Ratio, two decimals.
    Ratio,
}

fn format_value(unit: Unit, v: f64) -> String {
    match unit {
        Unit::Price => f::price(Some(v)),
        Unit::Percent => f::pct(Some(v)),
        Unit::Number => f::num(v, 2),
        Unit::Compact => f::compact(Some(v)),
        Unit::Ratio => f::ratio(Some(v)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ChartKind {
    /// One bar per series at each label.
    Bar,
    /// The series summed into one bar at each label.
    Stacked,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ChartDirection {
    #[default]
    Vertical,
    Horizontal,
}

/// The `--chart` / `--direction` pair every numeric `stocks` command accepts.
#[derive(Debug, Clone, clap::Args)]
pub struct ChartArgs {
    /// Draw a chart instead of the table: `bar` groups the series, `stacked` sums them.
    #[arg(long, value_enum, value_name = "KIND")]
    pub chart: Option<ChartKind>,
    /// Chart orientation.
    #[arg(long, value_enum, default_value = "vertical", value_name = "DIR")]
    pub direction: ChartDirection,
}

impl ChartArgs {
    /// Whether the user asked for a chart.
    pub fn is_on(&self) -> bool {
        self.chart.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct ChartSeries {
    pub name: String,
    pub values: Vec<Option<f64>>,
}

impl ChartSeries {
    pub fn new(name: impl Into<String>, values: Vec<Option<f64>>) -> Self {
        ChartSeries {
            name: name.into(),
            values,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chart {
    pub title: String,
    pub note: String,
    pub labels: Vec<String>,
    pub series: Vec<ChartSeries>,
    pub unit: Unit,
}

impl Chart {
    fn value(&self, series: usize, label: usize) -> Option<f64> {
        self.series
            .get(series)
            .and_then(|s| s.values.get(label))
            .copied()
            .flatten()
    }

    fn draw(&self, args: &ChartArgs, color: bool) -> String {
        render(
            self,
            args.chart.unwrap_or(ChartKind::Bar),
            args.direction,
            color,
        )
    }
}

/// One drawn piece of a bar: the value range it covers and which series owns it.
#[derive(Debug, Clone, Copy)]
struct Seg {
    lo: f64,
    hi: f64,
    series: usize,
}

fn has_values(c: &Chart) -> bool {
    c.series
        .iter()
        .any(|s| s.values.iter().any(Option::is_some))
}

pub fn render(c: &Chart, kind: ChartKind, direction: ChartDirection, color: bool) -> String {
    let mut out = format!("{}\n", c.title);
    if c.labels.is_empty() || !has_values(c) {
        out.push_str("No data to chart.\n");
        return out;
    }
    let body = match direction {
        ChartDirection::Vertical => vertical(c, kind, color),
        ChartDirection::Horizontal => horizontal(c, kind, color),
    };
    out.push_str(&body);
    if c.series.len() > 1 {
        out.push_str(&legend(c, color));
    }
    if !c.note.is_empty() {
        out.push_str(&c.note);
        out.push('\n');
    }
    out
}

/// Turns labels × series into drawable columns. `Bar` keeps one column per
/// series; `Stacked` sums them into one column, positives up and negatives down.
fn columns(c: &Chart, kind: ChartKind) -> Vec<Vec<Seg>> {
    let mut cols = Vec::new();
    match kind {
        ChartKind::Bar => {
            for label in 0..c.labels.len() {
                for series in 0..c.series.len() {
                    match c.value(series, label) {
                        Some(v) => cols.push(vec![Seg {
                            lo: v.min(0.0),
                            hi: v.max(0.0),
                            series,
                        }]),
                        None => cols.push(Vec::new()),
                    }
                }
            }
        }
        ChartKind::Stacked => {
            for label in 0..c.labels.len() {
                let mut col = Vec::new();
                let mut pos = 0.0_f64;
                let mut neg = 0.0_f64;
                for series in 0..c.series.len() {
                    if let Some(v) = c.value(series, label) {
                        if v >= 0.0 {
                            col.push(Seg {
                                lo: pos,
                                hi: pos + v,
                                series,
                            });
                            pos += v;
                        } else {
                            col.push(Seg {
                                lo: neg + v,
                                hi: neg,
                                series,
                            });
                            neg += v;
                        }
                    }
                }
                cols.push(col);
            }
        }
    }
    cols
}

/// The value range to draw, including zero. A flat all-zero series still gets
/// an axis instead of a division by zero.
fn scaled_range(cols: &[Vec<Seg>]) -> (f64, f64) {
    let mut lo = 0.0_f64;
    let mut hi = 0.0_f64;
    for col in cols {
        for seg in col {
            lo = lo.min(seg.lo).min(seg.hi);
            hi = hi.max(seg.lo).max(seg.hi);
        }
    }
    if (hi - lo).abs() < 1e-9 {
        if hi > 0.0 {
            (0.0, hi)
        } else if hi < 0.0 {
            (hi, 0.0)
        } else {
            (0.0, 1.0)
        }
    } else {
        (lo, hi)
    }
}

const GLYPHS: [char; 4] = ['█', '▓', '▒', '░'];
const PARTIAL: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
const PALETTE: [&str; 6] = [
    "\x1b[32m", "\x1b[33m", "\x1b[36m", "\x1b[35m", "\x1b[34m", "\x1b[31m",
];
const ROWS: usize = 12;
const BAR_COLS: usize = 44;

fn glyph(series: usize) -> char {
    GLYPHS[series % GLYPHS.len()]
}

fn paint(text: &str, series: usize, color: bool) -> String {
    if !color || text.is_empty() {
        return text.to_string();
    }
    format!("{}{text}\x1b[0m", PALETTE[series % PALETTE.len()])
}

fn legend(c: &Chart, color: bool) -> String {
    let mut line = String::from("  series:");
    for (i, s) in c.series.iter().enumerate() {
        let chip = paint(&format!("{} {}", glyph(i), s.name), i, color);
        line.push_str(&format!("  {chip}"));
    }
    line.push('\n');
    line
}

/// The glyph for one vertical cell: the segment covering most of this row.
fn cell(col: &[Seg], y_lo: f64, y_hi: f64, color: bool) -> String {
    let height = y_hi - y_lo;
    if height <= 0.0 {
        return " ".to_string();
    }
    let mut best: Option<(f64, usize, bool)> = None;
    for seg in col {
        let overlap = (y_hi.min(seg.hi) - y_lo.max(seg.lo)).max(0.0);
        if overlap > 0.0 && best.is_none_or(|(b, _, _)| overlap > b) {
            let below = seg.hi <= 0.0;
            best = Some((overlap, seg.series, below));
        }
    }
    let Some((overlap, series, below)) = best else {
        return " ".to_string();
    };
    let frac = (overlap / height).clamp(0.0, 1.0);
    let ch = if frac >= 0.999 {
        glyph(series)
    } else if below {
        '▀'
    } else {
        PARTIAL[((frac * 7.0).ceil() as usize).clamp(1, 7) - 1]
    };
    paint(&ch.to_string(), series, color)
}

fn vertical(c: &Chart, kind: ChartKind, color: bool) -> String {
    let cols = columns(c, kind);
    let (lo, hi) = scaled_range(&cols);
    let span = hi - lo;

    let nseries = if kind == ChartKind::Bar {
        c.series.len().max(1)
    } else {
        1
    };
    let group_chars = if kind == ChartKind::Bar {
        nseries * 2 - 1
    } else {
        1
    };

    let zero_row = if hi > 0.0 && lo < 0.0 {
        Some(((hi / span) * ROWS as f64).floor() as usize).map(|r| r.min(ROWS - 1))
    } else if hi <= 0.0 {
        Some(0)
    } else {
        None
    };

    // Y labels at regular ticks plus the zero line, so the scale reads at a
    // glance instead of only at the two ends.
    let mut ticks = vec![0, ROWS / 4, ROWS / 2, 3 * ROWS / 4, ROWS - 1];
    if let Some(z) = zero_row {
        if !ticks.contains(&z) {
            ticks.push(z);
        }
    }
    ticks.sort_unstable();
    ticks.dedup();
    let labels: Vec<Option<String>> = (0..ROWS)
        .map(|r| {
            if !ticks.contains(&r) {
                return None;
            }
            Some(if r == ROWS - 1 {
                format_value(c.unit, lo)
            } else if Some(r) == zero_row {
                "0".to_string()
            } else {
                format_value(c.unit, hi - span * r as f64 / ROWS as f64)
            })
        })
        .collect();
    let gutter = labels
        .iter()
        .flatten()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(1);

    // Width of one label slot. When the labels are few and short, widen the
    // spacing so every label reads in full; otherwise sample them.
    let max_label = c
        .labels
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(1);
    let slot = if c.labels.len() * (max_label + 1) <= 100 {
        (max_label + 1).max(group_chars + 1)
    } else {
        group_chars + 1
    };
    let step = if slot > group_chars + 1 {
        1
    } else {
        ((max_label + 2) as f64 / slot as f64).ceil().max(1.0) as usize
    };

    let mut out = String::new();
    for (r, tick_label) in labels.iter().enumerate() {
        let y_hi = hi - span * r as f64 / ROWS as f64;
        let y_lo = hi - span * (r + 1) as f64 / ROWS as f64;

        let label = tick_label.as_deref().unwrap_or("");
        out.push_str(&format!("{label:>gutter$}"));
        let axis = if r == ROWS - 1 {
            '└'
        } else if zero_row == Some(r) {
            '┼'
        } else {
            '┤'
        };
        out.push(axis);
        out.push(' ');
        for (g, group) in cols.chunks(nseries).enumerate() {
            if g > 0 {
                out.push_str(&" ".repeat(slot - group_chars));
            }
            for col in group {
                out.push_str(&cell(col, y_lo, y_hi, color));
            }
        }
        out.push('\n');
    }

    // X labels, placed only as often as they fit.
    let len = c.labels.len() * slot + max_label;
    let mut line = vec![' '; len];
    for (g, label) in c.labels.iter().enumerate() {
        if g % step != 0 {
            continue;
        }
        let start = g * slot;
        for (k, ch) in label.chars().enumerate() {
            let pos = start + k;
            if pos >= line.len() {
                break;
            }
            line[pos] = ch;
        }
    }
    let text: String = line.into_iter().collect();
    out.push_str(&" ".repeat(gutter + 1));
    out.push_str(text.trim_end());
    out.push('\n');
    out
}

/// One horizontal row's cells, filled per segment from the shared zero axis.
fn horizontal_cells(col: &[Seg], lo: f64, span: f64, width: usize) -> Vec<(char, usize)> {
    let mut cells = vec![(' ', usize::MAX); width];
    for seg in col {
        let mut a = ((seg.lo - lo) / span * width as f64).round() as i64;
        let mut b = ((seg.hi - lo) / span * width as f64).round() as i64;
        a = a.clamp(0, width as i64);
        b = b.clamp(0, width as i64);
        if a == b {
            if seg.hi > 0.0 {
                b = (b + 1).min(width as i64);
            } else if seg.hi < 0.0 {
                a = (a - 1).max(0);
            } else {
                continue;
            }
        }
        let start = a.min(b) as usize;
        let end = a.max(b) as usize;
        for cell in cells.iter_mut().take(end).skip(start) {
            *cell = (glyph(seg.series), seg.series);
        }
    }
    cells
}

fn render_cells(cells: &[(char, usize)], color: bool) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < cells.len() {
        let (_, series) = cells[i];
        let mut j = i + 1;
        while j < cells.len() && cells[j].1 == series {
            j += 1;
        }
        let run: String = cells[i..j].iter().map(|(c, _)| *c).collect();
        if series == usize::MAX {
            out.push_str(&run);
        } else {
            out.push_str(&paint(&run, series, color));
        }
        i = j;
    }
    out
}

fn horizontal(c: &Chart, kind: ChartKind, color: bool) -> String {
    let cols = columns(c, kind);
    let (lo, hi) = scaled_range(&cols);
    let span = hi - lo;
    let width = BAR_COLS;
    let nseries = c.series.len().max(1);

    // (label index, series index or MAX for a stacked row, segments, value text)
    let mut entries: Vec<(usize, usize, &[Seg], String)> = Vec::new();
    match kind {
        ChartKind::Bar => {
            for (i, col) in cols.iter().enumerate() {
                let label = i / nseries;
                let series = i % nseries;
                let value = c
                    .value(series, label)
                    .map(|v| format_value(c.unit, v))
                    .unwrap_or_else(|| f::DASH.to_string());
                entries.push((label, series, col.as_slice(), value));
            }
        }
        ChartKind::Stacked => {
            for (label, col) in cols.iter().enumerate() {
                let total: f64 = (0..c.series.len()).filter_map(|s| c.value(s, label)).sum();
                let value = if col.is_empty() {
                    f::DASH.to_string()
                } else {
                    format_value(c.unit, total)
                };
                entries.push((label, usize::MAX, col.as_slice(), value));
            }
        }
    }

    let label_w = c
        .labels
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(1)
        .clamp(1, 16);
    let value_w = entries
        .iter()
        .map(|e| e.3.chars().count())
        .max()
        .unwrap_or(3);

    let mut out = String::new();
    let mut prev_label = usize::MAX;
    for (label, _series, col, value) in entries {
        let prefix = if label != prev_label {
            format!("{:<label_w$} ", f::clip(&c.labels[label], label_w))
        } else {
            " ".repeat(label_w + 1)
        };
        prev_label = label;
        let bar = render_cells(&horizontal_cells(col, lo, span, width), color);
        out.push_str(&format!("{prefix}{bar} {value:>value_w$}\n"));
    }
    out
}

fn push_series(series: &mut Vec<ChartSeries>, name: &str, values: Vec<Option<f64>>) {
    if values.iter().any(Option::is_some) {
        series.push(ChartSeries::new(name, values));
    }
}

/// `stocks quote`: last price per ticker.
pub fn quotes(rows: &[Quote], args: &ChartArgs, color: bool) -> String {
    let currency = rows
        .iter()
        .find_map(|q| q.currency.clone())
        .unwrap_or_else(|| "IDR".into());
    let chart = Chart {
        title: format!(
            "Quotes · price in {currency} · {} ticker{}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ),
        note: "Last price, delayed at the source.".into(),
        labels: rows.iter().map(|q| q.symbol.to_string()).collect(),
        series: vec![ChartSeries::new(
            "Price",
            rows.iter().map(|q| q.price).collect(),
        )],
        unit: Unit::Price,
    };
    chart.draw(args, color)
}

/// `stocks history`: closing price per candle, respecting `--limit`.
pub fn history(h: &History, limit: Option<usize>, args: &ChartArgs, color: bool) -> String {
    let skip = limit
        .map(|n| h.candles.len().saturating_sub(n))
        .unwrap_or(0);
    let candles = &h.candles[skip..];
    let intraday = h.interval.is_intraday();
    let chart = Chart {
        title: format!("{} · close · {} / {}", h.symbol, h.period, h.interval),
        note: if skip > 0 {
            format!(
                "Showing last {} of {} candles (use --limit 0 for all).",
                candles.len(),
                h.candles.len()
            )
        } else {
            "Closing price per candle.".to_string()
        },
        labels: candles
            .iter()
            .map(|c| {
                if intraday {
                    c.time.format("%m-%d %H:%M").to_string()
                } else {
                    c.date().to_string()
                }
            })
            .collect(),
        series: vec![ChartSeries::new(
            "Close",
            candles.iter().map(|c| Some(c.close)).collect(),
        )],
        unit: Unit::Price,
    };
    chart.draw(args, color)
}

/// `stocks returns`: windows on the x axis, one series per ticker (plus the benchmark).
pub fn returns(list: &[Performance], args: &ChartArgs, color: bool) -> String {
    let labels = ReturnWindows::LABELS
        .iter()
        .map(|l| {
            if *l == "ytd" {
                "YTD".to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    let mut series: Vec<ChartSeries> = list
        .iter()
        .map(|p| ChartSeries::new(p.symbol.to_string(), p.returns.values().to_vec()))
        .collect();
    if let Some(b) = list.iter().find_map(|p| p.benchmark.as_ref()) {
        let name = b.symbol.to_string();
        if !series.iter().any(|s| s.name == name) {
            series.push(ChartSeries::new(name, b.returns.values().to_vec()));
        }
    }
    let as_of = list
        .first()
        .map(|p| p.as_of.to_string())
        .unwrap_or_default();
    let chart = Chart {
        title: format!("Price returns · percent · as of {as_of}"),
        note: "Dividends excluded; a window the history does not reach is left blank.".into(),
        labels,
        series,
        unit: Unit::Percent,
    };
    chart.draw(args, color)
}

/// `stocks dividends`: yearly dividend per share.
pub fn dividends(h: &DividendHistory, args: &ChartArgs, color: bool) -> String {
    let chart = Chart {
        title: format!(
            "{} · dividend per share by year ({})",
            h.symbol,
            h.currency.as_deref().unwrap_or("IDR")
        ),
        note: "Ex-date basis, amounts as reported (split-adjusted on Yahoo).".into(),
        labels: h.years.iter().map(|y| y.year.to_string()).collect(),
        series: vec![ChartSeries::new(
            "Dividend/share",
            h.years.iter().map(|y| Some(y.total)).collect(),
        )],
        unit: Unit::Number,
    };
    chart.draw(args, color)
}

/// `stocks financials`: revenue and net income per reporting period.
pub fn financials(fin: &Financials, args: &ChartArgs, color: bool) -> String {
    let labels = fin
        .periods
        .iter()
        .map(|p| match fin.period {
            StatementPeriod::Annual => format!("FY{}", p.end_date.format("%Y")),
            _ => p.end_date.to_string(),
        })
        .collect();
    let mut series = Vec::new();
    push_series(
        &mut series,
        "Revenue",
        fin.periods.iter().map(|p| p.income.total_revenue).collect(),
    );
    push_series(
        &mut series,
        "Net income",
        fin.periods
            .iter()
            .map(|p| p.income.net_income.or(p.income.net_income_common))
            .collect(),
    );
    let chart = Chart {
        title: format!(
            "{} · income statement · {} · {} periods",
            fin.symbol,
            fin.currency.as_deref().unwrap_or("source currency"),
            fin.periods.len()
        ),
        note: format!("Values as reported. Source: {}", fin.source),
        labels,
        series,
        unit: Unit::Compact,
    };
    chart.draw(args, color)
}

/// `stocks earnings`: EPS actual vs estimate, or revenue and earnings when the
/// source carries no quarterly EPS.
pub fn earnings(e: &Earnings, args: &ChartArgs, color: bool) -> String {
    let (labels, series, unit, title, note) = if !e.quarters.is_empty() {
        let mut series = Vec::new();
        push_series(
            &mut series,
            "EPS actual",
            e.quarters.iter().map(|q| q.eps_actual).collect(),
        );
        push_series(
            &mut series,
            "EPS estimate",
            e.quarters.iter().map(|q| q.eps_estimate).collect(),
        );
        (
            e.quarters.iter().map(|q| q.period.clone()).collect(),
            series,
            Unit::Number,
            format!(
                "{} · EPS actual vs estimate ({})",
                e.symbol,
                e.currency.as_deref().unwrap_or("IDR")
            ),
            format!("Reported quarters, oldest first. Source: {}", e.source),
        )
    } else {
        let rows: Vec<_> = if !e.yearly.is_empty() {
            e.yearly.iter().collect()
        } else {
            e.quarterly.iter().collect()
        };
        let mut series = Vec::new();
        push_series(
            &mut series,
            "Revenue",
            rows.iter().map(|r| r.revenue).collect(),
        );
        push_series(
            &mut series,
            "Earnings",
            rows.iter().map(|r| r.earnings).collect(),
        );
        (
            rows.iter().map(|r| r.period.clone()).collect(),
            series,
            Unit::Compact,
            format!(
                "{} · revenue and earnings ({})",
                e.symbol,
                e.currency.as_deref().unwrap_or("IDR")
            ),
            format!("Values as reported. Source: {}", e.source),
        )
    };
    let chart = Chart {
        title,
        note,
        labels,
        series,
        unit,
    };
    chart.draw(args, color)
}

/// `stocks compare`: profitability ratios side by side.
pub fn compare(list: &[Fundamentals], args: &ChartArgs, color: bool) -> String {
    let mut series = Vec::new();
    push_series(
        &mut series,
        "ROE",
        list.iter().map(|f| f.profitability.roe_pct).collect(),
    );
    push_series(
        &mut series,
        "ROA",
        list.iter().map(|f| f.profitability.roa_pct).collect(),
    );
    push_series(
        &mut series,
        "Net margin",
        list.iter()
            .map(|f| f.profitability.net_margin_pct)
            .collect(),
    );
    let chart = Chart {
        title: format!("Profitability · percent · {} companies", list.len()),
        note: "Ratios as reported by the source.".into(),
        labels: list.iter().map(|f| f.symbol.to_string()).collect(),
        series,
        unit: Unit::Percent,
    };
    chart.draw(args, color)
}

/// `stocks foreign`: foreign buying and selling per session.
pub fn foreign(flow: &ForeignFlow, args: &ChartArgs, color: bool) -> String {
    let mut series = Vec::new();
    push_series(
        &mut series,
        "Foreign buy",
        flow.days.iter().map(|d| d.foreign_buy).collect(),
    );
    push_series(
        &mut series,
        "Foreign sell",
        flow.days.iter().map(|d| d.foreign_sell).collect(),
    );
    let chart = Chart {
        title: format!(
            "{} · foreign flow · shares · {} sessions",
            flow.symbol,
            flow.days.len()
        ),
        note: "IDX publishes foreign trading in shares; non-regular trades excluded.".into(),
        labels: flow.days.iter().map(|d| d.date.to_string()).collect(),
        series,
        unit: Unit::Compact,
    };
    chart.draw(args, color)
}

/// `stocks screen`: the column the report was sorted by.
pub fn screen(report: &ScreenReport, args: &ChartArgs, color: bool) -> String {
    let key = Key::parse(&report.sort).unwrap_or(Key::Mcap);
    let (name, values, unit): (&str, Vec<Option<f64>>, Unit) = match key {
        Key::Pe => (
            "P/E",
            report.rows.iter().map(|r| r.pe_trailing).collect(),
            Unit::Ratio,
        ),
        Key::Fpe => (
            "Forward P/E",
            report.rows.iter().map(|r| r.pe_forward).collect(),
            Unit::Ratio,
        ),
        Key::Pb => (
            "P/B",
            report.rows.iter().map(|r| r.price_to_book).collect(),
            Unit::Ratio,
        ),
        Key::Ps => (
            "P/S",
            report.rows.iter().map(|r| r.price_to_sales).collect(),
            Unit::Ratio,
        ),
        Key::Peg => (
            "PEG",
            report.rows.iter().map(|r| r.peg).collect(),
            Unit::Ratio,
        ),
        Key::Price => (
            "Price",
            report.rows.iter().map(|r| r.price).collect(),
            Unit::Price,
        ),
        Key::Div => (
            "Dividend yield",
            report.rows.iter().map(|r| r.dividend_yield_pct).collect(),
            Unit::Percent,
        ),
        Key::Eps => (
            "EPS",
            report.rows.iter().map(|r| r.eps_ttm).collect(),
            Unit::Number,
        ),
        Key::Volume => (
            "Avg volume 3m",
            report
                .rows
                .iter()
                .map(|r| r.avg_volume_3m.map(|v| v as f64))
                .collect(),
            Unit::Compact,
        ),
        Key::Chg => (
            "Change",
            report.rows.iter().map(|r| r.change_pct).collect(),
            Unit::Percent,
        ),
        Key::Roe => (
            "ROE",
            report.rows.iter().map(|r| r.roe_pct).collect(),
            Unit::Percent,
        ),
        Key::Roa => (
            "ROA",
            report.rows.iter().map(|r| r.roa_pct).collect(),
            Unit::Percent,
        ),
        Key::Margin => (
            "Net margin",
            report.rows.iter().map(|r| r.net_margin_pct).collect(),
            Unit::Percent,
        ),
        Key::OpMargin => (
            "Operating margin",
            report.rows.iter().map(|r| r.operating_margin_pct).collect(),
            Unit::Percent,
        ),
        Key::De => (
            "Debt/equity",
            report.rows.iter().map(|r| r.debt_to_equity).collect(),
            Unit::Number,
        ),
        Key::Growth => (
            "Revenue growth",
            report.rows.iter().map(|r| r.revenue_growth_pct).collect(),
            Unit::Percent,
        ),
        Key::EpsGrowth => (
            "Earnings growth",
            report.rows.iter().map(|r| r.earnings_growth_pct).collect(),
            Unit::Percent,
        ),
        Key::Sector | Key::Industry => (
            "Market cap",
            report.rows.iter().map(|r| r.market_cap).collect(),
            Unit::Compact,
        ),
        Key::Mcap => (
            "Market cap",
            report.rows.iter().map(|r| r.market_cap).collect(),
            Unit::Compact,
        ),
    };
    let chart = Chart {
        title: format!("Screen · {} · {} rows", name, report.rows.len()),
        note: format!("Filters: {}", report.filters.join(", ")),
        labels: report.rows.iter().map(|r| r.symbol.to_string()).collect(),
        series: vec![ChartSeries::new(name, values)],
        unit,
    };
    chart.draw(args, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(unit: Unit) -> Chart {
        Chart {
            title: "Test chart".into(),
            note: String::new(),
            labels: vec!["A".into(), "B".into()],
            series: vec![
                ChartSeries::new("one", vec![Some(1.0), Some(3.0)]),
                ChartSeries::new("two", vec![Some(2.0), Some(-1.0)]),
            ],
            unit,
        }
    }

    #[test]
    fn horizontal_bar_has_label_bar_and_value() {
        let s = render(
            &sample(Unit::Number),
            ChartKind::Bar,
            ChartDirection::Horizontal,
            false,
        );
        assert!(s.contains("Test chart"));
        assert!(s.contains('█'));
        assert!(s.contains(" 1.00"));
        assert!(s.contains(" 3.00"));
        assert!(!s.contains("\x1b["), "no ANSI without color");
    }

    #[test]
    fn horizontal_stacked_uses_one_row_per_label_and_totals() {
        let s = render(
            &sample(Unit::Number),
            ChartKind::Stacked,
            ChartDirection::Horizontal,
            false,
        );
        // One row per label, plus the legend line.
        let rows: Vec<&str> = s.lines().skip(1).collect();
        assert_eq!(rows.len(), 3, "two labels + legend:\n{s}");
        assert!(s.contains("▓"), "second series has its own glyph");
        assert!(s.contains(" 3.00"), "A stacks 1 + 2");
        assert!(s.contains(" 2.00"), "B stacks 3 - 1");
        assert!(s.contains("series:"));
    }

    #[test]
    fn vertical_draws_axis_and_blocks() {
        let s = render(
            &sample(Unit::Number),
            ChartKind::Bar,
            ChartDirection::Vertical,
            false,
        );
        assert!(s.contains('┤'));
        assert!(s.contains('└'));
        assert!(s.contains('█'));
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }

    #[test]
    fn vertical_axis_labels_several_ticks() {
        let s = render(
            &sample(Unit::Number),
            ChartKind::Bar,
            ChartDirection::Vertical,
            false,
        );
        for v in ["3.00", "2.00", "1.00", "-1.00"] {
            assert!(s.contains(v), "missing y label {v}:\n{s}");
        }
        // The zero line carries its own label.
        assert!(
            s.lines().any(|l| l.contains("0┼") || l.contains("0┤")),
            "the zero row should be labelled:\n{s}"
        );
    }

    #[test]
    fn negative_values_get_their_own_side() {
        let s = render(
            &sample(Unit::Number),
            ChartKind::Bar,
            ChartDirection::Horizontal,
            false,
        );
        assert!(s.contains("-1.00"));
        // The positive bar for 3.00 is longer than the negative bar for -1.00.
        let line = |needle: &str| {
            s.lines()
                .find(|l| l.contains(needle))
                .unwrap_or("")
                .to_string()
        };
        let pos = line(" 3.00").matches('█').count();
        let neg = line("-1.00").matches('█').count();
        assert!(pos > neg, "bars share one scale:\n{s}");
    }

    #[test]
    fn color_paints_series() {
        let s = render(
            &sample(Unit::Number),
            ChartKind::Bar,
            ChartDirection::Horizontal,
            true,
        );
        assert!(s.contains("\x1b[32m"));
        assert!(s.contains("\x1b[0m"));
    }

    #[test]
    fn empty_chart_says_so() {
        let chart = Chart {
            title: "Empty".into(),
            note: String::new(),
            labels: vec![],
            series: vec![],
            unit: Unit::Number,
        };
        assert!(
            render(&chart, ChartKind::Bar, ChartDirection::Vertical, false)
                .contains("No data to chart.")
        );
    }
}
