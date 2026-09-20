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

/// Re-exported so callers that think in symbols do not have to know about the thread pool.
pub use crate::par::MAX_PARALLEL;

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn age_secs(q: &Quote, now: i64) -> i64 {
    now - q.ts
}

pub fn is_stale(q: &Quote, now: i64) -> bool {
    age_secs(q, now) > STALE_AFTER
}

/// A quote younger than this is not worth refetching. Two windows opened a moment apart, or a
/// second client booting against a hosted server, should cost nothing. An explicit Refresh passes
/// a ttl of 0 and refetches regardless: the user asking for prices now means now.
pub const REFRESH_TTL: i64 = 60;

/// Which of `wanted` actually have to be fetched: the ones with no quote at all, and the ones
/// whose quote has aged past `ttl`.
///
/// ponytail: this is the whole cache policy. It is a filter over a HashMap because the working set
/// is one quote per held symbol, a few dozen at most. A cache server for that would be a daemon,
/// a network hop and a fallback path to guard a few hundred bytes.
pub fn needs_fetch(cache: &Cache, wanted: &[String], ttl: i64, now: i64) -> Vec<String> {
    wanted
        .iter()
        .filter(|s| cache.get(*s).is_none_or(|q| age_secs(q, now) >= ttl))
        .cloned()
        .collect()
}

/// Fetch every symbol, several at a time.
pub fn fetch_many(symbols: &[String]) -> Vec<(String, Option<Quote>)> {
    crate::par::map(symbols, fetch)
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

/// One row from a ticker search, already narrowed to the configured scope.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct SearchHit {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    /// Yahoo's search does not return a currency; it is filled in when the quote is first fetched.
    pub currency: String,
}

/// The Yahoo symbol for a currency pair, or None when no conversion is needed.
///
/// ponytail: one hop, base currency only. A cross rate through a third currency is not
/// attempted; Yahoo quotes every pair we care about directly.
pub fn fx_symbol(from: &str, to: &str) -> Option<String> {
    let (from, to) = (from.to_ascii_uppercase(), to.to_ascii_uppercase());
    if from == to {
        return None;
    }
    Some(format!("{from}{to}=X"))
}

/// Search hits for a query, keeping only the exchanges this scope admits and only instruments
/// you can hold a share of. Order is Yahoo's own relevance order, which for a Nordic name puts
/// the US listing first, so the scope filter is what makes this usable rather than a nicety.
pub fn parse_search(body: &str, scope: crate::config::Scope) -> Vec<SearchHit> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(rows) = v.get("quotes").and_then(|q| q.as_array()) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|r| {
            let symbol = r.get("symbol")?.as_str()?.to_string();
            if !scope.accepts(&symbol) {
                return None;
            }
            // ponytail: currencies, indices, crypto and futures are not things you can hold a
            // share of. Yahoo returns them freely; a portfolio of them would break every weight.
            let kind = r.get("quoteType").and_then(|k| k.as_str()).unwrap_or("");
            if !matches!(kind, "EQUITY" | "ETF" | "MUTUALFUND") {
                return None;
            }
            Some(SearchHit {
                symbol,
                // longname before shortname: Oslo's shortname for EQNR.OL is the bare exchange
                // label "EQUINOR", while longname is "Equinor ASA". The long one is what a
                // person scanning a result list actually recognises.
                name: r
                    .get("longname")
                    .or_else(|| r.get("shortname"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                exchange: r
                    .get("exchange")
                    .and_then(|e| e.as_str())
                    .unwrap_or("")
                    .to_string(),
                currency: String::new(),
            })
        })
        .collect()
}

/// Run a search against Yahoo. An empty vector on any failure.
pub fn search(query: &str, scope: crate::config::Scope) -> Vec<SearchHit> {
    let url = format!(
        "https://query1.finance.yahoo.com/v1/finance/search?q={}&quotesCount=20&newsCount=0",
        urlencoding::encode(query)
    );
    ureq::get(&url)
        .header("User-Agent", AGENT)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_string().ok())
        .map(|b| parse_search(&b, scope))
        .unwrap_or_default()
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

    const SEARCH: &str = include_str!("../tests/fixtures/search_equinor.json");

    fn symbols(scope: Scope) -> Vec<String> {
        parse_search(SEARCH, scope)
            .into_iter()
            .map(|h| h.symbol)
            .collect()
    }

    #[test]
    fn search_in_a_narrow_scope_drops_the_foreign_listings() {
        // The fixture is real: Yahoo ranks the NYSE listing ABOVE the Oslo one, and throws in
        // OTC pink sheets, Frankfurt and Dusseldorf. This filter is why search is usable.
        assert_eq!(symbols(Scope::Norway), vec!["EQNR.OL"]);
    }

    #[test]
    fn a_wider_scope_keeps_stockholm_and_copenhagen_too() {
        assert_eq!(
            symbols(Scope::Scandinavia),
            vec!["EQNR.OL", "VOLCAR-B.ST", "VOLV-B.ST", "NOVO-B.CO"]
        );
    }

    #[test]
    fn europe_reaches_german_listings_that_scandinavia_does_not() {
        let eu = symbols(Scope::Europe);
        assert!(eu.contains(&"NOV.DE".to_string()));
        assert!(eu.contains(&"EQNR.OL".to_string()));
        assert!(!symbols(Scope::Scandinavia).contains(&"NOV.DE".to_string()));
    }

    #[test]
    fn global_scope_keeps_everything_tradeable_in_the_order_yahoo_gave() {
        let all = symbols(Scope::Global);
        assert_eq!(all[0], "EQNR", "yahoo's own ranking is preserved");
        assert_eq!(all[1], "EQNR.OL");
        assert!(
            all.contains(&"STOHF".to_string()),
            "otc listings are still holdable"
        );
    }

    #[test]
    fn things_you_cannot_hold_a_share_of_are_dropped_even_in_global_scope() {
        // The fixture carries NVOX-USD, a CRYPTOCURRENCY row Yahoo returns for "novo nordisk".
        assert!(!symbols(Scope::Global).contains(&"NVOX-USD".to_string()));
    }

    #[test]
    fn a_search_hit_carries_the_name_users_recognise() {
        let hits = parse_search(SEARCH, Scope::Norway);
        assert_eq!(hits[0].name, "Equinor ASA");
        assert_eq!(hits[0].exchange, "OSL");
    }

    #[test]
    fn a_search_over_garbage_is_empty_rather_than_a_panic() {
        assert!(parse_search("", Scope::Global).is_empty());
        assert!(parse_search("{}", Scope::Global).is_empty());
    }

    #[test]
    fn fx_is_a_yahoo_pair_symbol_and_the_same_currency_needs_none() {
        assert_eq!(fx_symbol("USD", "NOK"), Some("USDNOK=X".to_string()));
        assert_eq!(fx_symbol("SEK", "NOK"), Some("SEKNOK=X".to_string()));
        assert_eq!(
            fx_symbol("nok", "NOK"),
            None,
            "same currency needs no conversion"
        );
    }

    #[test]
    fn a_damaged_cache_loads_empty_because_it_is_only_a_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), Scope::Scandinavia);
        std::fs::create_dir_all(&cfg.store_dir).expect("mkdir");
        std::fs::write(cfg.quotes_path(), b"garbage").expect("write");
        assert!(load(&cfg).is_empty());
    }

    fn q(ts: i64) -> Quote {
        Quote {
            price: 1.0,
            prev_close: 1.0,
            currency: "NOK".into(),
            ts,
        }
    }

    fn syms(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("S{i}")).collect()
    }

    #[test]
    fn a_symbol_with_no_quote_at_all_is_always_fetched() {
        let cache = Cache::new();
        assert_eq!(needs_fetch(&cache, &syms(2), REFRESH_TTL, 1_000), syms(2));
    }

    #[test]
    fn a_quote_younger_than_the_ttl_is_left_alone() {
        let mut cache = Cache::new();
        cache.insert("S0".into(), q(1_000));
        // 30 seconds old against a 60 second ttl: the second window to open costs no requests.
        assert!(needs_fetch(&cache, &syms(1), REFRESH_TTL, 1_030).is_empty());
    }

    #[test]
    fn a_quote_older_than_the_ttl_is_fetched_again() {
        let mut cache = Cache::new();
        cache.insert("S0".into(), q(1_000));
        assert_eq!(needs_fetch(&cache, &syms(1), REFRESH_TTL, 1_061), syms(1));
    }

    #[test]
    fn a_ttl_of_zero_refetches_everything_however_fresh() {
        let mut cache = Cache::new();
        cache.insert("S0".into(), q(1_000));
        // What the Refresh button sends. A user asking for prices now must not be told they
        // already have them.
        assert_eq!(needs_fetch(&cache, &syms(1), 0, 1_000), syms(1));
    }
}
