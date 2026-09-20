//! Rules that fire on your phone, and the delivery that gets them there.
//!
//! Everything that decides is a pure function over figures already computed elsewhere, so the
//! rules are testable without a network and without a clock. Only `notify` touches the wire.
//!
//! One thing shapes the wording of every message: an ntfy topic is public to anyone who knows
//! its name. There is no password on it. So a notification carries a ticker and a percentage and
//! never an amount, a share count or a portfolio total. The app is one tap away for those.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::calc::{DriftRow, PortfolioView};

/// Where notifications go. `ntfy.sh` unless self-hosted.
pub const DEFAULT_SERVER: &str = "https://ntfy.sh";

/// A daily move at or beyond this is worth a phone buzzing, unless told otherwise.
pub const DEFAULT_MOVE_PCT: f64 = 5.0;

/// Quotes older than this mean the figures on screen are not today's.
pub const STALE_SECS: i64 = 6 * 3600;

/// A price to watch, and which side of it matters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub id: String,
    pub ticker: String,
    /// In the ticker's own currency, which is how the user reads a price.
    pub price: f64,
    /// True fires when the price is at or above; false when at or below.
    pub above: bool,
}

/// The whole alert configuration, stored beside the portfolios.
///
/// Every field has a default, because a store written before alerts existed must still parse.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alerts {
    pub enabled: bool,
    /// The ntfy topic. Treat it as a password that is printed on every message.
    pub topic: String,
    pub server: String,
    pub drift: bool,
    pub big_move: bool,
    pub big_move_pct: f64,
    pub stale: bool,
    pub levels: Vec<Level>,
}

impl Alerts {
    pub fn server_url(&self) -> &str {
        if self.server.trim().is_empty() {
            DEFAULT_SERVER
        } else {
            self.server.trim()
        }
    }
    pub fn threshold(&self) -> f64 {
        if self.big_move_pct > 0.0 {
            self.big_move_pct
        } else {
            DEFAULT_MOVE_PCT
        }
    }
    /// Configured well enough to send anything at all.
    pub fn live(&self) -> bool {
        self.enabled && !self.topic.trim().is_empty()
    }
}

/// One rule currently firing.
///
/// `key` identifies the rule, not the occasion: the same breach on the same holding produces the
/// same key every quarter hour, which is what lets the caller notify once and then stay quiet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Firing {
    pub key: String,
    pub text: String,
}

/// Everything currently true, given what the screens already computed.
///
/// `quotes_age_secs` is the age of the oldest quote in the cache. Unpriced holdings are read off
/// the views, where `priced: false` already means every money field is meaningless.
pub fn evaluate(
    cfg: &Alerts,
    views: &[PortfolioView],
    drift: &HashMap<String, Vec<DriftRow>>,
    quotes_age_secs: i64,
) -> Vec<Firing> {
    let mut out = Vec::new();
    if !cfg.live() {
        return out;
    }

    if cfg.drift {
        for v in views {
            for r in drift
                .get(&v.id)
                .into_iter()
                .flatten()
                .filter(|r| r.breached)
            {
                let side = if r.drift_pct > 0.0 { "above" } else { "below" };
                out.push(Firing {
                    key: format!("drift:{}", r.id),
                    text: format!(
                        "{} is {:.1}pp {} its target band",
                        r.ticker,
                        r.drift_pct.abs(),
                        side
                    ),
                });
            }
        }
    }

    if cfg.big_move {
        let limit = cfg.threshold();
        for v in views {
            for h in v.holdings.iter().filter(|h| h.priced) {
                if h.day_pct.abs() >= limit {
                    let way = if h.day_pct > 0.0 { "up" } else { "down" };
                    out.push(Firing {
                        key: format!("move:{}", h.id),
                        text: format!("{} is {} {:.1}% today", h.ticker, way, h.day_pct.abs()),
                    });
                }
            }
        }
    }

    for level in &cfg.levels {
        for v in views {
            for h in v
                .holdings
                .iter()
                .filter(|h| h.priced && h.ticker == level.ticker)
            {
                let hit = if level.above {
                    h.price >= level.price
                } else {
                    h.price <= level.price
                };
                if hit {
                    out.push(Firing {
                        key: format!("level:{}", level.id),
                        text: format!(
                            "{} is {} {:.2}, now {:.2} {}",
                            h.ticker,
                            if level.above { "above" } else { "below" },
                            level.price,
                            h.price,
                            h.currency
                        ),
                    });
                }
            }
        }
    }

    if cfg.stale {
        if quotes_age_secs >= STALE_SECS {
            out.push(Firing {
                key: "stale".to_string(),
                text: format!(
                    "Prices are {} hours old, so the figures in the app are not today's",
                    quotes_age_secs / 3600
                ),
            });
        }
        for v in views {
            for h in v.holdings.iter().filter(|h| !h.priced) {
                out.push(Firing {
                    key: format!("unpriced:{}", h.id),
                    text: format!(
                        "{} has no usable price and is left out of every total",
                        h.ticker
                    ),
                });
            }
        }
    }

    out
}

/// Which firings are new, and the state to remember for next time.
///
/// A rule that is still firing is not sent again: a drift breach that lasts a fortnight should
/// buzz once, not thirteen hundred times. A rule that stops firing is forgotten, so the next
/// breach is news again.
pub fn newly_firing(now: &[Firing], seen: &[String]) -> (Vec<Firing>, Vec<String>) {
    let fresh: Vec<Firing> = now
        .iter()
        .filter(|f| !seen.contains(&f.key))
        .cloned()
        .collect();
    (fresh, now.iter().map(|f| f.key.clone()).collect())
}

/// Send one notification. The only thing here that touches the network.
///
/// ponytail: a plain POST with a title header, no priority, no tags, no click action. ntfy's
/// whole appeal is that the body is the message.
pub fn notify(cfg: &Alerts, text: &str) -> Result<(), String> {
    let url = format!("{}/{}", cfg.server_url(), cfg.topic.trim());
    ureq::post(&url)
        .header("Title", "Meridian")
        .send(text)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::HoldingView;

    fn holding(ticker: &str, day_pct: f64, price: f64, priced: bool) -> HoldingView {
        HoldingView {
            id: format!("h_{ticker}"),
            ticker: ticker.to_string(),
            name: ticker.to_string(),
            cls: "Equity".into(),
            shares: 10.0,
            price,
            currency: "NOK".into(),
            value: if priced { 1000.0 } else { 0.0 },
            day_pct,
            pl: 0.0,
            pl_pct: 0.0,
            weight_pct: 50.0,
            target_pct: 50.0,
            cost_basis: 900.0,
            cost_currency: "NOK".into(),
            priced,
        }
    }

    fn view(holdings: Vec<HoldingView>) -> PortfolioView {
        PortfolioView {
            id: "p1".into(),
            name: "P".into(),
            owner: String::new(),
            band_pct: 3.0,
            value: 2000.0,
            day_pct: 0.0,
            pl: 0.0,
            unpriced: holdings.iter().filter(|h| !h.priced).count(),
            holdings,
            by_class: Vec::new(),
        }
    }

    fn cfg() -> Alerts {
        Alerts {
            enabled: true,
            topic: "a-topic".into(),
            ..Alerts::default()
        }
    }

    #[test]
    fn nothing_fires_until_alerts_are_both_on_and_addressed() {
        // An enabled configuration with nowhere to send is not enabled, and a rule that fires
        // into the void would still consume its one notification through the change tracking.
        let v = vec![view(vec![holding("EQNR.OL", -20.0, 100.0, true)])];
        let mut c = cfg();
        c.big_move = true;
        assert!(!evaluate(&c, &v, &HashMap::new(), 0).is_empty());
        c.topic = "  ".into();
        assert!(evaluate(&c, &v, &HashMap::new(), 0).is_empty(), "no topic");
        c.topic = "a-topic".into();
        c.enabled = false;
        assert!(
            evaluate(&c, &v, &HashMap::new(), 0).is_empty(),
            "switched off"
        );
    }

    #[test]
    fn each_rule_fires_only_when_its_own_switch_is_on() {
        // The four kinds are independent: turning on drift must not start sending price levels.
        let v = vec![view(vec![holding("EQNR.OL", -9.0, 100.0, true)])];
        let mut drift = HashMap::new();
        drift.insert(
            "p1".to_string(),
            vec![DriftRow {
                id: "h_EQNR.OL".into(),
                ticker: "EQNR.OL".into(),
                name: "Equinor".into(),
                target_pct: 40.0,
                actual_pct: 46.2,
                drift_pct: 6.2,
                breached: true,
            }],
        );
        let mut c = cfg();
        assert!(evaluate(&c, &v, &drift, 0).is_empty(), "all switches off");

        c.drift = true;
        let only_drift = evaluate(&c, &v, &drift, 0);
        assert_eq!(only_drift.len(), 1);
        assert_eq!(only_drift[0].key, "drift:h_EQNR.OL");
        assert!(only_drift[0].text.contains("6.2pp above"), "{only_drift:?}");

        c.big_move = true;
        c.big_move_pct = 5.0;
        assert_eq!(evaluate(&c, &v, &drift, 0).len(), 2, "both now");
    }

    #[test]
    fn a_notification_never_carries_an_amount() {
        // The topic is public. Tickers and percentages are the deal; position sizes are not.
        let v = vec![view(vec![holding("EQNR.OL", -9.0, 100.0, true)])];
        let mut c = cfg();
        c.big_move = true;
        c.stale = true;
        let fired = evaluate(&c, &v, &HashMap::new(), STALE_SECS);
        assert!(!fired.is_empty());
        for f in &fired {
            assert!(!f.text.contains("1000"), "value leaked: {}", f.text);
            assert!(!f.text.contains("NOK"), "a total leaked: {}", f.text);
        }
    }

    #[test]
    fn a_level_fires_on_the_side_it_was_set_for() {
        let mut c = cfg();
        c.levels = vec![Level {
            id: "l1".into(),
            ticker: "EQNR.OL".into(),
            price: 300.0,
            above: true,
        }];
        let below = vec![view(vec![holding("EQNR.OL", 0.0, 299.0, true)])];
        let above = vec![view(vec![holding("EQNR.OL", 0.0, 301.0, true)])];
        assert!(evaluate(&c, &below, &HashMap::new(), 0).is_empty());
        let hit = evaluate(&c, &above, &HashMap::new(), 0);
        assert_eq!(hit.len(), 1);
        assert!(
            hit[0].text.ends_with("301.00 NOK"),
            "the currency belongs with the price it qualifies: {hit:?}"
        );
        // and the other way round
        c.levels[0].above = false;
        assert!(evaluate(&c, &above, &HashMap::new(), 0).is_empty());
        assert_eq!(evaluate(&c, &below, &HashMap::new(), 0).len(), 1);
    }

    #[test]
    fn an_unpriced_holding_is_reported_rather_than_treated_as_unchanged() {
        // The most useful alert in the set: the numbers you are looking at are wrong.
        let v = vec![view(vec![holding("XNAS.DE", 0.0, 0.0, false)])];
        let mut c = cfg();
        c.stale = true;
        let fired = evaluate(&c, &v, &HashMap::new(), 0);
        assert_eq!(fired.len(), 1, "{fired:?}");
        assert_eq!(fired[0].key, "unpriced:h_XNAS.DE");
    }

    #[test]
    fn a_rule_that_keeps_firing_notifies_once() {
        // A drift breach can last a fortnight. It must buzz on the day it starts and then be
        // quiet, or the alerts become something to be muted, which is the same as switched off.
        let f = |key: &str| Firing {
            key: key.to_string(),
            text: "x".into(),
        };
        let (fresh, seen) = newly_firing(&[f("drift:a")], &[]);
        assert_eq!(fresh.len(), 1);
        assert_eq!(seen, vec!["drift:a".to_string()]);

        let (again, seen) = newly_firing(&[f("drift:a")], &seen);
        assert!(again.is_empty(), "still firing is not news");

        // it stops, so the key is forgotten, and the next breach is news again
        let (_, seen) = newly_firing(&[], &seen);
        assert!(seen.is_empty());
        let (back, _) = newly_firing(&[f("drift:a")], &seen);
        assert_eq!(back.len(), 1, "a fresh breach after a quiet spell");
    }
}
