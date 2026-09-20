//! Where the store lives, which markets we care about, and what currency totals are shown in.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which exchanges ticker search is allowed to return.
///
/// ponytail: a suffix list per scope, not a market-data abstraction. Widening it is a new
/// match arm; if this ever needs per-exchange trading hours it has outgrown the enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Norway,
    #[default]
    Scandinavia,
    Nordics,
    Europe,
    Global,
}

impl Scope {
    /// Yahoo exchange suffixes this scope admits. Empty means "everything".
    pub fn suffixes(&self) -> &'static [&'static str] {
        match self {
            Scope::Norway => &[".OL"],
            Scope::Scandinavia => &[".OL", ".ST", ".CO"],
            Scope::Nordics => &[".OL", ".ST", ".CO", ".HE", ".IC"],
            Scope::Europe => &[
                ".OL", ".ST", ".CO", ".HE", ".IC", ".DE", ".PA", ".AS", ".L", ".MI",
            ],
            Scope::Global => &[],
        }
    }

    pub fn default_currency(&self) -> &'static str {
        match self {
            Scope::Norway | Scope::Scandinavia | Scope::Nordics => "NOK",
            Scope::Europe => "EUR",
            Scope::Global => "USD",
        }
    }

    /// Whether a Yahoo symbol belongs to this scope. A suffix-less symbol is a US listing.
    pub fn accepts(&self, symbol: &str) -> bool {
        let suffixes = self.suffixes();
        if suffixes.is_empty() {
            return true;
        }
        suffixes.iter().any(|s| symbol.ends_with(s))
    }

    pub fn parse(s: &str) -> Option<Scope> {
        match s.to_ascii_lowercase().as_str() {
            "norway" => Some(Scope::Norway),
            "scandinavia" => Some(Scope::Scandinavia),
            "nordics" => Some(Scope::Nordics),
            "europe" => Some(Scope::Europe),
            "global" => Some(Scope::Global),
            _ => None,
        }
    }
}

/// Everything the server needs to know that is not in the store file.
#[derive(Clone, Debug)]
pub struct Config {
    /// Directory holding portfolios.json and quotes.json.
    pub store_dir: PathBuf,
    pub scope: Scope,
}

impl Config {
    pub fn new(store_dir: PathBuf, scope: Scope) -> Config {
        Config { store_dir, scope }
    }

    pub fn portfolios_path(&self) -> PathBuf {
        self.store_dir.join("portfolios.json")
    }

    pub fn quotes_path(&self) -> PathBuf {
        self.store_dir.join("quotes.json")
    }

    /// The default local store, `~/.meridian`. A server is always given one explicitly.
    pub fn default_store_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".meridian")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_suffixes_widen_outward() {
        assert_eq!(Scope::Norway.suffixes(), &[".OL"]);
        assert_eq!(Scope::Scandinavia.suffixes(), &[".OL", ".ST", ".CO"]);
        assert!(Scope::Nordics.suffixes().contains(&".HE"));
        assert!(Scope::Global.suffixes().is_empty());
    }

    #[test]
    fn scope_carries_a_default_currency() {
        assert_eq!(Scope::Scandinavia.default_currency(), "NOK");
        assert_eq!(Scope::Europe.default_currency(), "EUR");
        assert_eq!(Scope::Global.default_currency(), "USD");
    }

    #[test]
    fn a_global_scope_accepts_any_symbol_and_a_narrow_one_does_not() {
        assert!(Scope::Global.accepts("AAPL"));
        assert!(Scope::Norway.accepts("EQNR.OL"));
        assert!(!Scope::Norway.accepts("AAPL"));
        assert!(Scope::Scandinavia.accepts("VOLV-B.ST"));
    }

    #[test]
    fn scope_round_trips_through_its_name() {
        for s in [
            Scope::Norway,
            Scope::Scandinavia,
            Scope::Nordics,
            Scope::Europe,
            Scope::Global,
        ] {
            let name = serde_json::to_string(&s).expect("serialize");
            let bare = name.trim_matches('"');
            assert_eq!(Scope::parse(bare), Some(s), "{bare}");
        }
        assert_eq!(Scope::parse("atlantis"), None);
    }
}
