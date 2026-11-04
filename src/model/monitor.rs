//! The live tape behind `idx stockbit monitor`: market movers and the running
//! trade of one ticker.
//!
//! Prices are IDR. A field the source did not return is `None`; nothing here is
//! estimated. Units follow the exchange and the rest of `idx`: quote volume is
//! **shares**, and a print's volume is whatever Stockbit reports for it (see
//! [`Trade::volume`]).

use chrono::{DateTime, FixedOffset};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Symbol;

/// Which hotlist a row came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MoverKind {
    Gainer,
    Loser,
    MostActive,
}

impl MoverKind {
    /// The segment Stockbit's hotlist route uses.
    pub fn as_path(&self) -> &'static str {
        match self {
            MoverKind::Gainer => "topGainer",
            MoverKind::Loser => "topLoser",
            MoverKind::MostActive => "mostActive",
        }
    }

    /// Label for the terminal.
    pub fn as_str(&self) -> &'static str {
        match self {
            MoverKind::Gainer => "Top gainer",
            MoverKind::Loser => "Top loser",
            MoverKind::MostActive => "Most active",
        }
    }
}

/// One row of a market-mover list, reduced to what a board shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Mover {
    pub symbol: Symbol,
    pub name: Option<String>,
    /// Last traded price.
    pub last: Option<f64>,
    /// Absolute change versus the previous close.
    pub change: Option<f64>,
    /// Percent change, e.g. `2.51` for +2.51 %.
    pub change_pct: Option<f64>,
    /// Session volume in shares.
    pub volume: Option<u64>,
    /// Session value traded, IDR.
    pub value: Option<f64>,
}

/// One hotlist as the source ordered it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Movers {
    pub kind: MoverKind,
    pub rows: Vec<Mover>,
}

/// Which side of the book printed a trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TradeSide {
    /// A buyer lifted the offer.
    Buy,
    /// A seller hit the bid.
    Sell,
    /// The source did not say, or said something this build does not map.
    Unknown,
}

impl TradeSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            TradeSide::Buy => "B",
            TradeSide::Sell => "S",
            TradeSide::Unknown => "?",
        }
    }
}

/// One print on the tape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Trade {
    /// Print time in WIB, when the source dates it.
    pub time: Option<DateTime<FixedOffset>>,
    /// Trade price, IDR.
    pub price: f64,
    /// Size as the source reports it. Stockbit's running trade quotes lots on
    /// screen; this field carries the number it sends, unconverted, so a caller
    /// never reads a size x100 off the real one.
    pub volume: u64,
    pub side: TradeSide,
    /// Buying broker code, when the source named one.
    pub buyer: Option<String>,
    /// Selling broker code, when the source named one.
    pub seller: Option<String>,
}

/// The tape for one ticker, newest first as the source ordered it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunningTrade {
    pub symbol: Symbol,
    pub trades: Vec<Trade>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mover_kind_paths_are_the_route_segments() {
        assert_eq!(MoverKind::Gainer.as_path(), "topGainer");
        assert_eq!(MoverKind::Loser.as_path(), "topLoser");
        assert_eq!(MoverKind::MostActive.as_path(), "mostActive");
    }

    #[test]
    fn side_labels_are_the_single_letters() {
        assert_eq!(TradeSide::Buy.as_str(), "B");
        assert_eq!(TradeSide::Sell.as_str(), "S");
        assert_eq!(TradeSide::Unknown.as_str(), "?");
    }
}
