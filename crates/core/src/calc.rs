//! Every number the user sees. Nothing here touches the network or the disk, so it is all
//! directly testable, and it is the only place portfolio arithmetic is allowed to live.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::quotes::{fx_symbol, Cache};
use crate::types::{Holding, Portfolio, Quote};

/// One holding as the UI shows it. `priced` false means every money field is meaningless and
/// the row must be rendered as "no quote".
#[derive(Clone, Debug, Serialize)]
pub struct HoldingView {
    pub id: String,
    pub ticker: String,
    pub name: String,
    pub cls: String,
    pub shares: f64,
    /// In the holding's own currency, as quoted.
    pub price: f64,
    pub currency: String,
    /// In base currency.
    pub value: f64,
    pub day_pct: f64,
    pub pl: f64,
    pub pl_pct: f64,
    pub weight_pct: f64,
    pub target_pct: f64,
    /// What was paid, in `cost_currency`. Carried through unconverted because the Builder edits
    /// it: a value round-tripped through base currency would drift on every save.
    pub cost_basis: f64,
    pub cost_currency: String,
    pub priced: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClassSlice {
    pub cls: String,
    pub value: f64,
    pub pct: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PortfolioView {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub band_pct: f64,
    pub value: f64,
    pub day_pct: f64,
    pub pl: f64,
    pub holdings: Vec<HoldingView>,
    /// How many holdings had no usable quote. Shown to the user; never silently swallowed.
    pub unpriced: usize,
    pub by_class: Vec<ClassSlice>,
}

/// Convert between currencies using a cached pair. None when the rate is not available, which
/// callers must treat as "unpriced", never as a rate of 1.
pub fn convert(amount: f64, from: &str, to: &str, cache: &Cache) -> Option<f64> {
    match fx_symbol(from, to) {
        None => Some(amount),
        Some(pair) => cache.get(&pair).map(|r: &Quote| amount * r.price),
    }
}

/// A single holding valued in base currency.
///
/// An unknown ticker, a missing price rate and a missing cost rate all land in the same place:
/// priced = false with every money field zero. A row the user is told nothing is known about
/// beats a row quietly valued at zero and folded into the total.
pub fn view_holding(h: &Holding, base: &str, cache: &Cache) -> HoldingView {
    let blank = |price: f64, currency: String| HoldingView {
        id: h.id.clone(),
        ticker: h.ticker.clone(),
        name: h.name.clone(),
        cls: h.cls.clone(),
        shares: h.shares,
        price,
        currency,
        value: 0.0,
        day_pct: 0.0,
        pl: 0.0,
        pl_pct: 0.0,
        weight_pct: 0.0,
        target_pct: h.target_pct,
        cost_basis: h.cost_basis,
        cost_currency: h.cost_currency.clone(),
        priced: false,
    };
    let Some(q) = cache.get(&h.ticker) else {
        return blank(0.0, String::new());
    };
    let Some(value) = convert(h.shares * q.price, &q.currency, base, cache) else {
        return blank(q.price, q.currency.clone());
    };
    let Some(cost) = convert(h.cost_basis, &h.cost_currency, base, cache) else {
        return blank(q.price, q.currency.clone());
    };
    let day_pct = if q.prev_close > 0.0 {
        (q.price - q.prev_close) / q.prev_close * 100.0
    } else {
        0.0
    };
    let pl = value - cost;
    HoldingView {
        id: h.id.clone(),
        ticker: h.ticker.clone(),
        name: h.name.clone(),
        cls: h.cls.clone(),
        shares: h.shares,
        price: q.price,
        currency: q.currency.clone(),
        value,
        day_pct,
        pl,
        pl_pct: if cost > 0.0 { pl / cost * 100.0 } else { 0.0 },
        // filled in by view_portfolio, which is the only thing that knows the total
        weight_pct: 0.0,
        target_pct: h.target_pct,
        cost_basis: h.cost_basis,
        cost_currency: h.cost_currency.clone(),
        priced: true,
    }
}

/// A whole portfolio. Weights are a share of the PRICED total, so an unpriced holding shifts no
/// weight onto anything else and is reported separately instead.
pub fn view_portfolio(p: &Portfolio, base: &str, cache: &Cache) -> PortfolioView {
    let mut holdings: Vec<HoldingView> = p
        .holdings
        .iter()
        .map(|h| view_holding(h, base, cache))
        .collect();
    let total: f64 = holdings.iter().map(|h| h.value).sum();
    let unpriced = holdings.iter().filter(|h| !h.priced).count();
    if total > 0.0 {
        for h in &mut holdings {
            h.weight_pct = h.value / total * 100.0;
        }
    }
    // Value-weighted, not a mean of the percentages: a 1% move on the big position is not the
    // same event as a 1% move on the small one.
    let day_pct = if total > 0.0 {
        holdings.iter().map(|h| h.day_pct * h.value).sum::<f64>() / total
    } else {
        0.0
    };
    let mut by: BTreeMap<String, f64> = BTreeMap::new();
    for h in holdings.iter().filter(|h| h.priced) {
        *by.entry(h.cls.clone()).or_default() += h.value;
    }
    let by_class = by
        .into_iter()
        .map(|(cls, value)| ClassSlice {
            cls,
            value,
            pct: if total > 0.0 {
                value / total * 100.0
            } else {
                0.0
            },
        })
        .collect();
    PortfolioView {
        id: p.id.clone(),
        name: p.name.clone(),
        owner: p.owner.clone(),
        band_pct: p.band_pct,
        value: total,
        day_pct,
        pl: holdings.iter().map(|h| h.pl).sum(),
        holdings,
        unpriced,
        by_class,
    }
}

/// One row of the rebalance table.
#[derive(Clone, Debug, Serialize)]
pub struct DriftRow {
    pub id: String,
    pub ticker: String,
    pub name: String,
    pub target_pct: f64,
    pub actual_pct: f64,
    /// Actual minus target, in percentage points. Positive is overweight.
    pub drift_pct: f64,
    pub breached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Debug, Serialize)]
pub struct Trade {
    pub id: String,
    pub ticker: String,
    pub name: String,
    pub side: Side,
    /// Fractional. Rounding to whole shares is the broker's problem and the user's judgement.
    pub shares: f64,
    /// In base currency, always positive.
    pub amount: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum RebalanceError {
    #[error("target weights sum to {0:.1}%, not 100%")]
    TargetsDoNotSum(f64),
    #[error("no holding has a usable price")]
    NothingPriced,
}

/// Trades smaller than this in base currency are not worth the commission.
const MIN_TRADE: f64 = 1.0;
/// How far the sum of targets may be from 100 before rebalancing refuses.
const TARGET_TOLERANCE: f64 = 0.05;

/// How far each holding has drifted from its target, and whether that breaches the band.
///
/// Unpriced holdings are skipped. Including one would report it as 100 points underweight,
/// which is a statement about a missing quote dressed up as a statement about the portfolio.
pub fn drift(v: &PortfolioView) -> Vec<DriftRow> {
    v.holdings
        .iter()
        .filter(|h| h.priced)
        .map(|h| {
            let drift_pct = h.weight_pct - h.target_pct;
            DriftRow {
                id: h.id.clone(),
                ticker: h.ticker.clone(),
                name: h.name.clone(),
                target_pct: h.target_pct,
                actual_pct: h.weight_pct,
                drift_pct,
                breached: drift_pct.abs() > v.band_pct,
            }
        })
        .collect()
}

/// What to buy and sell to land on target.
///
/// With `cash` greater than zero the portfolio is rebalanced against its value PLUS that cash and
/// only buys are returned, which is how you deploy new money without selling anything.
///
/// ponytail: proportional, no tax lots, no wash-sale rules, no whole-share rounding, no
/// commission model. Those belong with a broker integration, not with typed-in positions.
pub fn trades(v: &PortfolioView, cash: f64) -> Result<Vec<Trade>, RebalanceError> {
    let priced: Vec<&HoldingView> = v.holdings.iter().filter(|h| h.priced).collect();
    if priced.is_empty() || v.value <= 0.0 {
        return Err(RebalanceError::NothingPriced);
    }
    let sum: f64 = priced.iter().map(|h| h.target_pct).sum();
    if (sum - 100.0).abs() > TARGET_TOLERANCE {
        return Err(RebalanceError::TargetsDoNotSum(sum));
    }
    let pot = v.value + cash.max(0.0);
    let mut out = Vec::new();
    for h in priced {
        let want = pot * h.target_pct / 100.0;
        let delta = want - h.value;
        if cash > 0.0 && delta <= 0.0 {
            continue; // cash mode never sells
        }
        if delta.abs() < MIN_TRADE || h.shares <= 0.0 {
            continue;
        }
        // h.price is in the holding's own currency while h.value is in base; their ratio is the
        // base-currency price per share, which avoids a second fx lookup and cannot disagree
        // with the value the rest of the screen shows.
        let per_share_base = h.value / h.shares;
        out.push(Trade {
            id: h.id.clone(),
            ticker: h.ticker.clone(),
            name: h.name.clone(),
            side: if delta > 0.0 { Side::Buy } else { Side::Sell },
            shares: (delta / per_share_base).abs(),
            amount: delta.abs(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Holding, Portfolio};

    fn q(price: f64, prev: f64, ccy: &str) -> Quote {
        Quote {
            price,
            prev_close: prev,
            currency: ccy.into(),
            ts: 0,
        }
    }

    fn cache() -> Cache {
        let mut c = Cache::new();
        c.insert("EQNR.OL".into(), q(270.0, 250.0, "NOK"));
        c.insert("AAPL".into(), q(200.0, 200.0, "USD"));
        c.insert("USDNOK=X".into(), q(10.0, 10.0, "NOK"));
        c
    }

    fn holding(id: &str, ticker: &str, shares: f64, cost: f64, ccy: &str, target: f64) -> Holding {
        Holding {
            id: id.into(),
            ticker: ticker.into(),
            name: ticker.into(),
            cls: "Equity".into(),
            shares,
            cost_basis: cost,
            cost_currency: ccy.into(),
            target_pct: target,
        }
    }

    fn portfolio(band: f64, holdings: Vec<Holding>) -> Portfolio {
        Portfolio {
            id: "p1".into(),
            name: "P".into(),
            owner: "me".into(),
            band_pct: band,
            holdings,
        }
    }

    #[test]
    fn the_same_currency_converts_to_itself_untouched() {
        assert_eq!(convert(100.0, "NOK", "NOK", &cache()), Some(100.0));
    }

    #[test]
    fn a_foreign_amount_converts_through_the_cached_pair() {
        assert_eq!(convert(100.0, "USD", "NOK", &cache()), Some(1000.0));
    }

    #[test]
    fn a_missing_rate_is_none_never_a_rate_of_one() {
        assert_eq!(convert(100.0, "JPY", "NOK", &cache()), None);
    }

    #[test]
    fn a_holding_is_valued_in_base_currency() {
        let h = holding("h1", "AAPL", 10.0, 15000.0, "NOK", 50.0);
        let v = view_holding(&h, "NOK", &cache());
        assert_eq!(v.value, 20000.0, "10 shares at 200 USD, at 10 NOK per USD");
        assert!(v.priced);
        assert_eq!(v.pl, 5000.0);
    }

    #[test]
    fn day_change_comes_from_the_previous_close_of_that_exchange() {
        let h = holding("h1", "EQNR.OL", 1.0, 250.0, "NOK", 50.0);
        let v = view_holding(&h, "NOK", &cache());
        assert!((v.day_pct - 8.0).abs() < 1e-9, "270 over 250 is +8%");
    }

    #[test]
    fn an_unpriced_holding_is_flagged_and_contributes_nothing() {
        let h = holding("h1", "NOPE.OL", 10.0, 1000.0, "NOK", 50.0);
        let v = view_holding(&h, "NOK", &cache());
        assert!(
            !v.priced,
            "the UI must be able to say this one has no quote"
        );
        assert_eq!(v.value, 0.0);
        assert_eq!(v.pl, 0.0, "an unpriced holding has no knowable P/L either");
    }

    #[test]
    fn a_missing_fx_rate_makes_a_holding_unpriced_rather_than_wrongly_valued() {
        let h = holding("h1", "AAPL", 10.0, 1000.0, "NOK", 50.0);
        let mut c = cache();
        c.remove("USDNOK=X");
        let v = view_holding(&h, "NOK", &c);
        assert!(!v.priced);
        assert_eq!(v.value, 0.0);
    }

    #[test]
    fn a_cost_basis_in_a_currency_with_no_rate_also_makes_it_unpriced() {
        let h = holding("h1", "EQNR.OL", 10.0, 1000.0, "JPY", 50.0);
        let v = view_holding(&h, "NOK", &cache());
        assert!(
            !v.priced,
            "a knowable value with an unknowable cost is not a usable row"
        );
    }

    #[test]
    fn portfolio_totals_exclude_unpriced_holdings_and_count_them() {
        let p = portfolio(
            3.0,
            vec![
                holding("h1", "EQNR.OL", 10.0, 2500.0, "NOK", 50.0),
                holding("h2", "NOPE.OL", 10.0, 1000.0, "NOK", 50.0),
            ],
        );
        let v = view_portfolio(&p, "NOK", &cache());
        assert_eq!(v.value, 2700.0);
        assert_eq!(v.unpriced, 1);
    }

    #[test]
    fn weights_are_shares_of_the_priced_total_and_sum_to_a_hundred() {
        let p = portfolio(
            3.0,
            vec![
                holding("h1", "EQNR.OL", 10.0, 0.0, "NOK", 50.0),
                holding("h2", "AAPL", 1.0, 0.0, "NOK", 50.0),
            ],
        );
        let v = view_portfolio(&p, "NOK", &cache());
        let sum: f64 = v.holdings.iter().map(|h| h.weight_pct).sum();
        assert!((sum - 100.0).abs() < 1e-9, "weights summed to {sum}");
    }

    #[test]
    fn a_portfolio_day_change_is_weighted_by_value_not_an_average_of_percentages() {
        // 2700 NOK up 8%, 2000 NOK flat. A naive mean would say 4.0%.
        let p = portfolio(
            3.0,
            vec![
                holding("h1", "EQNR.OL", 10.0, 0.0, "NOK", 50.0),
                holding("h2", "AAPL", 1.0, 0.0, "NOK", 50.0),
            ],
        );
        let v = view_portfolio(&p, "NOK", &cache());
        let want = 8.0 * 2700.0 / 4700.0;
        assert!(
            (v.day_pct - want).abs() < 1e-9,
            "got {} want {want}",
            v.day_pct
        );
    }

    #[test]
    fn the_allocation_groups_by_class_and_ignores_unpriced_rows() {
        let mut a = holding("h1", "EQNR.OL", 10.0, 0.0, "NOK", 50.0);
        a.cls = "Energy".into();
        let mut b = holding("h2", "AAPL", 1.0, 0.0, "NOK", 50.0);
        b.cls = "Tech".into();
        let mut c = holding("h3", "NOPE.OL", 1.0, 0.0, "NOK", 0.0);
        c.cls = "Ghost".into();
        let v = view_portfolio(&portfolio(3.0, vec![a, b, c]), "NOK", &cache());
        let names: Vec<&str> = v.by_class.iter().map(|s| s.cls.as_str()).collect();
        assert_eq!(
            names,
            vec!["Energy", "Tech"],
            "no slice for an unpriced class"
        );
        let sum: f64 = v.by_class.iter().map(|s| s.pct).sum();
        assert!((sum - 100.0).abs() < 1e-9);
    }

    /// A portfolio that is 2700 NOK and 1800 NOK by value, so exactly 60/40.
    fn sixty_forty(target_a: f64, target_b: f64, band: f64) -> PortfolioView {
        let p = portfolio(
            band,
            vec![
                holding("h1", "EQNR.OL", 10.0, 0.0, "NOK", target_a),
                holding("h2", "AAPL", 0.9, 0.0, "NOK", target_b),
            ],
        );
        view_portfolio(&p, "NOK", &cache())
    }

    #[test]
    fn drift_is_actual_minus_target_in_percentage_points() {
        let d = drift(&sixty_forty(50.0, 50.0, 3.0));
        assert_eq!(d.len(), 2);
        assert!((d[0].actual_pct - 60.0).abs() < 1e-9);
        assert!((d[0].drift_pct - 10.0).abs() < 1e-9);
        assert!((d[1].drift_pct + 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_drift_inside_the_band_is_not_a_breach() {
        let d = drift(&sixty_forty(58.0, 42.0, 3.0));
        assert!(!d[0].breached, "2 points of drift inside a 3 point band");
    }

    #[test]
    fn a_drift_outside_the_band_is_a_breach_on_both_sides() {
        let d = drift(&sixty_forty(50.0, 50.0, 3.0));
        assert!(d[0].breached, "overweight breaches");
        assert!(d[1].breached, "underweight breaches too");
    }

    #[test]
    fn drift_skips_unpriced_rows_rather_than_calling_them_a_hundred_percent_underweight() {
        let p = portfolio(
            3.0,
            vec![
                holding("h1", "EQNR.OL", 10.0, 0.0, "NOK", 50.0),
                holding("h2", "NOPE.OL", 10.0, 0.0, "NOK", 50.0),
            ],
        );
        let d = drift(&view_portfolio(&p, "NOK", &cache()));
        assert_eq!(d.len(), 1, "an unknown price is not a drift signal");
    }

    #[test]
    fn trades_move_each_holding_onto_its_target() {
        let t = trades(&sixty_forty(50.0, 50.0, 3.0), 0.0).expect("targets sum to 100");
        let sell = t.iter().find(|t| t.ticker == "EQNR.OL").expect("a sell");
        assert_eq!(sell.side, Side::Sell);
        assert!(
            (sell.amount - 450.0).abs() < 1e-6,
            "4500 total, target 2250, held 2700"
        );
        assert!((sell.shares - 450.0 / 270.0).abs() < 1e-6);
        let buy = t.iter().find(|t| t.ticker == "AAPL").expect("a buy");
        assert_eq!(buy.side, Side::Buy);
        assert!((buy.amount - 450.0).abs() < 1e-6);
        assert!(
            (buy.shares - 450.0 / 2000.0).abs() < 1e-6,
            "priced per share in NOK, not USD"
        );
    }

    #[test]
    fn cash_to_deploy_produces_buys_only() {
        let t = trades(&sixty_forty(50.0, 50.0, 3.0), 1000.0).expect("targets sum to 100");
        assert!(!t.is_empty());
        assert!(
            t.iter().all(|t| t.side == Side::Buy),
            "cash mode never sells: {t:?}"
        );
    }

    #[test]
    fn cash_to_deploy_spends_no_more_than_the_cash() {
        let t = trades(&sixty_forty(50.0, 50.0, 3.0), 1000.0).expect("targets sum to 100");
        let spent: f64 = t.iter().map(|t| t.amount).sum();
        assert!(spent <= 1000.0 + 1e-6, "spent {spent} of 1000");
    }

    #[test]
    fn targets_that_do_not_sum_to_a_hundred_refuse_to_produce_trades() {
        match trades(&sixty_forty(50.0, 40.0, 3.0), 0.0) {
            Err(RebalanceError::TargetsDoNotSum(got)) => assert!((got - 90.0).abs() < 1e-9),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_portfolio_with_nothing_priced_refuses_rather_than_dividing_by_zero() {
        let p = portfolio(3.0, vec![holding("h1", "NOPE.OL", 10.0, 0.0, "NOK", 100.0)]);
        let v = view_portfolio(&p, "NOK", &cache());
        assert!(matches!(
            trades(&v, 0.0),
            Err(RebalanceError::NothingPriced)
        ));
    }

    #[test]
    fn a_holding_already_on_target_generates_no_trade() {
        let t = trades(&sixty_forty(60.0, 40.0, 3.0), 0.0).expect("targets sum to 100");
        assert!(t.is_empty(), "nothing to do, got {t:?}");
    }

    #[test]
    fn an_empty_portfolio_does_not_divide_by_zero() {
        let v = view_portfolio(&portfolio(3.0, vec![]), "NOK", &cache());
        assert_eq!(v.value, 0.0);
        assert_eq!(v.day_pct, 0.0);
        assert!(v.holdings.is_empty());
        assert!(v.by_class.is_empty());
    }

    #[test]
    fn the_cost_basis_survives_an_unpriced_holding_so_the_builder_can_still_correct_it() {
        // A holding with no quote is exactly the one a user needs to edit. If the view dropped
        // its cost basis, saving that edit would silently rewrite it to zero.
        let h = holding("h1", "NOPE.OL", 5.0, 1234.5, "SEK", 10.0);
        let v = view_holding(&h, "NOK", &cache());
        assert!(!v.priced);
        assert_eq!(v.cost_basis, 1234.5);
        assert_eq!(v.cost_currency, "SEK");
    }
}
