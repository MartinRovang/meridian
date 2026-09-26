//! Saved conditions over figures the app already computes.
//!
//! A rule is a question with a yes or no answer about one holding, one class, or the portfolio:
//! "is anything more than 40% of the money", "has anything fallen more than 15% since I bought
//! it". Nothing here fetches anything. `evaluate` is a pure function over the views and drift
//! rows the Dashboard already has, which is what makes every rule testable without a clock or a
//! network, and what lets the alert loop reuse it for free.
//!
//! ponytail: one field, one comparison, one number. No AND, no OR, no parentheses. Two rules
//! that must both hold are two rules, and an expression language is a parser, a precedence table
//! and an error message for every way of getting it wrong.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::alerts::Firing;
use crate::calc::{DriftRow, PortfolioView};

/// What a rule looks at. The money fields are read only from priced holdings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// Share of the portfolio, percent.
    #[default]
    Weight,
    /// Distance from the target weight, percentage points, signed.
    Drift,
    /// Today's move, percent, signed.
    DayMove,
    /// Gain or loss against what was paid, percent, signed.
    PlPct,
    /// The quoted price, in the holding's own currency.
    Price,
    /// The whole portfolio's move today, percent, signed.
    PortfolioDayMove,
    /// Share of the portfolio held in one class, percent.
    ClassWeight,
}

impl Field {
    /// Whether the rule asks about the portfolio rather than about each holding.
    pub fn whole_portfolio(self) -> bool {
        matches!(self, Field::PortfolioDayMove | Field::ClassWeight)
    }
    /// How the figure reads in a sentence, and in what unit.
    fn label(self) -> (&'static str, &'static str) {
        match self {
            Field::Weight => ("weight", "%"),
            Field::Drift => ("drift", "pp"),
            Field::DayMove => ("today's move", "%"),
            Field::PlPct => ("profit", "%"),
            Field::Price => ("price", ""),
            Field::PortfolioDayMove => ("today's move", "%"),
            Field::ClassWeight => ("weight", "%"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    #[default]
    Above,
    Below,
}

impl Op {
    fn holds(self, lhs: f64, rhs: f64) -> bool {
        match self {
            // At the threshold counts: a rule set at 40% that stays quiet on exactly 40% reads
            // as broken to the person who set it.
            Op::Above => lhs >= rhs,
            Op::Below => lhs <= rhs,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Op::Above => "above",
            Op::Below => "below",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub id: String,
    /// What the user called it. Empty is fine: `describe` writes the sentence either way.
    pub name: String,
    pub enabled: bool,
    /// Whether a hit should also reach the phone through the alert loop.
    pub notify: bool,
    pub field: Field,
    pub op: Op,
    pub value: f64,
    /// Narrows a holding rule to one ticker. Empty means every holding.
    pub ticker: String,
    /// Which class a `ClassWeight` rule is about. Ignored by every other field.
    pub cls: String,
}

impl Rule {
    /// The rule as a sentence, for the screen and for the notification.
    pub fn describe(&self) -> String {
        let (what, unit) = self.field.label();
        let subject = match self.field {
            Field::PortfolioDayMove => "the portfolio".to_string(),
            Field::ClassWeight => self.cls.clone(),
            _ if self.ticker.is_empty() => "any holding".to_string(),
            _ => self.ticker.clone(),
        };
        format!(
            "{}: {} {} {}{}",
            subject,
            what,
            self.op.word(),
            trim(self.value),
            unit
        )
    }
}

/// One rule currently true, and what made it true.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub rule_id: String,
    pub rule_name: String,
    pub portfolio_id: String,
    /// The ticker, the class, or the portfolio's name.
    pub subject: String,
    pub value: f64,
    pub text: String,
}

/// Everything currently true, given what the screens already computed.
///
/// Disabled rules are skipped. Unpriced holdings are skipped for every field: a holding with no
/// quote has a zeroed weight and a zeroed price, and a rule reading those would fire on a number
/// that means "unknown", not "zero".
pub fn evaluate(
    rules: &[Rule],
    views: &[PortfolioView],
    drift: &HashMap<String, Vec<DriftRow>>,
) -> Vec<Hit> {
    let mut out = Vec::new();
    for r in rules.iter().filter(|r| r.enabled) {
        for v in views {
            if r.field.whole_portfolio() {
                let found = match r.field {
                    Field::PortfolioDayMove => Some((v.name.clone(), v.day_pct)),
                    _ => v
                        .by_class
                        .iter()
                        .find(|c| c.cls == r.cls)
                        .map(|c| (c.cls.clone(), c.pct)),
                };
                if let Some((subject, value)) = found {
                    push(&mut out, r, v, subject, value);
                }
                continue;
            }
            for h in v
                .holdings
                .iter()
                .filter(|h| h.priced && (r.ticker.is_empty() || h.ticker == r.ticker))
            {
                let value = match r.field {
                    Field::Weight => h.weight_pct,
                    Field::DayMove => h.day_pct,
                    Field::PlPct => h.pl_pct,
                    Field::Price => h.price,
                    Field::Drift => match drift
                        .get(&v.id)
                        .into_iter()
                        .flatten()
                        .find(|d| d.id == h.id)
                    {
                        Some(d) => d.drift_pct,
                        // No drift row means no target to drift from, which is not a zero drift.
                        None => continue,
                    },
                    _ => continue,
                };
                push(&mut out, r, v, h.ticker.clone(), value);
            }
        }
    }
    out
}

fn push(out: &mut Vec<Hit>, r: &Rule, v: &PortfolioView, subject: String, value: f64) {
    if !r.op.holds(value, r.value) {
        return;
    }
    let (what, unit) = r.field.label();
    out.push(Hit {
        rule_id: r.id.clone(),
        rule_name: r.name.clone(),
        portfolio_id: v.id.clone(),
        text: format!("{} {} is {}{}", subject, what, trim(value), unit),
        subject,
        value,
    });
}

/// One decimal, without a trailing one on a round number: "40%" rather than "40.0%".
fn trim(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").unwrap_or(&s).to_string()
}

/// The hits that asked to reach a phone, as alert firings.
///
/// The key carries the rule and the subject, so a rule watching every holding buzzes once per
/// holding and then stays quiet, exactly like every other alert.
///
/// The text carries a ticker or a class and a percentage, and never a value: an ntfy topic is
/// public, and a rule is not an exception to that.
pub fn firings(hits: &[Hit], rules: &[Rule]) -> Vec<Firing> {
    hits.iter()
        .filter(|h| {
            rules
                .iter()
                .any(|r| r.id == h.rule_id && r.notify && r.enabled)
        })
        .map(|h| Firing {
            key: format!("rule:{}:{}", h.rule_id, h.subject),
            text: if h.rule_name.trim().is_empty() {
                h.text.clone()
            } else {
                format!("{}: {}", h.rule_name.trim(), h.text)
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::{ClassSlice, HoldingView};

    fn holding(ticker: &str, priced: bool) -> HoldingView {
        HoldingView {
            id: format!("h_{ticker}"),
            ticker: ticker.to_string(),
            name: ticker.to_string(),
            cls: "Energy".into(),
            shares: 10.0,
            price: 100.0,
            currency: "NOK".into(),
            price_base: 100.0,
            value: if priced { 1000.0 } else { 0.0 },
            day_pct: -2.0,
            pl: 100.0,
            pl_pct: 10.0,
            weight_pct: if priced { 50.0 } else { 0.0 },
            target_pct: 40.0,
            cost_basis: 900.0,
            cost_currency: "NOK".into(),
            priced,
        }
    }

    fn view(holdings: Vec<HoldingView>) -> PortfolioView {
        PortfolioView {
            id: "p1".into(),
            name: "Personal".into(),
            owner: String::new(),
            band_pct: 3.0,
            value: 2000.0,
            day_pct: -1.5,
            pl: 0.0,
            unpriced: holdings.iter().filter(|h| !h.priced).count(),
            by_class: vec![ClassSlice {
                cls: "Energy".into(),
                value: 1000.0,
                pct: 50.0,
            }],
            holdings,
        }
    }

    fn rule(field: Field, op: Op, value: f64) -> Rule {
        Rule {
            id: "r1".into(),
            name: String::new(),
            enabled: true,
            notify: false,
            field,
            op,
            value,
            ticker: String::new(),
            cls: String::new(),
        }
    }

    #[test]
    fn a_rule_fires_at_its_threshold_and_on_the_side_it_was_set_for() {
        let v = vec![view(vec![holding("EQNR.OL", true)])];
        let d = HashMap::new();
        // weight is 50
        assert_eq!(
            evaluate(&[rule(Field::Weight, Op::Above, 50.0)], &v, &d).len(),
            1
        );
        assert_eq!(
            evaluate(&[rule(Field::Weight, Op::Below, 50.0)], &v, &d).len(),
            1
        );
        assert!(evaluate(&[rule(Field::Weight, Op::Above, 50.1)], &v, &d).is_empty());
        assert!(evaluate(&[rule(Field::Weight, Op::Below, 49.9)], &v, &d).is_empty());
    }

    #[test]
    fn a_switched_off_rule_is_not_evaluated() {
        // Off must mean off, not "quietly still listed as firing on the screen".
        let v = vec![view(vec![holding("EQNR.OL", true)])];
        let mut r = rule(Field::Weight, Op::Above, 10.0);
        r.enabled = false;
        assert!(evaluate(&[r], &v, &HashMap::new()).is_empty());
    }

    #[test]
    fn an_unpriced_holding_is_skipped_rather_than_compared_as_zero() {
        // Its weight and price are zeroed because they are unknown. A "weight below 5%" rule
        // reading that would fire on every holding the app cannot price, which is the opposite
        // of useful.
        let v = vec![view(vec![holding("XNAS.DE", false)])];
        let hits = evaluate(&[rule(Field::Weight, Op::Below, 5.0)], &v, &HashMap::new());
        assert!(hits.is_empty(), "{hits:?}");
        let priced = vec![view(vec![holding("EQNR.OL", true)])];
        assert_eq!(
            evaluate(
                &[rule(Field::Price, Op::Below, 200.0)],
                &priced,
                &HashMap::new()
            )
            .len(),
            1,
            "and the same rule does fire on a holding that has a price"
        );
    }

    #[test]
    fn a_ticker_narrows_a_rule_to_one_holding() {
        let v = vec![view(vec![holding("EQNR.OL", true), holding("AAPL", true)])];
        let mut r = rule(Field::Weight, Op::Above, 10.0);
        assert_eq!(
            evaluate(std::slice::from_ref(&r), &v, &HashMap::new()).len(),
            2,
            "both"
        );
        r.ticker = "AAPL".into();
        let one = evaluate(&[r], &v, &HashMap::new());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].subject, "AAPL");
    }

    #[test]
    fn drift_is_read_from_the_drift_rows_and_a_holding_without_one_is_skipped() {
        // No drift row means no target was set. That is not a drift of zero, and a "drift below
        // -5pp" rule must not claim it is.
        let v = vec![view(vec![holding("EQNR.OL", true)])];
        let r = rule(Field::Drift, Op::Below, -5.0);
        assert!(
            evaluate(std::slice::from_ref(&r), &v, &HashMap::new()).is_empty(),
            "no row"
        );

        let mut d = HashMap::new();
        d.insert(
            "p1".to_string(),
            vec![DriftRow {
                id: "h_EQNR.OL".into(),
                ticker: "EQNR.OL".into(),
                name: "Equinor".into(),
                target_pct: 40.0,
                actual_pct: 30.0,
                drift_pct: -10.0,
                breached: true,
            }],
        );
        let hit = evaluate(&[r], &v, &d);
        assert_eq!(hit.len(), 1);
        assert!(hit[0].text.contains("-10pp"), "{hit:?}");
    }

    #[test]
    fn the_portfolio_and_class_fields_ask_about_the_whole_rather_than_each_holding() {
        let v = vec![view(vec![holding("EQNR.OL", true), holding("AAPL", true)])];
        let d = HashMap::new();
        // Two holdings, but one portfolio: one hit, not two.
        let day = evaluate(&[rule(Field::PortfolioDayMove, Op::Below, -1.0)], &v, &d);
        assert_eq!(day.len(), 1, "{day:?}");
        assert_eq!(day[0].subject, "Personal");

        let mut r = rule(Field::ClassWeight, Op::Above, 40.0);
        r.cls = "Energy".into();
        assert_eq!(evaluate(std::slice::from_ref(&r), &v, &d).len(), 1);
        // A class the portfolio does not hold is not a weight of zero that fires "below 10%".
        r.cls = "Utilities".into();
        r.op = Op::Below;
        r.value = 10.0;
        assert!(evaluate(&[r], &v, &d).is_empty());
    }

    #[test]
    fn only_rules_asking_to_notify_reach_the_phone_and_each_subject_keys_separately() {
        let v = vec![view(vec![holding("EQNR.OL", true), holding("AAPL", true)])];
        let mut r = rule(Field::Weight, Op::Above, 10.0);
        r.name = "  Too concentrated  ".into();
        let hits = evaluate(std::slice::from_ref(&r), &v, &HashMap::new());
        assert_eq!(hits.len(), 2);
        assert!(
            firings(&hits, std::slice::from_ref(&r)).is_empty(),
            "notify is off"
        );

        r.notify = true;
        let f = firings(&hits, &[r]);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].key, "rule:r1:EQNR.OL");
        assert_eq!(
            f[1].key, "rule:r1:AAPL",
            "one key per subject, not per rule"
        );
        assert!(f[0].text.starts_with("Too concentrated: "), "{f:?}");
        for x in &f {
            assert!(!x.text.contains("1000"), "a value leaked: {}", x.text);
            assert!(!x.text.contains("NOK"), "a total leaked: {}", x.text);
        }
    }

    #[test]
    fn a_rule_reads_as_a_sentence_whether_or_not_it_was_named() {
        let mut r = rule(Field::Weight, Op::Above, 40.0);
        assert_eq!(r.describe(), "any holding: weight above 40%");
        r.ticker = "EQNR.OL".into();
        assert_eq!(r.describe(), "EQNR.OL: weight above 40%");
        let mut c = rule(Field::ClassWeight, Op::Below, 12.5);
        c.cls = "Energy".into();
        assert_eq!(c.describe(), "Energy: weight below 12.5%");
        assert_eq!(
            rule(Field::PortfolioDayMove, Op::Below, -2.0).describe(),
            "the portfolio: today's move below -2%"
        );
    }
}
