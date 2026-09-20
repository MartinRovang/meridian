//! Prices: fetching them from Yahoo, caching them on disk, and knowing when they are old.
//!
//! ponytail: Yahoo's chart endpoint, one request per symbol, no API key and no crumb. The v7
//! quote endpoint takes several symbols at once but now demands a cookie and a crumb, which is
//! more moving parts than a portfolio of a few dozen positions is worth. If the position count
//! ever makes the request count hurt, batching is the upgrade.
//!
//! This endpoint is UNOFFICIAL. Everything here is built so that losing it degrades the app to
//! stale prices rather than breaking it.

use std::collections::HashMap;
use std::time::Duration;

use crate::config::Config;
use crate::types::Quote;

/// A quote older than this is shown dimmed, with its age.
pub const STALE_AFTER: i64 = 15 * 60;

/// Yahoo rejects the default agent string outright.
const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) meridian";
const TIMEOUT: Duration = Duration::from_secs(10);

pub type Cache = HashMap<String, Quote>;

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn age_secs(q: &Quote, now: i64) -> i64 {
    now - q.ts
}

pub fn is_stale(q: &Quote, now: i64) -> bool {
    age_secs(q, now) > STALE_AFTER
}

/// The price out of a chart response, or None when Yahoo does not know the symbol.
///
/// Returning None rather than a zero is load-bearing: a zero would be silently counted into
/// portfolio totals, and a wrong total is worse than a visibly missing one.
pub fn parse_chart(body: &str) -> Option<Quote> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let meta = v.get("chart")?.get("result")?.get(0)?.get("meta")?;
    let price = meta.get("regularMarketPrice")?.as_f64()?;
    let prev = meta
        .get("chartPreviousClose")
        .or_else(|| meta.get("previousClose"))
        .and_then(|p| p.as_f64())
        .unwrap_or(price);
    let currency = meta.get("currency")?.as_str()?.to_string();
    Some(Quote {
        price,
        prev_close: prev,
        currency,
        ts: now(),
    })
}

/// Fetch one symbol. None on any failure: a network error and an unknown symbol are the same
/// thing to the caller, which falls back to whatever is cached.
pub fn fetch(symbol: &str) -> Option<Quote> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=1d",
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
    parse_chart(&body)
}

/// The cache on disk. A damaged or missing file is an empty cache, because nothing here is
/// authoritative: the worst case is one extra round of fetches.
pub fn load(cfg: &Config) -> Cache {
    std::fs::read(cfg.quotes_path())
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &Config, c: &Cache) -> std::io::Result<()> {
    std::fs::create_dir_all(&cfg.store_dir)?;
    let tmp = cfg.quotes_path().with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec(c).unwrap_or_else(|_| b"{}".to_vec()),
    )?;
    std::fs::rename(tmp, cfg.quotes_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Scope;

    const OK: &str = include_str!("../tests/fixtures/chart_eqnr_ol.json");
    const UNKNOWN: &str = include_str!("../tests/fixtures/chart_unknown.json");

    #[test]
    fn a_chart_response_yields_price_previous_close_and_currency() {
        let q = parse_chart(OK).expect("a known symbol parses");
        assert_eq!(q.price, 419.0);
        assert_eq!(q.prev_close, 416.3);
        assert_eq!(q.currency, "NOK");
    }

    #[test]
    fn an_unknown_symbol_is_none_not_a_zero_price() {
        assert!(parse_chart(UNKNOWN).is_none());
    }

    #[test]
    fn garbage_is_none_rather_than_a_panic() {
        assert!(parse_chart("").is_none());
        assert!(parse_chart("{}").is_none());
        assert!(parse_chart("not json at all").is_none());
        assert!(parse_chart(r#"{"chart":{"result":[]}}"#).is_none());
        assert!(parse_chart(r#"{"chart":{"result":[{"meta":{}}]}}"#).is_none());
    }

    #[test]
    fn a_quote_goes_stale_on_a_clock_we_control() {
        let q = Quote {
            price: 1.0,
            prev_close: 1.0,
            currency: "NOK".into(),
            ts: 1_000,
        };
        assert_eq!(age_secs(&q, 1_060), 60);
        assert!(!is_stale(&q, 1_000 + STALE_AFTER - 1));
        assert!(is_stale(&q, 1_000 + STALE_AFTER + 1));
    }

    #[test]
    fn the_cache_round_trips_through_a_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), Scope::Scandinavia);
        let mut c = Cache::new();
        c.insert(
            "EQNR.OL".into(),
            Quote {
                price: 419.0,
                prev_close: 416.3,
                currency: "NOK".into(),
                ts: 1,
            },
        );
        save(&cfg, &c).expect("save");
        let back = load(&cfg);
        assert_eq!(back.get("EQNR.OL").map(|q| q.price), Some(419.0));
        assert!(!back.contains_key("NOPE.OL"));
    }

    #[test]
    fn a_damaged_cache_loads_empty_because_it_is_only_a_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), Scope::Scandinavia);
        std::fs::create_dir_all(&cfg.store_dir).expect("mkdir");
        std::fs::write(cfg.quotes_path(), b"garbage").expect("write");
        assert!(load(&cfg).is_empty());
    }
}
