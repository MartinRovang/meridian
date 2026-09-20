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
    let out: Out = match (req.method().as_str(), path.as_str()) {
        ("GET", "/api/state") => state(ctx),
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
}
