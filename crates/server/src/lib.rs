//! Meridian's JSON API.
//!
//! This crate links no Tauri and no desktop library on purpose: the same code runs inside the
//! desktop app and on a server with nothing installed on it. If a dependency here ever pulls in
//! gtk or webkit, the split this design exists for is gone, and CI's server-isolation job is
//! what notices.
//!
//! Auth is one shared secret in X-Meridian-Token. There is one store per server and one token
//! that opens it. Accounts are a separate project.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use meridian_core::config::Config;
use meridian_core::{
    alerts, calc, discover, history, import, optimize, quotes, rules, store, universe,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The most a request body may be.
const BODY_MAX: usize = 1024 * 1024;

/// An error the page should read: (status, message).
pub struct Fail(pub u16, pub String);

impl Fail {
    pub fn new(code: u16, msg: impl Into<String>) -> Fail {
        Fail(code, msg.into())
    }
}

type Out = Result<Value, Fail>;

/// Everything a route needs. The quote cache is shared so a refresh is visible to the next read.
#[derive(Clone)]
pub struct Ctx {
    pub cfg: Config,
    pub cache: Arc<Mutex<quotes::Cache>>,
    /// Held for the length of a refresh. A second caller does not get turned away, it waits and
    /// then finds every quote fresh, so two windows booting together make one set of requests.
    pub refreshing: Arc<Mutex<()>>,
}

/// Equal without leaking where they differ.
pub fn same_token(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A random session token, hex.
pub fn new_token() -> String {
    let mut raw = [0u8; 24];
    getrandom::fill(&mut raw).expect("randomness");
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

fn header(req: &Request, name: &'static str) -> String {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
        .unwrap_or_default()
}

/// Every reply.
///
/// ponytail: the TOKEN is the guard here, not the origin, so the allow-origin is a wildcard and
/// says so honestly. gitdashy's Host allowlist is deliberately not ported: it was meaningful on
/// loopback and is meaningless once the server is legitimately remote, which is the whole point
/// of this crate. Tighten the origin the day a browser client with cookies exists; there is none.
fn send(req: Request, code: u16, body: Vec<u8>, ctype: &str) {
    let ok = |h: Result<Header, ()>| h.expect("a static header is well formed");
    let resp = Response::from_data(body)
        .with_status_code(code)
        .with_header(ok(Header::from_bytes("Content-Type", ctype)))
        .with_header(ok(Header::from_bytes("Access-Control-Allow-Origin", "*")))
        .with_header(ok(Header::from_bytes(
            "Access-Control-Allow-Headers",
            "X-Meridian-Token, Content-Type",
        )))
        .with_header(ok(Header::from_bytes(
            "Access-Control-Allow-Methods",
            "GET, POST, PATCH, DELETE, OPTIONS",
        )))
        .with_header(ok(Header::from_bytes("Cache-Control", "no-store")));
    let _ = req.respond(resp);
}

fn send_json(req: Request, code: u16, body: Value) {
    send(req, code, body.to_string().into_bytes(), "application/json");
}

/// The whole computed view: every portfolio, its holdings, and how old the prices are.
fn state(ctx: &Ctx) -> Out {
    let s = store::load(&ctx.cfg).map_err(|e| Fail::new(500, e.to_string()))?;
    let cache = ctx.cache.lock().expect("cache lock").clone();
    let now = quotes::now();
    // The OLDEST quote decides staleness: one fresh price does not make the screen current.
    let oldest = s
        .portfolios
        .iter()
        .flat_map(|p| p.holdings.iter())
        .filter_map(|h| cache.get(&h.ticker))
        .map(|q| q.ts)
        .min();
    let views: Vec<calc::PortfolioView> = s
        .portfolios
        .iter()
        .map(|p| calc::view_portfolio(p, &s.base_currency, &cache))
        .collect();
    let drift: Vec<Value> = views
        .iter()
        .map(|v| json!({ "id": v.id, "rows": calc::drift(v) }))
        .collect();
    Ok(json!({
        "version": VERSION,
        "base_currency": s.base_currency,
        "scope": ctx.cfg.scope,
        "portfolios": views,
        "drift": drift,
        "quotes_age_secs": oldest.map(|t| now - t),
        "stale": oldest.is_some_and(|t| now - t > quotes::STALE_AFTER),
    }))
}

/// Read a JSON object body, capped.
pub fn body(req: &mut Request) -> Result<Value, Fail> {
    let mut raw = Vec::new();
    req.as_reader()
        .take(BODY_MAX as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| Fail::new(400, e.to_string()))?;
    if raw.len() > BODY_MAX {
        return Err(Fail::new(413, "body too large"));
    }
    if raw.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&raw).map_err(|_| Fail::new(400, "bad body"))
}

/// Read a file body unchanged, capped.
///
/// Not `body`: a broker export is commonly UTF-16, and forcing those bytes through a JSON string
/// would destroy the very encoding the parser exists to cope with.
fn raw_body(req: &mut Request) -> Result<Vec<u8>, Fail> {
    let mut raw = Vec::new();
    req.as_reader()
        .take(BODY_MAX as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| Fail::new(400, e.to_string()))?;
    if raw.len() > BODY_MAX {
        return Err(Fail::new(
            413,
            "that file is too large to be a positions export",
        ));
    }
    Ok(raw)
}

/// The trailing path segment, for routes shaped /api/thing/:id.
fn tail<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)
        .filter(|r| !r.is_empty() && !r.contains('/'))
}

fn need_str(v: &Value, key: &str) -> Result<String, Fail> {
    let s = v
        .get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if s.is_empty() {
        return Err(Fail::new(400, format!("{key} is required")));
    }
    Ok(s)
}

/// A finite number, or a 400.
///
/// as_f64 rejects a string and a null for us; the is_finite check is what stops a NaN or an
/// infinity reaching the store, where it would silently poison every total that touches it.
fn need_f64(v: &Value, key: &str) -> Result<f64, Fail> {
    let n = v
        .get(key)
        .and_then(|x| x.as_f64())
        .ok_or_else(|| Fail::new(400, format!("{key} must be a number")))?;
    if !n.is_finite() {
        return Err(Fail::new(400, format!("{key} must be a real number")));
    }
    Ok(n)
}

fn load(ctx: &Ctx) -> Result<meridian_core::types::Store, Fail> {
    store::load(&ctx.cfg).map_err(|e| Fail::new(500, e.to_string()))
}

fn commit(ctx: &Ctx, s: &meridian_core::types::Store) -> Result<(), Fail> {
    store::save(&ctx.cfg, s).map_err(|e| Fail::new(500, e.to_string()))
}

fn create_portfolio(ctx: &Ctx, b: &Value) -> Out {
    let name = need_str(b, "name")?;
    let mut s = load(ctx)?;
    let p = meridian_core::types::Portfolio {
        id: meridian_core::types::new_id('p'),
        name,
        owner: b
            .get("owner")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        band_pct: b.get("band_pct").and_then(|x| x.as_f64()).unwrap_or(3.0),
        holdings: Vec::new(),
    };
    let id = p.id.clone();
    s.portfolios.push(p);
    commit(ctx, &s)?;
    Ok(json!({ "id": id }))
}

fn patch_portfolio(ctx: &Ctx, id: &str, b: &Value) -> Out {
    let mut s = load(ctx)?;
    if b.get("delete").and_then(|d| d.as_bool()).unwrap_or(false) {
        let before = s.portfolios.len();
        s.portfolios.retain(|p| p.id != id);
        if s.portfolios.len() == before {
            return Err(Fail::new(404, "no such portfolio"));
        }
        commit(ctx, &s)?;
        return Ok(json!({ "ok": true }));
    }
    let p = s
        .portfolios
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;
    if b.get("name").is_some() {
        p.name = need_str(b, "name")?;
    }
    if let Some(o) = b.get("owner").and_then(|x| x.as_str()) {
        p.owner = o.to_string();
    }
    if b.get("band_pct").is_some() {
        let band = need_f64(b, "band_pct")?;
        if !(0.0..=100.0).contains(&band) {
            return Err(Fail::new(400, "band_pct must be between 0 and 100"));
        }
        p.band_pct = band;
    }
    commit(ctx, &s)?;
    Ok(json!({ "ok": true }))
}

fn put_holding(ctx: &Ctx, b: &Value) -> Out {
    let pid = need_str(b, "portfolio_id")?;
    let ticker = need_str(b, "ticker")?.to_uppercase();
    let shares = need_f64(b, "shares")?;
    let cost = need_f64(b, "cost_basis")?;
    let target = need_f64(b, "target_pct")?;
    // ponytail: no shorts, no negative cost. Both are real instruments and neither is something
    // this app models; accepting them would make every weight and drift figure nonsense.
    if shares < 0.0 {
        return Err(Fail::new(400, "shares cannot be negative"));
    }
    if cost < 0.0 {
        return Err(Fail::new(400, "cost_basis cannot be negative"));
    }
    if !(0.0..=100.0).contains(&target) {
        return Err(Fail::new(400, "target_pct must be between 0 and 100"));
    }
    let mut s = load(ctx)?;
    let base = s.base_currency.clone();
    let p = s
        .portfolios
        .iter_mut()
        .find(|p| p.id == pid)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;
    let id = b.get("id").and_then(|x| x.as_str()).map(str::to_string);
    let h = meridian_core::types::Holding {
        id: id
            .clone()
            .unwrap_or_else(|| meridian_core::types::new_id('h')),
        ticker,
        name: b
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        cls: b
            .get("cls")
            .and_then(|x| x.as_str())
            .unwrap_or("Uncategorised")
            .to_string(),
        shares,
        cost_basis: cost,
        cost_currency: b
            .get("cost_currency")
            .and_then(|x| x.as_str())
            .unwrap_or(&base)
            .to_uppercase(),
        target_pct: target,
    };
    let out = json!({ "id": h.id });
    match p.holdings.iter_mut().find(|x| Some(&x.id) == id.as_ref()) {
        Some(existing) => *existing = h,
        None => p.holdings.push(h),
    }
    commit(ctx, &s)?;
    Ok(out)
}

fn delete_holding(ctx: &Ctx, id: &str) -> Out {
    let mut s = load(ctx)?;
    let mut hit = false;
    for p in &mut s.portfolios {
        let before = p.holdings.len();
        p.holdings.retain(|h| h.id != id);
        hit |= p.holdings.len() != before;
    }
    if !hit {
        return Err(Fail::new(404, "no such holding"));
    }
    commit(ctx, &s)?;
    Ok(json!({ "ok": true }))
}

/// Refetch every held symbol and every fx pair the store needs.
///
/// A symbol that fails keeps whatever was cached, so a partial outage costs freshness, not data.
/// The failures come back by name so the page can say which rows it could not price.
fn refresh(ctx: &Ctx, force: bool) -> Out {
    // Taken before anything is read, so a second refresh arriving mid-flight waits here rather
    // than starting its own round of requests against the same symbols.
    let _flight = ctx.refreshing.lock().unwrap_or_else(|e| e.into_inner());
    let s = load(ctx)?;
    let mut wanted: Vec<String> = s
        .portfolios
        .iter()
        .flat_map(|p| p.holdings.iter().map(|h| h.ticker.clone()))
        .collect();
    wanted.sort();
    wanted.dedup();
    let ttl = if force { 0 } else { quotes::REFRESH_TTL };
    let now = quotes::now();
    let mut cache = ctx.cache.lock().expect("cache lock").clone();

    let mut failed = Vec::new();
    let stale = quotes::needs_fetch(&cache, &wanted, ttl, now);
    // What was actually asked of Yahoo, not what the portfolio holds. Reporting the holding count
    // here would claim sixteen requests on a refresh that made none.
    let mut fetched = 0usize;
    let cached = wanted.len() - stale.len();
    for (sym, q) in quotes::fetch_many(&stale) {
        match q {
            Some(q) => {
                cache.insert(sym, q);
                fetched += 1;
            }
            None => failed.push(sym),
        }
    }
    // Which fx pairs are needed is only knowable once the quotes say which currencies are in play.
    let mut pairs: Vec<String> = cache
        .values()
        .filter_map(|q| quotes::fx_symbol(&q.currency, &s.base_currency))
        .collect();
    for h in s.portfolios.iter().flat_map(|p| p.holdings.iter()) {
        if let Some(p) = quotes::fx_symbol(&h.cost_currency, &s.base_currency) {
            pairs.push(p);
        }
    }
    pairs.sort();
    pairs.dedup();
    for (pair, q) in quotes::fetch_many(&quotes::needs_fetch(&cache, &pairs, ttl, now)) {
        if let Some(q) = q {
            cache.insert(pair, q);
        }
    }
    // Completion order is arbitrary; the page lists these to the user.
    failed.sort();
    let _ = quotes::save(&ctx.cfg, &cache);
    *ctx.cache.lock().expect("cache lock") = cache;
    Ok(json!({ "fetched": fetched, "cached": cached, "failed": failed }))
}

/// The trade list a rebalance would need. Its own route rather than part of /api/state because
/// it takes a cash figure the user types, and recomputing every portfolio's trades on every poll
/// of the dashboard would be work nobody asked for.
fn trades(ctx: &Ctx, pid: &str, cash: f64) -> Out {
    let s = store::load(&ctx.cfg).map_err(|e| Fail::new(500, e.to_string()))?;
    let p = s
        .portfolios
        .iter()
        .find(|p| p.id == pid)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;
    let cache = ctx.cache.lock().expect("cache lock").clone();
    let v = calc::view_portfolio(p, &s.base_currency, &cache);
    match calc::trades(&v, cash) {
        Ok(t) => Ok(json!({ "trades": t })),
        // 409: the request is well formed, the portfolio is not ready for it. The page shows this
        // message verbatim, so it must read as a sentence.
        Err(e) => Err(Fail::new(409, e.to_string())),
    }
}

/// Daily bars for a set of symbols, plus the fx series they need, fetching only what is stale.
///
/// Shared by every screen that thinks in history. Which fx pairs are wanted is only knowable
/// once the price histories say which currencies are in play, which is why this is one function
/// and not two.
fn histories_for(
    ctx: &Ctx,
    wanted: &[String],
    base: &str,
) -> (
    HashMap<String, history::Series>,
    HashMap<String, history::Series>,
) {
    let fetch_into = |syms: &[String]| -> HashMap<String, history::Series> {
        let mut out: HashMap<String, history::Series> = HashMap::new();
        let mut stale = Vec::new();
        for sym in syms {
            match history::load(&ctx.cfg, sym) {
                Some(series) if history::is_current(&series) => {
                    out.insert(sym.clone(), series);
                }
                _ => stale.push(sym.clone()),
            }
        }
        for (sym, got) in history::fetch_many(&stale) {
            if let Some(series) = got {
                let _ = history::save(&ctx.cfg, &sym, &series);
                out.insert(sym, series);
            }
        }
        out
    };

    let loaded = fetch_into(wanted);
    let mut pairs: Vec<String> = loaded
        .values()
        .filter_map(|series| quotes::fx_symbol(&series.currency, base))
        .collect();
    pairs.sort();
    pairs.dedup();
    let fx = fetch_into(&pairs);
    (loaded, fx)
}

/// Weights the covariance argues for, over the holdings there is history to measure.
fn optimize_route(ctx: &Ctx, pid: &str, method: &str) -> Out {
    let s = load(ctx)?;
    let p = s
        .portfolios
        .iter()
        .find(|p| p.id == pid)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;
    let mut wanted: Vec<String> = p.holdings.iter().map(|h| h.ticker.clone()).collect();
    wanted.sort();
    wanted.dedup();
    let (loaded, fx) = histories_for(ctx, &wanted, &s.base_currency);
    let out = optimize::suggest(p, &s.base_currency, &loaded, &fx, method);
    serde_json::to_value(&out).map_err(|e| Fail::new(500, e.to_string()))
}

/// Write target weights, and nothing else.
///
/// Deliberately not part of the holding endpoint: applying an optimizer's proposal must not be
/// able to touch a share count or a cost basis, whatever the payload says.
fn set_targets(ctx: &Ctx, b: &Value) -> Out {
    let pid = b.get("portfolio").and_then(|x| x.as_str()).unwrap_or("");
    let targets = b
        .get("targets")
        .and_then(|x| x.as_array())
        .ok_or_else(|| Fail::new(400, "targets must be an array"))?;

    let mut by_id: HashMap<String, f64> = HashMap::new();
    for t in targets {
        let id = t
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Fail::new(400, "every target needs an id"))?;
        let pct = t
            .get("pct")
            .and_then(Value::as_f64)
            .ok_or_else(|| Fail::new(400, "every target needs a pct"))?;
        if !(0.0..=100.0).contains(&pct) {
            return Err(Fail::new(400, "pct must be between 0 and 100"));
        }
        by_id.insert(id.to_string(), pct);
    }

    let mut s = load(ctx)?;
    let p = s
        .portfolios
        .iter_mut()
        .find(|p| p.id == pid)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;

    // Every holding or none. A partial write would leave targets summing to something arbitrary,
    // and Rebalance would then refuse to do anything with the portfolio it just changed.
    if by_id.len() != p.holdings.len() || p.holdings.iter().any(|h| !by_id.contains_key(&h.id)) {
        return Err(Fail::new(
            400,
            "targets must cover every holding exactly once",
        ));
    }
    let total: f64 = by_id.values().sum();
    if (total - 100.0).abs() > 0.05 {
        return Err(Fail::new(
            400,
            format!("targets sum to {total:.2}%, not 100%"),
        ));
    }
    for h in &mut p.holdings {
        h.target_pct = by_id[&h.id];
    }
    commit(ctx, &s)?;
    Ok(json!({ "written": by_id.len() }))
}

/// How often the alert loop looks. Fifteen minutes is frequent enough to matter and rare enough
/// that Yahoo's unofficial endpoints do not notice.
const ALERT_EVERY: Duration = Duration::from_secs(15 * 60);

/// Which rules were firing at the last check.
fn load_firing(ctx: &Ctx) -> Vec<String> {
    std::fs::read(ctx.cfg.alert_state_path())
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save_firing(ctx: &Ctx, keys: &[String]) {
    if let Ok(raw) = serde_json::to_vec(keys) {
        let _ = std::fs::write(ctx.cfg.alert_state_path(), raw);
    }
}

/// Everything currently true, from the same figures the Dashboard shows.
///
/// Returns the firings rather than sending them, so the loop and the test button share one
/// evaluation and the rules stay testable without a network.
fn current_firings(ctx: &Ctx) -> Result<(alerts::Alerts, Vec<alerts::Firing>), Fail> {
    let (s, views, drift, age) = snapshot(ctx)?;
    let mut firing = alerts::evaluate(&s.alerts, &views, &drift, age);
    // Rules reach the phone through the same loop and the same once-only tracking. They are
    // gated on the same switch: a configuration with nowhere to send has nowhere to send these
    // either, and firing into the void would still consume the one notification.
    if s.alerts.live() {
        firing.extend(rules::firings(
            &rules::evaluate(&s.rules, &views, &drift),
            &s.rules,
        ));
    }
    Ok((s.alerts, firing))
}

/// The store, the views, the drift rows and the age of the oldest quote, computed once.
///
/// Three callers need the same four things, and computing them twice in one request is how two
/// screens start disagreeing about what is true.
type Snapshot = (
    meridian_core::types::Store,
    Vec<calc::PortfolioView>,
    HashMap<String, Vec<calc::DriftRow>>,
    i64,
);

fn snapshot(ctx: &Ctx) -> Result<Snapshot, Fail> {
    let s = load(ctx)?;
    let cache = ctx.cache.lock().expect("cache lock").clone();
    let now = quotes::now();
    let oldest = s
        .portfolios
        .iter()
        .flat_map(|p| p.holdings.iter())
        .filter_map(|h| cache.get(&h.ticker))
        .map(|q| q.ts)
        .min();
    let views: Vec<calc::PortfolioView> = s
        .portfolios
        .iter()
        .map(|p| calc::view_portfolio(p, &s.base_currency, &cache))
        .collect();
    let drift: HashMap<String, Vec<calc::DriftRow>> = views
        .iter()
        .map(|v| (v.id.clone(), calc::drift(v)))
        .collect();
    let age = oldest.map(|t| now - t).unwrap_or(0);
    Ok((s, views, drift, age))
}

/// Every rule currently true, for the Rules screen.
///
/// Unlike the alert path this does not care whether alerts are configured: a rule is a thing to
/// look at on a screen first, and a notification second.
fn rule_hits(ctx: &Ctx) -> Out {
    let (s, views, drift, _) = snapshot(ctx)?;
    let hits = rules::evaluate(&s.rules, &views, &drift);
    serde_json::to_value(&hits).map_err(|e| Fail::new(500, e.to_string()))
}

fn read_rules(ctx: &Ctx) -> Out {
    let s = load(ctx)?;
    serde_json::to_value(&s.rules).map_err(|e| Fail::new(500, e.to_string()))
}

/// Replace the rule list wholesale, the way the alert configuration is replaced.
fn write_rules(ctx: &Ctx, b: &Value) -> Out {
    let mut list: Vec<rules::Rule> =
        serde_json::from_value(b.clone()).map_err(|e| Fail::new(400, e.to_string()))?;
    for r in &mut list {
        if r.id.is_empty() {
            r.id = meridian_core::types::new_id('r');
        }
        r.ticker = r.ticker.trim().to_uppercase();
        r.name = r.name.trim().to_string();
        if r.field == rules::Field::ClassWeight && r.cls.trim().is_empty() {
            return Err(Fail::new(400, "a class rule needs a class"));
        }
        if !r.value.is_finite() {
            return Err(Fail::new(400, "a rule needs a number to compare against"));
        }
    }
    let mut s = load(ctx)?;
    s.rules = list.clone();
    commit(ctx, &s)?;
    serde_json::to_value(&list).map_err(|e| Fail::new(500, e.to_string()))
}

/// Check, send what is new, remember what is still true.
fn run_alerts(ctx: &Ctx) -> Result<usize, Fail> {
    let (cfg, firing) = current_firings(ctx)?;
    if !cfg.live() {
        return Ok(0);
    }
    let (fresh, keys) = alerts::newly_firing(&firing, &load_firing(ctx));
    for f in &fresh {
        // A failed send is not a reason to stop: the next rule may reach the phone, and the state
        // is written either way so a broken topic cannot queue up a hundred messages.
        if let Err(e) = alerts::notify(&cfg, &f.text) {
            eprintln!("meridian: alert not sent: {e}");
        }
    }
    save_firing(ctx, &keys);
    Ok(fresh.len())
}

fn read_alerts(ctx: &Ctx) -> Out {
    let s = load(ctx)?;
    // A store written before these rules existed carries zero, and the evaluator reads zero as
    // "use the default". The screen would show the zero, which reads as "every move fires".
    let cfg = alerts::Alerts {
        big_move_pct: s.alerts.threshold(),
        portfolio_move_pct: s.alerts.portfolio_threshold(),
        ..s.alerts
    };
    serde_json::to_value(&cfg).map_err(|e| Fail::new(500, e.to_string()))
}

/// Replace the alert configuration wholesale.
fn write_alerts(ctx: &Ctx, b: &Value) -> Out {
    let mut cfg: alerts::Alerts =
        serde_json::from_value(b.clone()).map_err(|e| Fail::new(400, e.to_string()))?;
    cfg.topic = cfg.topic.trim().to_string();
    cfg.server = cfg.server.trim().to_string();
    // A topic with a slash in it would address a different topic than the one shown on screen.
    if cfg.topic.contains(['/', '?', '#', ' ']) {
        return Err(Fail::new(400, "a topic cannot contain spaces or / ? #"));
    }
    if !cfg.server.is_empty() && !cfg.server.starts_with("https://") {
        // Plain http would put the alert text, and the topic, on the wire in clear.
        return Err(Fail::new(400, "the server must be https"));
    }
    for l in &mut cfg.levels {
        if l.id.is_empty() {
            l.id = meridian_core::types::new_id('l');
        }
        l.ticker = l.ticker.trim().to_uppercase();
        if l.price <= 0.0 {
            return Err(Fail::new(400, "a level price must be above zero"));
        }
    }
    let mut s = load(ctx)?;
    s.alerts = cfg.clone();
    commit(ctx, &s)?;
    serde_json::to_value(&cfg).map_err(|e| Fail::new(500, e.to_string()))
}

/// Send one notification now, so the topic can be proven to reach the phone.
fn test_alert(ctx: &Ctx) -> Out {
    let s = load(ctx)?;
    if !s.alerts.live() {
        return Err(Fail::new(400, "alerts are off, or no topic is set"));
    }
    alerts::notify(
        &s.alerts,
        "Test from Meridian. Alerts are reaching this phone.",
    )
    .map_err(|e| Fail::new(502, e))?;
    Ok(json!({ "sent": true }))
}

/// What would fire right now, without sending anything.
fn preview_alerts(ctx: &Ctx) -> Out {
    let (_, firing) = current_firings(ctx)?;
    serde_json::to_value(&firing).map_err(|e| Fail::new(500, e.to_string()))
}

/// Search a market for a basket of n, choosing on the earlier part of the window only.
///
/// The first call for a market fetches a history per listing, which is slow and then cached.
/// Everything after that is arithmetic.
fn discover_route(ctx: &Ctx, q: &HashMap<String, String>) -> Out {
    let s = load(ctx)?;
    let market = q.get("market").map(String::as_str).unwrap_or("");
    let n: usize = q.get("n").and_then(|x| x.parse().ok()).unwrap_or(5);
    let top_k: usize = q.get("top_k").and_then(|x| x.parse().ok()).unwrap_or(30);
    let cost_bps: f64 = q
        .get("cost_bps")
        .and_then(|x| x.parse().ok())
        .unwrap_or(discover::COST_BPS);
    let method = q.get("method").map(String::as_str).unwrap_or("minvar");
    let years: f64 = q.get("years").and_then(|x| x.parse().ok()).unwrap_or(5.0);
    if !(2.0..=10.0).contains(&years) {
        return Err(Fail::new(400, "years must be between 2 and 10"));
    }
    let horizon: f64 = q.get("horizon").and_then(|x| x.parse().ok()).unwrap_or(1.0);
    if !(0.25..=2.0).contains(&horizon) {
        return Err(Fail::new(400, "horizon must be between 0.25 and 2 years"));
    }
    if !(1..=12).contains(&n) {
        return Err(Fail::new(400, "n must be between 1 and 12"));
    }
    if !(0.0..=500.0).contains(&cost_bps) {
        return Err(Fail::new(400, "cost_bps must be between 0 and 500"));
    }

    let listed = universe::in_market(market);
    if listed.is_empty() {
        return Err(Fail::new(404, "no such market"));
    }
    let (loaded, fx) = histories_for(ctx, &listed, &s.base_currency);
    // Calendar days, not trading days: this is a date to compare bar labels against, and 365.25
    // keeps a five-year window from drifting a day per leap year.
    let since = history::day(chrono::Utc::now().timestamp() - (years * 365.25 * 86_400.0) as i64);
    let (m, unusable) = optimize::build_since(&listed, &s.base_currency, &loaded, &fx, &since);
    let out = discover::search(
        &m,
        unusable,
        listed.len(),
        n,
        method,
        top_k,
        cost_bps,
        horizon,
    );
    serde_json::to_value(&out).map_err(|e| Fail::new(500, e.to_string()))
}

/// What today's allocation would have done, day by day.
///
/// Histories are fetched once and kept: daily bars change once a day, so a chart drawn twice in a
/// session costs nothing the second time. A symbol whose stored series already reaches yesterday
/// is left alone.
fn history_route(ctx: &Ctx, pid: &str, benchmark: &str) -> Out {
    let s = load(ctx)?;
    let p = s
        .portfolios
        .iter()
        .find(|p| p.id == pid)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;

    let bench = benchmark.trim().to_uppercase();
    let mut wanted: Vec<String> = p.holdings.iter().map(|h| h.ticker.clone()).collect();
    if !bench.is_empty() {
        wanted.push(bench.clone());
    }
    wanted.sort();
    wanted.dedup();

    let (loaded, fx) = histories_for(ctx, &wanted, &s.base_currency);

    let out = calc::allocation_history(p, &s.base_currency, &loaded, &fx);
    if bench.is_empty() {
        return serde_json::to_value(&out).map_err(|e| Fail::new(500, e.to_string()));
    }

    // A benchmark is one share of one symbol, so it goes through the same function the portfolio
    // does: the same forward fill, the same fx conversion into base currency, the same refusal to
    // value anything it has no rate for. A second implementation of that would drift from this one.
    let one = meridian_core::types::Portfolio {
        id: String::new(),
        name: bench.clone(),
        owner: String::new(),
        band_pct: 0.0,
        holdings: vec![meridian_core::types::Holding {
            id: String::new(),
            ticker: bench.clone(),
            name: bench.clone(),
            cls: String::new(),
            shares: 1.0,
            cost_basis: 0.0,
            cost_currency: s.base_currency.clone(),
            target_pct: 0.0,
        }],
    };
    let b = calc::allocation_history(&one, &s.base_currency, &loaded, &fx);
    let (points, bench_points) = calc::align(&out.points, &b.points);
    serde_json::to_value(json!({
        "points": points,
        "missing": out.missing,
        "benchmark": { "symbol": bench, "points": bench_points },
    }))
    .map_err(|e| Fail::new(500, e.to_string()))
}

/// What a broker export would do to a portfolio. Writes nothing.
fn import_preview(ctx: &Ctx, pid: &str, file: &[u8]) -> Out {
    let s = load(ctx)?;
    let p = s
        .portfolios
        .iter()
        .find(|p| p.id == pid)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;
    // 422: the file arrived intact and is not a positions export. The page prints this verbatim,
    // so it has to name what is wrong with the file rather than blame the request.
    let rows = import::parse(file).map_err(|e| Fail::new(422, e.to_string()))?;
    let plan = import::plan(&rows, p, &s.aliases);
    serde_json::to_value(&plan).map_err(|e| Fail::new(500, e.to_string()))
}

/// Symbols that might be the fund a broker calls `name`, best first.
///
/// Always global scope: the export is whatever the user actually owns, and a EUR ETF listed in
/// Frankfurt is invisible in the scandinavia scope the rest of the app may be running in.
fn import_candidates(name: &str, currency: &str, last: Option<f64>) -> Out {
    if name.trim().is_empty() {
        return Ok(json!({ "candidates": [] }));
    }
    // Longest name first, stopping at the first form that answers: the most specific search that
    // works is the one whose hits are used.
    let hits = import::search_terms(name)
        .iter()
        .find_map(|t| {
            let h = quotes::search(t, meridian_core::config::Scope::Global);
            (!h.is_empty()).then_some(h)
        })
        .unwrap_or_default();

    // Yahoo's search does not state a currency, so the quotes have to be fetched to learn it.
    let symbols: Vec<String> = hits
        .iter()
        .take(quotes::MAX_PARALLEL)
        .map(|h| h.symbol.clone())
        .collect();
    let priced: Vec<(String, meridian_core::types::Quote)> = quotes::fetch_many(&symbols)
        .into_iter()
        .filter_map(|(s, q)| q.map(|q| (s, q)))
        .collect();
    // Ranked by the two things the export knows about the listing the user actually holds: its
    // currency, and the price the broker printed for it. Nothing is hidden, because an export that
    // omits Valuta would otherwise leave nothing to choose from.
    let mut ranked: Vec<(bool, Option<f64>, &String, &meridian_core::types::Quote)> = priced
        .iter()
        .map(|(sym, q)| {
            (
                q.currency.eq_ignore_ascii_case(currency),
                import::price_gap(q.price, last),
                sym,
                q,
            )
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0).then(
            a.1.unwrap_or(f64::MAX)
                .partial_cmp(&b.1.unwrap_or(f64::MAX))
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    let out: Vec<Value> = ranked
        .iter()
        .map(|(currency_ok, gap, sym, q)| {
            let hit = hits.iter().find(|h| h.symbol == **sym);
            json!({
                "symbol": sym,
                "name": hit.map(|h| h.name.clone()).unwrap_or_default(),
                "exchange": hit.map(|h| h.exchange.clone()).unwrap_or_default(),
                "currency": q.currency,
                "price": q.price,
                "currency_ok": currency_ok,
                // The broker's own price says this is the line, not merely a plausible one.
                "exact": *currency_ok && gap.is_some_and(|g| g < import::PRICE_MATCH),
            })
        })
        .collect();
    Ok(json!({ "candidates": out }))
}

/// Write the rows the user approved, and remember the names they matched.
fn import_apply(ctx: &Ctx, b: &Value) -> Out {
    let pid = need_str(b, "portfolio_id")?;
    let rows = b
        .get("rows")
        .and_then(|r| r.as_array())
        .ok_or_else(|| Fail::new(400, "rows must be an array"))?;
    let mut s = load(ctx)?;
    let base = s.base_currency.clone();

    // Learned before the rows are written, so a failure partway through still leaves the matching
    // knowledge behind rather than making the user repeat it.
    if let Some(a) = b.get("aliases").and_then(|a| a.as_object()) {
        for (name, ticker) in a {
            if let Some(t) = ticker.as_str() {
                // Normalised here rather than in the browser: the lookup that reads these keys
                // is in core, and two implementations of the same normalisation drift.
                s.aliases.insert(import::norm(name), t.to_uppercase());
            }
        }
    }

    let p = s
        .portfolios
        .iter_mut()
        .find(|p| p.id == pid)
        .ok_or_else(|| Fail::new(404, "no such portfolio"))?;
    let mut applied = 0;
    for r in rows {
        let ticker = need_str(r, "ticker")?.to_uppercase();
        let shares = need_f64(r, "shares")?;
        let cost = need_f64(r, "cost_basis")?;
        if shares < 0.0 || cost < 0.0 {
            return Err(Fail::new(
                400,
                format!("{ticker}: shares and cost cannot be negative"),
            ));
        }
        let name = r
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let currency = r
            .get("cost_currency")
            .and_then(|x| x.as_str())
            .unwrap_or(&base)
            .to_uppercase();
        match p.holdings.iter_mut().find(|h| h.ticker == ticker) {
            Some(h) => {
                h.shares = shares;
                h.cost_basis = cost;
                h.cost_currency = currency;
                // Name, class and target are the user's, not the broker's: an import updates the
                // numbers it knows and leaves the ones it does not.
            }
            None => p.holdings.push(meridian_core::types::Holding {
                id: meridian_core::types::new_id('h'),
                ticker,
                name,
                cls: "Uncategorised".to_string(),
                shares,
                cost_basis: cost,
                cost_currency: currency,
                // The export has no target allocation. Zero means Rebalance keeps refusing until
                // the user sets one, which is the correct refusal rather than an invented target.
                target_pct: 0.0,
            }),
        }
        applied += 1;
    }
    commit(ctx, &s)?;
    Ok(json!({ "applied": applied }))
}

fn search(ctx: &Ctx, query: &str) -> Out {
    if query.trim().is_empty() {
        return Ok(json!({ "hits": [] }));
    }
    Ok(json!({ "hits": quotes::search(query, ctx.cfg.scope) }))
}

fn handle(ctx: &Ctx, token: &str, req: Request) {
    let url = req.url().to_string();
    let (path, _raw_query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let path = path.to_string();

    // A preflight carries no token by definition, so it is answered before the guard.
    if req.method() == &Method::Options {
        return send(req, 204, Vec::new(), "text/plain");
    }
    if path == "/api/health" {
        return send_json(req, 200, json!({ "version": VERSION, "ok": true }));
    }
    if !same_token(&header(&req, "X-Meridian-Token"), token) {
        return send_json(req, 401, json!({ "error": "bad token" }));
    }
    let query: std::collections::HashMap<String, String> = _raw_query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            let dec = |s: &str| {
                urlencoding::decode(&s.replace('+', " "))
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| s.to_string())
            };
            (dec(k), dec(v))
        })
        .collect();

    let mut req = req;
    let out: Out = match (req.method().as_str(), path.as_str()) {
        ("GET", "/api/state") => state(ctx),
        ("GET", "/api/search") => search(ctx, query.get("q").map(String::as_str).unwrap_or("")),
        ("GET", "/api/alerts") => read_alerts(ctx),
        ("POST", "/api/alerts") => body(&mut req).and_then(|b| write_alerts(ctx, &b)),
        ("POST", "/api/alerts/test") => test_alert(ctx),
        ("GET", "/api/alerts/preview") => preview_alerts(ctx),
        ("GET", "/api/rules") => read_rules(ctx),
        ("POST", "/api/rules") => body(&mut req).and_then(|b| write_rules(ctx, &b)),
        ("GET", "/api/rules/hits") => rule_hits(ctx),
        ("GET", "/api/markets") => Ok(json!({ "markets": universe::SCOPES })),
        ("GET", "/api/discover") => discover_route(ctx, &query),
        ("GET", "/api/optimize") => optimize_route(
            ctx,
            query.get("portfolio").map(String::as_str).unwrap_or(""),
            query.get("method").map(String::as_str).unwrap_or("minvar"),
        ),
        ("GET", "/api/history") => history_route(
            ctx,
            query.get("portfolio").map(String::as_str).unwrap_or(""),
            query.get("benchmark").map(String::as_str).unwrap_or(""),
        ),
        ("GET", "/api/trades") => trades(
            ctx,
            query.get("portfolio").map(String::as_str).unwrap_or(""),
            query
                .get("cash")
                .and_then(|c| c.parse().ok())
                .unwrap_or(0.0),
        ),
        // force=1 is the Refresh button: the user asking for prices now overrides the ttl.
        ("POST", "/api/refresh") => refresh(ctx, query.contains_key("force")),
        ("POST", "/api/portfolio") => body(&mut req).and_then(|b| create_portfolio(ctx, &b)),
        ("POST", "/api/holding") => body(&mut req).and_then(|b| put_holding(ctx, &b)),
        ("POST", "/api/targets") => body(&mut req).and_then(|b| set_targets(ctx, &b)),
        ("POST", "/api/import/preview") => {
            let pid = query.get("portfolio").cloned().unwrap_or_default();
            raw_body(&mut req).and_then(|f| import_preview(ctx, &pid, &f))
        }
        ("GET", "/api/import/candidates") => import_candidates(
            query.get("name").map(String::as_str).unwrap_or(""),
            query.get("currency").map(String::as_str).unwrap_or(""),
            query.get("last").and_then(|l| l.parse().ok()),
        ),
        ("POST", "/api/import/apply") => body(&mut req).and_then(|b| import_apply(ctx, &b)),
        ("PATCH", p) if tail(p, "/api/portfolio/").is_some() => {
            let id = tail(p, "/api/portfolio/")
                .expect("checked just above")
                .to_string();
            body(&mut req).and_then(|b| patch_portfolio(ctx, &id, &b))
        }
        ("DELETE", p) if tail(p, "/api/holding/").is_some() => {
            delete_holding(ctx, tail(p, "/api/holding/").expect("checked just above"))
        }
        _ => Err(Fail::new(404, "not found")),
    };
    match out {
        Ok(v) => send_json(req, 200, v),
        Err(Fail(c, m)) => send_json(req, c, json!({ "error": m })),
    }
}

/// Serve on 127.0.0.1:`port` (0 = any free) and return the bound port. The accept loop runs on
/// its own thread, so this returns immediately.
///
/// ponytail: bound to loopback. A hosted deployment sits behind a reverse proxy, which is where
/// TLS and any address other than loopback belong. Change this to 0.0.0.0 only together with TLS.
pub fn serve(cfg: Config, port: u16, token: String) -> std::io::Result<u16> {
    let server =
        Server::http(("127.0.0.1", port)).map_err(|e| std::io::Error::other(e.to_string()))?;
    let bound = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(port);
    let ctx = Ctx {
        cache: Arc::new(Mutex::new(quotes::load(&cfg))),
        refreshing: Arc::new(Mutex::new(())),
        cfg,
    };
    // The alert loop lives here rather than in the desktop app because the app is shut at
    // exactly the moment an alert matters.
    let watcher = ctx.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(ALERT_EVERY);
        // Prices first: rules read the cache, and a rule evaluated against yesterday's quotes
        // would fire on yesterday's news.
        let _ = refresh(&watcher, false);
        if let Err(e) = run_alerts(&watcher) {
            eprintln!("meridian: alert check failed: {}", e.1);
        }
    });

    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let (ctx, token) = (ctx.clone(), token.clone());
            std::thread::spawn(move || handle(&ctx, &token, req));
        }
    });
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use meridian_core::config::Scope;

    fn up() -> (tempfile::TempDir, u16, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), Scope::Scandinavia);
        let token = new_token();
        let port = serve(cfg, 0, token.clone()).expect("serve");
        (dir, port, token)
    }

    fn get(port: u16, path: &str, token: Option<&str>) -> (u16, String) {
        let url = format!("http://127.0.0.1:{port}{path}");
        let mut req = ureq::get(&url);
        if let Some(t) = token {
            req = req.header("X-Meridian-Token", t);
        }
        match req.call() {
            Ok(mut r) => (
                r.status().as_u16(),
                r.body_mut().read_to_string().unwrap_or_default(),
            ),
            Err(ureq::Error::StatusCode(c)) => (c, String::new()),
            Err(e) => panic!("request failed: {e}"),
        }
    }

    #[test]
    fn health_needs_no_token_because_it_reveals_nothing() {
        let (_d, port, _t) = up();
        let (code, body) = get(port, "/api/health", None);
        assert_eq!(code, 200);
        assert!(body.contains("version"), "got {body}");
    }

    #[test]
    fn a_request_without_a_token_is_rejected() {
        let (_d, port, _t) = up();
        assert_eq!(get(port, "/api/state", None).0, 401);
    }

    #[test]
    fn a_request_with_the_wrong_token_is_rejected() {
        let (_d, port, _t) = up();
        assert_eq!(get(port, "/api/state", Some("nope")).0, 401);
    }

    #[test]
    fn state_on_a_fresh_store_is_empty_but_well_formed() {
        let (_d, port, token) = up();
        let (code, body) = get(port, "/api/state", Some(&token));
        assert_eq!(code, 200);
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(v["base_currency"], "NOK");
        assert!(v["portfolios"].as_array().expect("array").is_empty());
        assert_eq!(v["scope"], "scandinavia");
    }

    #[test]
    fn every_reply_carries_cors_headers_so_the_app_can_be_a_different_origin() {
        let (_d, port, token) = up();
        let url = format!("http://127.0.0.1:{port}/api/state");
        let r = ureq::get(&url)
            .header("X-Meridian-Token", &token)
            .call()
            .expect("call");
        assert!(r.headers().get("access-control-allow-origin").is_some());
    }

    #[test]
    fn a_preflight_is_answered_without_a_token() {
        let (_d, port, _t) = up();
        let url = format!("http://127.0.0.1:{port}/api/state");
        // ureq 3 keeps arbitrary methods behind run(), so a preflight is built by hand.
        let req = ureq::http::Request::builder()
            .method("OPTIONS")
            .uri(&url)
            .body(())
            .expect("build");
        let r = ureq::run(req).expect("preflight");
        assert_eq!(r.status().as_u16(), 204);
    }

    fn call(port: u16, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{port}{path}");
        // ureq turns a 4xx into an Error that carries the code and drops the body. A route whose
        // contract IS the wording of its refusal cannot be tested that way, so this agent hands
        // back every status as an ordinary response.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let res = match method {
            "POST" => agent
                .post(&url)
                .header("X-Meridian-Token", token)
                .send_json(body),
            "PATCH" => agent
                .patch(&url)
                .header("X-Meridian-Token", token)
                .send_json(body),
            "DELETE" => agent.delete(&url).header("X-Meridian-Token", token).call(),
            _ => agent.get(&url).header("X-Meridian-Token", token).call(),
        };
        match res {
            Ok(mut r) => (
                r.status().as_u16(),
                r.body_mut().read_json().unwrap_or(Value::Null),
            ),
            Err(e) => panic!("request failed: {e}"),
        }
    }

    fn with_portfolio(port: u16, t: &str) -> String {
        let (_, p) = call(port, "POST", "/api/portfolio", t, json!({"name": "A"}));
        p["id"].as_str().expect("an id").to_string()
    }

    #[test]
    fn a_created_portfolio_comes_back_in_state() {
        let (_d, port, t) = up();
        let (code, v) = call(
            port,
            "POST",
            "/api/portfolio",
            &t,
            json!({"name": "Balanced"}),
        );
        assert_eq!(code, 200);
        let id = v["id"].as_str().expect("an id").to_string();
        assert!(id.starts_with("p_"));
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        assert_eq!(s["portfolios"][0]["name"], "Balanced");
        assert_eq!(s["portfolios"][0]["id"], id);
        assert_eq!(
            s["portfolios"][0]["band_pct"], 3.0,
            "the design's default band"
        );
    }

    #[test]
    fn a_patch_renames_without_touching_the_holdings() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        call(
            port,
            "POST",
            "/api/holding",
            &t,
            json!({
            "portfolio_id": pid, "ticker": "EQNR.OL", "name": "Equinor", "cls": "Equity",
            "shares": 10.0, "cost_basis": 2500.0, "cost_currency": "NOK", "target_pct": 100.0}),
        );
        let (code, _) = call(
            port,
            "PATCH",
            &format!("/api/portfolio/{pid}"),
            &t,
            json!({"name": "B", "band_pct": 5.0}),
        );
        assert_eq!(code, 200);
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        assert_eq!(s["portfolios"][0]["name"], "B");
        assert_eq!(s["portfolios"][0]["band_pct"], 5.0);
        assert_eq!(
            s["portfolios"][0]["holdings"]
                .as_array()
                .expect("arr")
                .len(),
            1
        );
    }

    /// Two holdings, so a partial write has something to be partial about.
    fn with_two_holdings(port: u16, t: &str) -> (String, String, String) {
        let pid = with_portfolio(port, t);
        let add = |ticker: &str, target: f64| {
            json!({
                "portfolio_id": pid, "ticker": ticker, "name": ticker, "cls": "Equity",
                "shares": 10.0, "cost_basis": 1000.0, "cost_currency": "NOK",
                "target_pct": target})
        };
        let (_, a) = call(port, "POST", "/api/holding", t, add("EQNR.OL", 50.0));
        let (_, b) = call(port, "POST", "/api/holding", t, add("AAPL", 50.0));
        (
            pid,
            a["id"].as_str().expect("id").to_string(),
            b["id"].as_str().expect("id").to_string(),
        )
    }

    #[test]
    fn targets_are_written_when_they_cover_everything_and_sum_to_a_hundred() {
        let (_d, port, t) = up();
        let (pid, a, b) = with_two_holdings(port, &t);
        let (code, _) = call(
            port,
            "POST",
            "/api/targets",
            &t,
            json!({"portfolio": pid, "targets": [
                {"id": a, "pct": 30.0}, {"id": b, "pct": 70.0}]}),
        );
        assert_eq!(code, 200);
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        let hs = s["portfolios"][0]["holdings"].as_array().expect("arr");
        let by = |id: &str| -> f64 {
            hs.iter().find(|h| h["id"] == id).expect("holding")["target_pct"]
                .as_f64()
                .expect("pct")
        };
        assert_eq!(by(&a), 30.0);
        assert_eq!(by(&b), 70.0);
    }

    #[test]
    fn a_partial_target_write_is_refused_rather_than_half_applied() {
        // Writing one of two would leave the portfolio summing to something arbitrary, and
        // Rebalance would then refuse to act on the portfolio this endpoint just changed.
        let (_d, port, t) = up();
        let (pid, a, _b) = with_two_holdings(port, &t);
        let (code, err) = call(
            port,
            "POST",
            "/api/targets",
            &t,
            json!({"portfolio": pid, "targets": [{"id": a, "pct": 100.0}]}),
        );
        assert_eq!(code, 400, "{err}");
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        for h in s["portfolios"][0]["holdings"].as_array().expect("arr") {
            assert_eq!(h["target_pct"], 50.0, "nothing was written");
        }
    }

    #[test]
    fn targets_that_do_not_sum_to_a_hundred_are_refused() {
        let (_d, port, t) = up();
        let (pid, a, b) = with_two_holdings(port, &t);
        let (code, err) = call(
            port,
            "POST",
            "/api/targets",
            &t,
            json!({"portfolio": pid, "targets": [
                {"id": a, "pct": 30.0}, {"id": b, "pct": 30.0}]}),
        );
        assert_eq!(code, 400);
        assert!(
            err["error"].as_str().expect("error").contains("60"),
            "the message should say what they did sum to: {err}"
        );
    }

    #[test]
    fn rules_round_trip_and_a_class_rule_without_a_class_is_refused() {
        let (_d, port, t) = up();
        let (code, got) = call(
            port,
            "POST",
            "/api/rules",
            &t,
            json!([{"id": "", "name": "  Too concentrated  ", "enabled": true, "notify": true,
                    "field": "weight", "op": "above", "value": 40.0, "ticker": "eqnr.ol",
                    "cls": ""}]),
        );
        assert_eq!(code, 200, "{got}");
        assert_eq!(got[0]["ticker"], "EQNR.OL", "upper-cased on the way in");
        assert_eq!(got[0]["name"], "Too concentrated", "trimmed");
        assert!(
            got[0]["id"].as_str().is_some_and(|s| !s.is_empty()),
            "a rule gets an id so it can be removed later"
        );

        let (_, back) = call(port, "GET", "/api/rules", &t, Value::Null);
        assert_eq!(back[0]["field"], "weight");
        assert_eq!(back[0]["value"], 40.0);

        // A class rule with no class would silently match nothing for ever.
        let (bad, _) = call(
            port,
            "POST",
            "/api/rules",
            &t,
            json!([{"field": "class_weight", "op": "above", "value": 30.0, "enabled": true}]),
        );
        assert_eq!(bad, 400);

        // Nothing is held, so nothing can be firing, but the screen still gets its list.
        let (hits, body) = call(port, "GET", "/api/rules/hits", &t, Value::Null);
        assert_eq!(hits, 200);
        assert_eq!(body.as_array().map(|a| a.len()), Some(0), "{body}");
    }

    #[test]
    fn alerts_round_trip_and_reject_a_topic_that_would_address_something_else() {
        let (_d, port, t) = up();
        let (code, got) = call(
            port,
            "POST",
            "/api/alerts",
            &t,
            json!({"enabled": true, "topic": "meridian-abc123", "drift": true,
                   "big_move": true, "big_move_pct": 4.0, "portfolio_move": true,
                   "stale": true, "levels": [
                       {"id": "", "ticker": "eqnr.ol", "price": 300.0, "above": true}]}),
        );
        assert_eq!(code, 200, "{got}");
        assert_eq!(
            got["levels"][0]["ticker"], "EQNR.OL",
            "upper-cased on the way in"
        );
        assert!(
            got["levels"][0]["id"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "a level gets an id so it can be removed later"
        );
        let (_, back) = call(port, "GET", "/api/alerts", &t, Value::Null);
        assert_eq!(back["topic"], "meridian-abc123");
        assert_eq!(back["big_move_pct"], 4.0);
        assert_eq!(back["portfolio_move"], true);
        // Sent as nothing, so it comes back as the default rather than as the zero the evaluator
        // would read as "use the default" and the screen would read as "every move fires".
        assert_eq!(
            back["portfolio_move_pct"],
            meridian_core::alerts::DEFAULT_PORTFOLIO_MOVE_PCT
        );

        // A slash would post to a different topic than the one the screen displays.
        let (bad, _) = call(
            port,
            "POST",
            "/api/alerts",
            &t,
            json!({"enabled": true, "topic": "mine/else"}),
        );
        assert_eq!(bad, 400);
        // and plain http would put the alert text and the topic on the wire in clear
        let (insecure, _) = call(
            port,
            "POST",
            "/api/alerts",
            &t,
            json!({"enabled": true, "topic": "mine", "server": "http://ntfy.example"}),
        );
        assert_eq!(insecure, 400);
    }

    #[test]
    fn a_test_alert_refuses_rather_than_posting_nowhere() {
        // No topic means no destination. Sending anyway would hit ntfy.sh with an empty path,
        // and a test that appears to succeed while reaching nobody is worse than an error.
        let (_d, port, t) = up();
        let (code, err) = call(port, "POST", "/api/alerts/test", &t, Value::Null);
        assert_eq!(code, 400, "{err}");
    }

    #[test]
    fn nothing_fires_from_an_empty_store() {
        let (_d, port, t) = up();
        let (code, got) = call(port, "GET", "/api/alerts/preview", &t, Value::Null);
        assert_eq!(code, 200);
        assert!(got.as_array().expect("array").is_empty(), "{got}");
    }

    #[test]
    fn a_portfolio_can_be_deleted() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        let (code, _) = call(
            port,
            "PATCH",
            &format!("/api/portfolio/{pid}"),
            &t,
            json!({"delete": true}),
        );
        assert_eq!(code, 200);
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        assert!(s["portfolios"].as_array().expect("arr").is_empty());
        let (again, _) = call(
            port,
            "PATCH",
            &format!("/api/portfolio/{pid}"),
            &t,
            json!({"delete": true}),
        );
        assert_eq!(again, 404, "deleting it twice is not a silent success");
    }

    #[test]
    fn posting_a_holding_with_an_existing_id_updates_it_rather_than_duplicating() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        let mk = |shares: f64, id: Value| {
            json!({
            "id": id, "portfolio_id": pid, "ticker": "EQNR.OL", "name": "Equinor",
            "cls": "Equity", "shares": shares, "cost_basis": 2500.0,
            "cost_currency": "NOK", "target_pct": 100.0})
        };
        let (_, h) = call(port, "POST", "/api/holding", &t, mk(10.0, Value::Null));
        let hid = h["id"].as_str().expect("id").to_string();
        call(port, "POST", "/api/holding", &t, mk(20.0, json!(hid)));
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        let hs = s["portfolios"][0]["holdings"].as_array().expect("arr");
        assert_eq!(hs.len(), 1, "updated, not duplicated");
        assert_eq!(hs[0]["shares"], 20.0);
    }

    #[test]
    fn a_ticker_is_stored_uppercased_so_the_cache_key_always_matches() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        call(
            port,
            "POST",
            "/api/holding",
            &t,
            json!({
            "portfolio_id": pid, "ticker": "eqnr.ol", "name": "Equinor", "cls": "Equity",
            "shares": 1.0, "cost_basis": 1.0, "cost_currency": "nok", "target_pct": 100.0}),
        );
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        assert_eq!(s["portfolios"][0]["holdings"][0]["ticker"], "EQNR.OL");
    }

    #[test]
    fn deleting_a_holding_removes_only_that_one() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        let add = |ticker: &str| {
            json!({
            "portfolio_id": pid, "ticker": ticker, "name": ticker, "cls": "Equity",
            "shares": 1.0, "cost_basis": 1.0, "cost_currency": "NOK", "target_pct": 50.0})
        };
        let (_, a) = call(port, "POST", "/api/holding", &t, add("EQNR.OL"));
        call(port, "POST", "/api/holding", &t, add("AAPL"));
        let aid = a["id"].as_str().expect("id").to_string();
        let (code, _) = call(
            port,
            "DELETE",
            &format!("/api/holding/{aid}"),
            &t,
            Value::Null,
        );
        assert_eq!(code, 200);
        let (_, s) = call(port, "GET", "/api/state", &t, Value::Null);
        let hs = s["portfolios"][0]["holdings"].as_array().expect("arr");
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0]["ticker"], "AAPL");
    }

    #[test]
    fn deleting_something_that_is_not_there_is_a_404_not_a_silent_ok() {
        let (_d, port, t) = up();
        assert_eq!(
            call(port, "DELETE", "/api/holding/h_nope", &t, Value::Null).0,
            404
        );
    }

    #[test]
    fn a_holding_for_a_portfolio_that_does_not_exist_is_a_404() {
        let (_d, port, t) = up();
        let (code, _) = call(
            port,
            "POST",
            "/api/holding",
            &t,
            json!({
            "portfolio_id": "p_nope", "ticker": "EQNR.OL", "name": "E", "cls": "Equity",
            "shares": 1.0, "cost_basis": 1.0, "cost_currency": "NOK", "target_pct": 50.0}),
        );
        assert_eq!(code, 404);
    }

    #[test]
    fn a_portfolio_name_that_is_blank_is_refused() {
        let (_d, port, t) = up();
        assert_eq!(
            call(port, "POST", "/api/portfolio", &t, json!({"name": "  "})).0,
            400
        );
    }

    #[test]
    fn negative_shares_are_refused_because_this_app_does_not_do_shorts() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        let (code, _) = call(
            port,
            "POST",
            "/api/holding",
            &t,
            json!({
            "portfolio_id": pid, "ticker": "EQNR.OL", "name": "E", "cls": "Equity",
            "shares": -5.0, "cost_basis": 1.0, "cost_currency": "NOK", "target_pct": 50.0}),
        );
        assert_eq!(code, 400);
    }

    #[test]
    fn a_target_outside_zero_to_a_hundred_is_refused() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        let (code, _) = call(
            port,
            "POST",
            "/api/holding",
            &t,
            json!({
            "portfolio_id": pid, "ticker": "EQNR.OL", "name": "E", "cls": "Equity",
            "shares": 1.0, "cost_basis": 1.0, "cost_currency": "NOK", "target_pct": 140.0}),
        );
        assert_eq!(code, 400);
    }

    #[test]
    fn a_shares_value_that_is_not_a_number_is_refused_rather_than_stored_as_nan() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        for bad in [json!("ten"), json!(null)] {
            let (code, _) = call(
                port,
                "POST",
                "/api/holding",
                &t,
                json!({
                "portfolio_id": pid, "ticker": "EQNR.OL", "name": "E", "cls": "Equity",
                "shares": bad, "cost_basis": 1.0, "cost_currency": "NOK", "target_pct": 50.0}),
            );
            assert_eq!(code, 400, "a NaN here would poison every total");
        }
    }

    #[test]
    fn a_search_with_no_query_is_empty_without_reaching_the_network() {
        let (_d, port, t) = up();
        let (code, v) = call(port, "GET", "/api/search?q=", &t, Value::Null);
        assert_eq!(code, 200);
        assert!(v["hits"].as_array().expect("arr").is_empty());
    }

    #[test]
    fn tokens_compare_without_leaking_where_they_differ() {
        assert!(same_token("abc", "abc"));
        assert!(!same_token("abc", "abd"));
        assert!(!same_token("abc", "abcd"));
        assert!(!same_token("", "a"));
    }

    #[test]
    fn a_generated_token_is_long_enough_to_be_worth_guarding() {
        let t = new_token();
        assert_eq!(t.len(), 48, "24 random bytes as hex");
        assert_ne!(t, new_token(), "two calls must not agree");
    }

    #[test]
    fn trades_are_refused_in_words_the_page_can_show_when_nothing_has_a_price() {
        let (_d, port, t) = up();
        let pid = with_portfolio(port, &t);
        call(
            port,
            "POST",
            "/api/holding",
            &t,
            json!({"portfolio_id": pid, "ticker": "EQNR.OL", "name": "E", "cls": "Equity",
                   "shares": 1.0, "cost_basis": 1.0, "cost_currency": "NOK", "target_pct": 100.0}),
        );
        // No quote is cached in a fresh temp store, so this is the NothingPriced path.
        let (code, v) = call(
            port,
            "GET",
            &format!("/api/trades?portfolio={pid}&cash=0"),
            &t,
            Value::Null,
        );
        assert_eq!(code, 409);
        assert!(
            v["error"].as_str().expect("a message").contains("price"),
            "got {v}"
        );
    }

    #[test]
    fn trades_refuse_a_portfolio_that_does_not_exist() {
        let (_d, port, t) = up();
        let (code, _) = call(
            port,
            "GET",
            "/api/trades?portfolio=p_nope&cash=0",
            &t,
            Value::Null,
        );
        assert_eq!(code, 404);
    }

    /// The import routes take a file, not JSON, so they need their own sender.
    fn send_file(port: u16, path: &str, token: &str, file: &[u8]) -> (u16, Value) {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut r = agent
            .post(&format!("http://127.0.0.1:{port}{path}"))
            .header("X-Meridian-Token", token)
            .header("Content-Type", "application/octet-stream")
            .send(file)
            .expect("send");
        (
            r.status().as_u16(),
            r.body_mut().read_json().unwrap_or(Value::Null),
        )
    }

    const NORDNET: &[u8] = include_bytes!("../../core/tests/fixtures/nordnet_positions.csv");

    fn portfolio_with(port: u16, token: &str, holdings: Vec<Value>) -> String {
        let (_, p) = call(
            port,
            "POST",
            "/api/portfolio",
            token,
            json!({ "name": "Test" }),
        );
        let pid = p["id"].as_str().expect("id").to_string();
        for h in holdings {
            let mut h = h;
            h["portfolio_id"] = json!(pid);
            call(port, "POST", "/api/holding", token, h);
        }
        pid
    }

    #[test]
    fn a_preview_says_what_would_change_and_writes_nothing() {
        let (_d, port, t) = up();
        let pid = portfolio_with(port, &t, vec![]);
        let (code, v) = send_file(
            port,
            &format!("/api/import/preview?portfolio={pid}"),
            &t,
            NORDNET,
        );
        assert_eq!(code, 200);
        let changes = v["changes"].as_array().expect("changes");
        assert_eq!(changes.len(), 2);
        // Nothing is held and nothing has been taught, so both rows need a ticker from the user.
        assert_eq!(changes[0]["action"], "unmatched");
        assert_eq!(changes[0]["ticker"], Value::Null);
        assert_eq!(changes[0]["cost_basis"], 25_000.0);
        assert_eq!(changes[0]["currency"], "EUR");

        // The preview must not have written anything.
        let (_, state) = call(port, "GET", "/api/state", &t, Value::Null);
        assert!(state["portfolios"][0]["holdings"]
            .as_array()
            .expect("holdings")
            .is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_positions_export_is_refused_with_a_readable_reason() {
        let (_d, port, t) = up();
        let pid = portfolio_with(port, &t, vec![]);
        let (code, v) = send_file(
            port,
            &format!("/api/import/preview?portfolio={pid}"),
            &t,
            b"just some text\nwith no columns\n",
        );
        assert_eq!(code, 422);
        let msg = v["error"].as_str().expect("error");
        assert!(
            msg.contains("Navn") || msg.contains("positions export"),
            "got {msg}"
        );
    }

    #[test]
    fn applying_an_import_writes_the_holdings_and_remembers_the_names() {
        let (_d, port, t) = up();
        let pid = portfolio_with(port, &t, vec![]);
        let (code, v) = call(
            port,
            "POST",
            "/api/import/apply",
            &t,
            json!({
                "portfolio_id": pid,
                "rows": [{
                    "ticker": "xnas.de",
                    "name": "Xtrackers NASDAQ 100 ETF 1C",
                    "shares": 200.0,
                    "cost_basis": 12_000.0,
                    "cost_currency": "eur",
                }],
                "aliases": { "xtrackers nasdaq 100 etf 1c": "xnas.de" },
            }),
        );
        assert_eq!(code, 200, "got {v}");
        assert_eq!(v["applied"], 1);

        let (_, state) = call(port, "GET", "/api/state", &t, Value::Null);
        let h = &state["portfolios"][0]["holdings"][0];
        assert_eq!(h["ticker"], "XNAS.DE");
        assert_eq!(h["shares"], 200.0);
        // The export carries no target allocation, so an imported holding starts at zero and
        // Rebalance keeps refusing until the user sets one.
        assert_eq!(h["target_pct"], 0.0);

        // The second preview of the same file needs no clicks: the name is known now.
        let (_, v) = send_file(
            port,
            &format!("/api/import/preview?portfolio={pid}"),
            &t,
            NORDNET,
        );
        let learned = v["changes"]
            .as_array()
            .expect("changes")
            .iter()
            .find(|c| c["name"] == "Xtrackers NASDAQ 100 ETF 1C")
            .expect("the taught row");
        assert_eq!(learned["ticker"], "XNAS.DE");
        // The applied numbers are the file's own, so re-importing it is a no-op rather than a
        // second write of the same figures.
        assert_eq!(learned["action"], "unchanged", "got {learned}");
    }

    #[test]
    fn an_import_never_removes_a_holding_the_file_does_not_mention() {
        let (_d, port, t) = up();
        let pid = portfolio_with(
            port,
            &t,
            vec![json!({
                "ticker": "EQNR.OL", "name": "Equinor ASA", "shares": 111.0,
                "cost_basis": 38_000.0, "target_pct": 100.0, "cost_currency": "NOK",
            })],
        );
        let (_, v) = send_file(
            port,
            &format!("/api/import/preview?portfolio={pid}"),
            &t,
            NORDNET,
        );
        assert_eq!(v["absent"], json!(["EQNR.OL"]));

        call(
            port,
            "POST",
            "/api/import/apply",
            &t,
            json!({
                "portfolio_id": pid,
                "rows": [{ "ticker": "XNAS.DE", "shares": 200.0, "cost_basis": 12_000.0 }],
            }),
        );
        let (_, state) = call(port, "GET", "/api/state", &t, Value::Null);
        let held: Vec<&str> = state["portfolios"][0]["holdings"]
            .as_array()
            .expect("holdings")
            .iter()
            .map(|h| h["ticker"].as_str().expect("ticker"))
            .collect();
        assert!(
            held.contains(&"EQNR.OL"),
            "the import deleted an untouched holding: {held:?}"
        );
        assert!(held.contains(&"XNAS.DE"));
    }

    #[test]
    fn an_imported_row_with_negative_shares_is_refused_before_anything_is_written() {
        let (_d, port, t) = up();
        let pid = portfolio_with(port, &t, vec![]);
        let (code, _) = call(
            port,
            "POST",
            "/api/import/apply",
            &t,
            json!({
                "portfolio_id": pid,
                "rows": [{ "ticker": "XNAS.DE", "shares": -5.0, "cost_basis": 100.0 }],
            }),
        );
        assert_eq!(code, 400);
        let (_, state) = call(port, "GET", "/api/state", &t, Value::Null);
        assert!(state["portfolios"][0]["holdings"]
            .as_array()
            .expect("h")
            .is_empty());
    }
}
