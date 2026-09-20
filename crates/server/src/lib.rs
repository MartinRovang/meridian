//! Meridian's JSON API.
//!
//! This crate links no Tauri and no desktop library on purpose: the same code runs inside the
//! desktop app and on a server with nothing installed on it. If a dependency here ever pulls in
//! gtk or webkit, the split this design exists for is gone, and CI's server-isolation job is
//! what notices.
//!
//! Auth is one shared secret in X-Meridian-Token. There is one store per server and one token
//! that opens it. Accounts are a separate project.

use std::io::Read;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use meridian_core::config::Config;
use meridian_core::{calc, quotes, store};

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
}
