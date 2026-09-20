//! Daily closes, so the app can say something about the past.
//!
//! ponytail: one JSON file per symbol under `<store>/history/`. Twenty holdings of five years of
//! daily bars is about a megabyte spread over twenty files, each read only when a chart is drawn.
//! SQLite is the upgrade when intraday bars arrive, and nothing outside this module would change:
//! callers only see `load`, `save` and `fetch`.
//!
//! Adjusted close, not close. A Norwegian portfolio is full of dividend payers, and an unadjusted
//! series silently reports every dividend as a loss on the ex-date.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;

const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) meridian";
const TIMEOUT: Duration = Duration::from_secs(20);

/// One trading day. `day` is `YYYY-MM-DD` in UTC: a chart axis, not an instant.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bar {
    pub day: String,
    pub close: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Series {
    pub currency: String,
    pub bars: Vec<Bar>,
}

impl Series {
    pub fn last(&self) -> Option<&Bar> {
        self.bars.last()
    }
}

/// Unix seconds to a UTC calendar day.
pub fn day(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .unwrap_or_default()
        .format("%Y-%m-%d")
        .to_string()
}

/// The daily series out of a chart response.
///
/// A null close is a day the exchange did not trade, or a gap in Yahoo's data. It is dropped, not
/// carried forward and never turned into a zero: a zero would show as a total wipeout and back to
/// normal the next day.
pub fn parse_series(body: &str) -> Option<Series> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let r = v.get("chart")?.get("result")?.get(0)?;
    let currency = r.get("meta")?.get("currency")?.as_str()?.to_string();
    let stamps = r.get("timestamp")?.as_array()?;
    let ind = r.get("indicators")?;
    // Adjusted close when Yahoo provides it, plain close when it does not. Preferring the
    // adjusted series is what makes a dividend a return rather than a drop.
    let closes = ind
        .get("adjclose")
        .and_then(|a| a.get(0))
        .and_then(|a| a.get("adjclose"))
        .and_then(|c| c.as_array())
        .or_else(|| {
            ind.get("quote")?
                .get(0)?
                .get("close")
                .and_then(|c| c.as_array())
        })?;

    let bars: Vec<Bar> = stamps
        .iter()
        .zip(closes)
        .filter_map(|(t, c)| {
            Some(Bar {
                day: day(t.as_i64()?),
                close: c.as_f64()?,
            })
        })
        .filter(|b| b.close > 0.0)
        .collect();
    (!bars.is_empty()).then_some(Series { currency, bars })
}

/// Five years of daily closes for one symbol. None on any failure, like every other fetch here.
pub fn fetch(symbol: &str) -> Option<Series> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=5y&interval=1d&events=div",
        urlencoding::encode(symbol)
    );
    let body = ureq::get(&url)
        .header("User-Agent", AGENT)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    parse_series(&body)
}

/// True when the series already reaches yesterday.
///
/// Yesterday, not today: today's bar does not exist until the exchange has traded, so asking for
/// one would refetch every symbol all day. Dates are YYYY-MM-DD, which compares correctly as text.
pub fn is_current(s: &Series) -> bool {
    let cutoff = day(chrono::Utc::now().timestamp() - 86_400);
    s.last().is_some_and(|b| b.day >= cutoff)
}

/// Five years of daily closes for several symbols, several at a time.
pub fn fetch_many(symbols: &[String]) -> Vec<(String, Option<Series>)> {
    crate::par::map(symbols, fetch)
}

/// Where one symbol's history lives. The symbol is not a filename: `BRK-B` is fine but a symbol
/// with a slash would escape the directory, so anything unexpected becomes an underscore.
fn path(cfg: &Config, symbol: &str) -> PathBuf {
    let safe: String = symbol
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '=' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cfg.store_dir.join("history").join(format!("{safe}.json"))
}

pub fn load(cfg: &Config, symbol: &str) -> Option<Series> {
    let raw = std::fs::read(path(cfg, symbol)).ok()?;
    serde_json::from_slice(&raw).ok()
}

pub fn save(cfg: &Config, symbol: &str, s: &Series) -> std::io::Result<()> {
    let p = path(cfg, symbol);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serde_json::to_vec(s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONTH: &str = include_str!("../tests/fixtures/chart_eqnr_1mo.json");

    #[test]
    fn a_chart_response_becomes_a_dated_daily_series() {
        let s = parse_series(MONTH).expect("parses");
        assert_eq!(s.currency, "NOK");
        assert_eq!(s.bars[0].day, "2026-08-18");
        assert!((s.bars[0].close - 394.1).abs() < 0.01);
        assert_eq!(s.last().expect("last").day, "2026-08-27");
    }

    #[test]
    fn a_day_the_exchange_did_not_trade_is_dropped_not_zeroed() {
        // The fixture has a null on 2026-08-21. A zero there would draw as a total wipeout that
        // recovers the next day, which is the chart equivalent of pricing a holding at nothing.
        let s = parse_series(MONTH).expect("parses");
        assert_eq!(s.bars.len(), 7, "8 timestamps, one of them null");
        assert!(!s.bars.iter().any(|b| b.day == "2026-08-21"));
        assert!(s.bars.iter().all(|b| b.close > 0.0));
    }

    #[test]
    fn garbage_is_none_rather_than_an_empty_series() {
        // None and "no bars" mean different things to the caller: one is a failed fetch worth
        // retrying, the other is a symbol that genuinely has no history.
        assert!(parse_series("not json").is_none());
        assert!(parse_series(r#"{"chart":{"result":[]}}"#).is_none());
    }

    #[test]
    fn a_series_survives_a_round_trip_through_the_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), crate::config::Scope::Scandinavia);
        let s = parse_series(MONTH).expect("parses");
        save(&cfg, "EQNR.OL", &s).expect("save");
        assert_eq!(load(&cfg, "EQNR.OL").expect("load"), s);
        assert!(load(&cfg, "NOSUCH.OL").is_none());
    }

    #[test]
    fn a_symbol_can_never_escape_the_history_directory() {
        // Symbols arrive from an import, where the user types them. A slash or a .. in one must
        // not turn a save into a write somewhere else on disk.
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), crate::config::Scope::Scandinavia);
        for nasty in ["../../etc/passwd", "..", "/etc/passwd", "a/../../b", ""] {
            let p = path(&cfg, nasty);
            // The property that matters: whatever the symbol was, the write lands directly in the
            // history directory. The separators are gone, so there is nothing left to traverse.
            assert_eq!(
                p.parent().expect("parent"),
                cfg.store_dir.join("history"),
                "{nasty}"
            );
            let name = p
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            assert!(
                !name.contains(std::path::MAIN_SEPARATOR),
                "{nasty} -> {name}"
            );
            assert!(p.starts_with(&cfg.store_dir), "{nasty} -> {}", p.display());
        }
        // The ordinary shapes still survive intact: a suffix, a dash, an fx pair.
        assert!(path(&cfg, "EQNR.OL").ends_with("EQNR.OL.json"));
        assert!(path(&cfg, "BRK-B").ends_with("BRK-B.json"));
        assert!(path(&cfg, "EURNOK=X").ends_with("EURNOK=X.json"));
    }
}
