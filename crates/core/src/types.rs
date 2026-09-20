//! The serde types every layer shares: what is stored, and what the API returns.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A position, as the user entered it. Nothing derived is stored here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Holding {
    pub id: String,
    /// The full Yahoo symbol, e.g. "EQNR.OL".
    pub ticker: String,
    pub name: String,
    /// Free-text asset class, used to group the allocation donut.
    pub cls: String,
    pub shares: f64,
    /// Total paid for the position, not a per-share average.
    pub cost_basis: f64,
    pub cost_currency: String,
    pub target_pct: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Portfolio {
    pub id: String,
    pub name: String,
    pub owner: String,
    /// Drift tolerance in percentage points, applied to every holding in this portfolio.
    pub band_pct: f64,
    pub holdings: Vec<Holding>,
}

/// The whole persisted file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Store {
    pub version: u32,
    pub base_currency: String,
    pub portfolios: Vec<Portfolio>,
    /// Broker name to ticker, learned when an import is confirmed: "Xtrackers NASDAQ 100 ETF 1C"
    /// to "XNAS.DE". Exports name funds, not symbols, and the matching cannot be fully automatic,
    /// so the answer is kept and the next import of the same account needs no clicks.
    ///
    /// `default` is load-bearing: a portfolios.json written before this field existed must still
    /// parse. A parse failure is treated as corruption and the file is moved aside.
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    /// What buzzes your phone. Default for the same reason as `aliases`: a store written before
    /// this field existed must still parse, or the user's positions are treated as corruption.
    #[serde(default)]
    pub alerts: crate::alerts::Alerts,
    /// Saved conditions over the figures the app already computes. Same reason for the default.
    #[serde(default)]
    pub rules: Vec<crate::rules::Rule>,
}

impl Store {
    pub fn empty(base_currency: &str) -> Store {
        Store {
            version: 1,
            base_currency: base_currency.to_string(),
            portfolios: Vec::new(),
            aliases: HashMap::new(),
            alerts: crate::alerts::Alerts::default(),
            rules: Vec::new(),
        }
    }
}

/// One cached price. `ts` is unix seconds at fetch time, not exchange time.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Quote {
    pub price: f64,
    pub prev_close: f64,
    pub currency: String,
    pub ts: i64,
}

/// A hex id with a one-letter kind prefix, e.g. "p_9f3c1a". Short enough to read in a JSON file.
pub fn new_id(prefix: char) -> String {
    let mut raw = [0u8; 4];
    getrandom::fill(&mut raw).expect("randomness");
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}_{hex}")
}
