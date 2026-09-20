//! The covariance of daily returns in base currency, and the weights that follow from it.
//!
//! Nothing here forecasts a return except where it says so. Covariance is estimated far more
//! reliably than expected return, which is why the default objectives use only covariance: an
//! optimiser fed five years of measured returns will pour the portfolio into whatever happened
//! to go up, and present it to four decimal places.

use std::collections::{BTreeMap, HashMap};

use crate::history::Series;
use crate::quotes::fx_symbol;

/// Trading days in a year. A convention, not a measurement, and the reason figures derived from
/// it are labelled "annualised" rather than presented as a year's worth of anything.
pub const YEAR: f64 = 252.0;

/// Fewer shared observations than this and the covariance is noise with a decimal point.
pub const MIN_OBS: usize = 252;

/// Daily returns for a set of symbols over the days they all share.
///
/// `rets[t][i]` is symbol `i`'s return on the `t`-th shared day. Aligned rather than ragged:
/// a covariance computed over days where one symbol was absent measures the calendar.
#[derive(Clone, Debug, Default)]
pub struct Matrix {
    pub symbols: Vec<String>,
    /// The day each row of returns ends on, so a window can be cut by date.
    pub days: Vec<String>,
    pub rets: Vec<Vec<f64>>,
}

impl Matrix {
    pub fn len(&self) -> usize {
        self.rets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rets.is_empty()
    }
    pub fn width(&self) -> usize {
        self.symbols.len()
    }

    /// The rows in `[from, to)`, keeping the same symbols.
    pub fn slice(&self, from: usize, to: usize) -> Matrix {
        let to = to.min(self.len());
        let from = from.min(to);
        Matrix {
            symbols: self.symbols.clone(),
            days: self.days[from..to].to_vec(),
            rets: self.rets[from..to].to_vec(),
        }
    }

    /// The same rows, restricted to the named symbols, in the order given.
    pub fn pick(&self, want: &[String]) -> Matrix {
        let idx: Vec<usize> = want
            .iter()
            .filter_map(|s| self.symbols.iter().position(|x| x == s))
            .collect();
        Matrix {
            symbols: idx.iter().map(|i| self.symbols[*i].clone()).collect(),
            days: self.days.clone(),
            rets: self
                .rets
                .iter()
                .map(|row| idx.iter().map(|i| row[*i]).collect())
                .collect(),
        }
    }
}

/// A symbol's closes converted to base currency, by day.
///
/// None when a rate is needed and missing. Never 1.0: valuing a dollar as a krone is the same
/// class of mistake as valuing a missing price at zero.
fn in_base(
    series: &Series,
    base: &str,
    fx: &HashMap<String, Series>,
) -> Option<BTreeMap<String, f64>> {
    let rates: Option<BTreeMap<String, f64>> = match fx_symbol(&series.currency, base) {
        None => None,
        Some(pair) => Some(
            fx.get(&pair)?
                .bars
                .iter()
                .map(|b| (b.day.clone(), b.close))
                .collect(),
        ),
    };
    let mut out = BTreeMap::new();
    for bar in &series.bars {
        let rate = match &rates {
            None => Some(1.0),
            // Forward fill, as elsewhere: exchanges keep different holidays, and a rate that is
            // missing because Oslo was shut is not a missing rate.
            Some(r) => r.range(..=bar.day.clone()).next_back().map(|x| *x.1),
        };
        // A day before the rate series begins is dropped, not valued at one. Dropping the whole
        // symbol for it would be worse: an fx history that starts a week late would cost an
        // instrument rather than a week.
        if let Some(rate) = rate {
            out.insert(bar.day.clone(), bar.close * rate);
        }
    }
    Some(out)
}

/// Daily returns for every symbol that has enough history, and the names of those that do not.
pub fn build(
    want: &[String],
    base: &str,
    histories: &HashMap<String, Series>,
    fx: &HashMap<String, Series>,
) -> (Matrix, Vec<String>) {
    build_since(want, base, histories, fx, "")
}

/// The same, restricted to days on or after `since`.
///
/// The window has to be chosen before the symbols are, not after. Intersecting first means one
/// company that listed last year truncates every other symbol to last year, and a search over
/// "five years" quietly becomes a search over one. Here a symbol that does not reach back to the
/// start of the window is excluded and named, and the window survives.
pub fn build_since(
    want: &[String],
    base: &str,
    histories: &HashMap<String, Series>,
    fx: &HashMap<String, Series>,
    since: &str,
) -> (Matrix, Vec<String>) {
    let mut excluded = Vec::new();
    let mut prices: Vec<(String, BTreeMap<String, f64>)> = Vec::new();
    for sym in want {
        let got = histories.get(sym).and_then(|s| in_base(s, base, fx));
        let reaches_back = |p: &BTreeMap<String, f64>| {
            since.is_empty() || p.keys().next().is_some_and(|d| d.as_str() <= since)
        };
        match got {
            Some(p) if p.len() > MIN_OBS && reaches_back(&p) => {
                let p = if since.is_empty() {
                    p
                } else {
                    p.into_iter().filter(|(d, _)| d.as_str() >= since).collect()
                };
                if p.len() > MIN_OBS {
                    prices.push((sym.clone(), p));
                } else {
                    excluded.push(sym.clone());
                }
            }
            _ => excluded.push(sym.clone()),
        }
    }
    if prices.is_empty() {
        return (Matrix::default(), excluded);
    }

    // The days every symbol has. Not a forward fill: a covariance wants observations that really
    // happened on the same day, and a filled day is a zero return that did not occur.
    let mut days: Vec<String> = prices[0].1.keys().cloned().collect();
    for (_, p) in &prices[1..] {
        days.retain(|d| p.contains_key(d));
    }
    days.sort();

    if days.len() <= MIN_OBS {
        for (sym, _) in prices {
            excluded.push(sym);
        }
        return (Matrix::default(), excluded);
    }

    let symbols: Vec<String> = prices.iter().map(|(s, _)| s.clone()).collect();
    let rets: Vec<Vec<f64>> = days
        .windows(2)
        .map(|w| {
            prices
                .iter()
                .map(|(_, p)| p[&w[1]] / p[&w[0]] - 1.0)
                .collect()
        })
        .collect();
    let matrix = Matrix {
        symbols,
        days: days[1..].to_vec(),
        rets,
    };
    (matrix, excluded)
}

/// Annualised sample covariance. `n-1`, because these are a sample of returns and not every
/// return there will ever be.
pub fn covariance(m: &Matrix) -> Vec<Vec<f64>> {
    let n = m.width();
    let t = m.len();
    if t < 2 || n == 0 {
        return vec![vec![0.0; n]; n];
    }
    let mean: Vec<f64> = (0..n)
        .map(|i| m.rets.iter().map(|r| r[i]).sum::<f64>() / t as f64)
        .collect();
    let mut cov = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in i..n {
            let s: f64 = m
                .rets
                .iter()
                .map(|r| (r[i] - mean[i]) * (r[j] - mean[j]))
                .sum();
            let v = s / (t as f64 - 1.0) * YEAR;
            cov[i][j] = v;
            cov[j][i] = v;
        }
    }
    cov
}

/// Annualised mean return per symbol. The one forecast in this module, used only by max-Sharpe.
pub fn means(m: &Matrix) -> Vec<f64> {
    let t = m.len();
    (0..m.width())
        .map(|i| {
            if t == 0 {
                0.0
            } else {
                m.rets.iter().map(|r| r[i]).sum::<f64>() / t as f64 * YEAR
            }
        })
        .collect()
}

/// Correlation, derived from the covariance so the two can never disagree.
pub fn correlation(cov: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = cov.len();
    let sd: Vec<f64> = (0..n).map(|i| cov[i][i].max(0.0).sqrt()).collect();
    let mut out = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            out[i][j] = if sd[i] > 0.0 && sd[j] > 0.0 {
                cov[i][j] / (sd[i] * sd[j])
            } else {
                0.0
            };
        }
    }
    out
}

/// Annualised volatility of a weighted portfolio, as a percent.
pub fn volatility(cov: &[Vec<f64>], w: &[f64]) -> f64 {
    let mut v = 0.0;
    for i in 0..w.len() {
        for j in 0..w.len() {
            v += w[i] * w[j] * cov[i][j];
        }
    }
    v.max(0.0).sqrt() * 100.0
}

/// Weights proportional to 1/sigma. No solver, no failure mode, and no return forecast.
pub fn inverse_vol(cov: &[Vec<f64>]) -> Vec<f64> {
    let inv: Vec<f64> = cov
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let sd = row[i].max(0.0).sqrt();
            if sd > 0.0 {
                1.0 / sd
            } else {
                0.0
            }
        })
        .collect();
    normalise(inv)
}

fn normalise(v: Vec<f64>) -> Vec<f64> {
    let sum: f64 = v.iter().sum();
    if sum <= 0.0 {
        let n = v.len();
        return vec![if n == 0 { 0.0 } else { 1.0 / n as f64 }; n];
    }
    v.into_iter().map(|x| x / sum).collect()
}

/// Euclidean projection onto the probability simplex: the nearest point that is non-negative and
/// sums to one. The sorting method, which is exact rather than iterative.
pub fn project_simplex(v: &[f64]) -> Vec<f64> {
    let n = v.len();
    if n == 0 {
        return Vec::new();
    }
    let mut u = v.to_vec();
    u.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let mut sum = 0.0;
    let mut theta = 0.0;
    for (j, val) in u.iter().enumerate() {
        sum += val;
        let t = (sum - 1.0) / (j as f64 + 1.0);
        if val - t > 0.0 {
            theta = t;
        }
    }
    v.iter().map(|x| (x - theta).max(0.0)).collect()
}

/// Long-only minimum variance.
///
/// The closed form `inv(S) * 1 / (1' * inv(S) * 1)` is not usable here: it routinely wants short
/// positions and this app cannot hold one. Projected gradient descent instead, which is forty
/// lines and cannot produce a weight the user cannot act on.
///
/// ponytail: fixed iteration count, no convergence test beyond a step that stops moving. For the
/// handful of assets a portfolio holds this converges long before the cap; a proper QP solver is
/// the upgrade if someone ever optimises hundreds of assets at once.
pub fn min_variance(cov: &[Vec<f64>]) -> Vec<f64> {
    let n = cov.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }
    // Gershgorin: the largest eigenvalue is at most the largest absolute row sum. The gradient of
    // w'Sw is 2Sw, so 1/(2L) is a step that cannot overshoot.
    let l = cov
        .iter()
        .map(|row| row.iter().map(|x| x.abs()).sum::<f64>())
        .fold(0.0, f64::max);
    if l <= 0.0 {
        return vec![1.0 / n as f64; n];
    }
    let step = 1.0 / (2.0 * l);
    let mut w = vec![1.0 / n as f64; n];
    // ponytail: 2000 iterations and a relative tolerance. For the handful of assets a basket
    // holds this converges in tens; the cap only bites on a covariance so degenerate that every
    // answer on the simplex is as good as the next.
    for _ in 0..2_000 {
        let grad: Vec<f64> = (0..n)
            .map(|i| 2.0 * (0..n).map(|j| cov[i][j] * w[j]).sum::<f64>())
            .collect();
        let next = project_simplex(&(0..n).map(|i| w[i] - step * grad[i]).collect::<Vec<f64>>());
        let moved: f64 = (0..n).map(|i| (next[i] - w[i]).abs()).sum();
        w = next;
        if moved < 1e-12 {
            break;
        }
    }
    w
}

/// One row of the optimizer's proposal.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Suggestion {
    pub id: String,
    pub ticker: String,
    pub name: String,
    /// The target in force today, which is what the suggestion replaces.
    pub target_pct: f64,
    pub suggested_pct: f64,
    /// False when the holding has no usable history: its target is carried over untouched and
    /// the screen says why.
    pub optimised: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Proposal {
    pub rows: Vec<Suggestion>,
    /// Named, never silently dropped.
    pub excluded: Vec<String>,
    /// Annualised volatility over the optimised holdings at their current targets, and at the
    /// suggested ones. Both measured over the same subset, or the comparison would be between
    /// two different portfolios.
    pub vol_current: f64,
    pub vol_suggested: f64,
    pub method: String,
    /// How much of the portfolio the optimizer was allowed to move.
    pub budget_pct: f64,
    pub observations: usize,
}

/// What the weights should be, given what is known about how these holdings move together.
///
/// Holdings with no usable history keep their current target untouched, and the optimised ones
/// are scaled to fill exactly what remains of 100. Rebalance refuses targets that do not sum to
/// 100, and a suggestion that cannot be applied is not a suggestion.
pub fn suggest(
    p: &crate::types::Portfolio,
    base: &str,
    histories: &HashMap<String, Series>,
    fx: &HashMap<String, Series>,
    method: &str,
) -> Proposal {
    let want: Vec<String> = p.holdings.iter().map(|h| h.ticker.clone()).collect();
    let (m, excluded) = build(&want, base, histories, fx);
    let cov = covariance(&m);
    let weights = match method {
        "invvol" => inverse_vol(&cov),
        _ => min_variance(&cov),
    };

    // What the optimizer may not touch: everything it could not measure.
    let kept: f64 = p
        .holdings
        .iter()
        .filter(|h| !m.symbols.contains(&h.ticker))
        .map(|h| h.target_pct)
        .sum();
    let budget = (100.0 - kept).max(0.0);

    let mut current: Vec<f64> = Vec::new();
    let rows: Vec<Suggestion> = p
        .holdings
        .iter()
        .map(|h| match m.symbols.iter().position(|s| *s == h.ticker) {
            Some(i) => {
                current.push(h.target_pct);
                Suggestion {
                    id: h.id.clone(),
                    ticker: h.ticker.clone(),
                    name: h.name.clone(),
                    target_pct: h.target_pct,
                    suggested_pct: weights[i] * budget,
                    optimised: true,
                }
            }
            None => Suggestion {
                id: h.id.clone(),
                ticker: h.ticker.clone(),
                name: h.name.clone(),
                target_pct: h.target_pct,
                suggested_pct: h.target_pct,
                optimised: false,
            },
        })
        .collect();

    Proposal {
        rows,
        excluded,
        // Renormalised within the subset: comparing a subset at weights summing to 70 against one
        // summing to 100 would report the difference in size as a difference in risk.
        vol_current: volatility(&cov, &normalise(current)),
        vol_suggested: volatility(&cov, &weights),
        method: method.to_string(),
        budget_pct: budget,
        observations: m.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Bar;

    fn diag(vars: &[f64]) -> Vec<Vec<f64>> {
        let n = vars.len();
        let mut m = vec![vec![0.0; n]; n];
        for (i, v) in vars.iter().enumerate() {
            m[i][i] = *v;
        }
        m
    }

    #[test]
    fn covariance_of_a_known_pair_is_the_hand_computed_one() {
        // Two assets, four days, numbers small enough to check on paper.
        let m = Matrix {
            symbols: vec!["A".into(), "B".into()],
            days: vec!["d1".into(), "d2".into(), "d3".into(), "d4".into()],
            rets: vec![
                vec![0.01, 0.02],
                vec![-0.01, -0.02],
                vec![0.02, 0.04],
                vec![-0.02, -0.04],
            ],
        };
        let cov = covariance(&m);
        // B is exactly twice A every day, so var(B) = 4 var(A) and cov = 2 var(A).
        assert!((cov[1][1] - 4.0 * cov[0][0]).abs() < 1e-12);
        assert!((cov[0][1] - 2.0 * cov[0][0]).abs() < 1e-12);
        // mean is zero here, so var(A) = sum(r^2)/(n-1) * 252.
        let hand = (0.0001 + 0.0001 + 0.0004 + 0.0004) / 3.0 * YEAR;
        assert!((cov[0][0] - hand).abs() < 1e-12, "{}", cov[0][0]);
        // Perfectly proportional, so the correlation is exactly one.
        assert!((correlation(&cov)[0][1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn minimum_variance_on_uncorrelated_assets_is_inverse_variance() {
        // With no correlation the answer is known in closed form: weight proportional to
        // 1/variance. If the solver drifts from that, it is wrong in a way no chart would show.
        let cov = diag(&[0.04, 0.01, 0.09]);
        let w = min_variance(&cov);
        let raw = [1.0 / 0.04, 1.0 / 0.01, 1.0 / 0.09];
        let total: f64 = raw.iter().sum();
        for (i, r) in raw.iter().enumerate() {
            assert!((w[i] - r / total).abs() < 1e-4, "{i}: {:?}", w);
        }
    }

    #[test]
    fn minimum_variance_refuses_to_short_what_it_would_like_to_short() {
        // sigma 30% and 10%, correlated 0.9. The unconstrained minimum-variance answer here is
        // -37% of the volatile one against 137% of the calm one, which is a short sale. Long-only
        // must hold none of it instead, and must not quietly report a negative weight the user
        // cannot act on.
        let cov = vec![vec![0.09, 0.027], vec![0.027, 0.01]];
        let w = min_variance(&cov);
        assert!(w.iter().all(|x| *x >= -1e-12), "no shorts: {w:?}");
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-9, "{w:?}");
        assert!(w[0] < 1e-6 && (w[1] - 1.0).abs() < 1e-6, "{w:?}");
        // and it must beat the naive split, or it is not solving anything
        assert!(volatility(&cov, &w) < volatility(&cov, &[0.5, 0.5]));
    }

    #[test]
    fn a_degenerate_covariance_still_gives_usable_weights() {
        // Two identical assets: every split is equally good, so any answer is correct as long as
        // it is one the user can act on.
        let cov = vec![vec![0.04, 0.04], vec![0.04, 0.04]];
        let w = min_variance(&cov);
        assert!(w.iter().all(|x| *x >= -1e-12), "{w:?}");
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-9, "{w:?}");
    }

    #[test]
    fn inverse_volatility_is_proportional_to_one_over_sigma() {
        let w = inverse_vol(&diag(&[0.04, 0.01]));
        // sigma 0.2 and 0.1, so the second gets twice the first.
        assert!((w[1] / w[0] - 2.0).abs() < 1e-12, "{w:?}");
        assert!((w[0] + w[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn volatility_of_one_asset_is_its_own() {
        assert!((volatility(&diag(&[0.04]), &[1.0]) - 20.0).abs() < 1e-9);
    }

    fn series(currency: &str, closes: &[f64]) -> Series {
        Series {
            currency: currency.to_string(),
            bars: closes
                .iter()
                .enumerate()
                .map(|(i, c)| Bar {
                    day: format!("2020-01-{:04}", i + 1),
                    close: *c,
                })
                .collect(),
        }
    }

    fn holding(ticker: &str, target: f64) -> crate::types::Holding {
        crate::types::Holding {
            id: format!("h_{ticker}"),
            ticker: ticker.to_string(),
            name: ticker.to_string(),
            cls: "Equity".into(),
            shares: 1.0,
            cost_basis: 100.0,
            cost_currency: "NOK".into(),
            target_pct: target,
        }
    }

    /// A price path with a known volatility: a fixed daily move whose sign alternates.
    fn wobble(step: f64, n: usize) -> Series {
        let mut close = 100.0;
        let closes: Vec<f64> = (0..n)
            .map(|i| {
                close *= if i % 2 == 0 {
                    1.0 + step
                } else {
                    1.0 / (1.0 + step)
                };
                close
            })
            .collect();
        series("NOK", &closes)
    }

    #[test]
    fn suggested_targets_and_untouched_ones_sum_to_exactly_a_hundred() {
        // NOHIST.OL has no history, so its 40% target is carried over and the optimizer is left
        // 60 points to divide. Rebalance refuses anything that does not sum to 100, so a
        // suggestion that breaks this is a suggestion the user cannot apply.
        let mut h = HashMap::new();
        h.insert("CALM.OL".to_string(), wobble(0.005, 400));
        h.insert("WILD.OL".to_string(), wobble(0.03, 400));
        let p = crate::types::Portfolio {
            id: "p1".into(),
            name: "P".into(),
            owner: String::new(),
            band_pct: 3.0,
            holdings: vec![
                holding("CALM.OL", 30.0),
                holding("WILD.OL", 30.0),
                holding("NOHIST.OL", 40.0),
            ],
        };
        let out = suggest(&p, "NOK", &h, &HashMap::new(), "minvar");
        assert_eq!(out.excluded, vec!["NOHIST.OL".to_string()]);
        assert!((out.budget_pct - 60.0).abs() < 1e-9);
        let total: f64 = out.rows.iter().map(|r| r.suggested_pct).sum();
        assert!((total - 100.0).abs() < 1e-9, "{total}");
        let untouched = out.rows.iter().find(|r| !r.optimised).expect("the row");
        assert!((untouched.suggested_pct - 40.0).abs() < 1e-9);
        // and the calm one must get the larger share of the budget
        let calm = out
            .rows
            .iter()
            .find(|r| r.ticker == "CALM.OL")
            .expect("calm");
        let wild = out
            .rows
            .iter()
            .find(|r| r.ticker == "WILD.OL")
            .expect("wild");
        assert!(calm.suggested_pct > wild.suggested_pct, "{calm:?} {wild:?}");

        // The two measured holdings carry targets of 30 and 30, which is a half-and-half split of
        // the part that was measured. Reported volatility must be that split's, not the figure
        // you get by feeding weights of 30.0 and 30.0 into a formula expecting fractions.
        let (m, _) = build(
            &["CALM.OL".to_string(), "WILD.OL".to_string()],
            "NOK",
            &h,
            &HashMap::new(),
        );
        let half = volatility(&covariance(&m), &[0.5, 0.5]);
        assert!(
            (out.vol_current - half).abs() < 1e-9,
            "{} vs {half}",
            out.vol_current
        );
    }

    #[test]
    fn the_suggestion_is_less_volatile_than_what_it_replaces() {
        // The whole claim of the screen in one assertion. Measured over the same subset at both
        // sets of weights, since comparing a subset against the whole portfolio would report a
        // difference in size as a difference in risk.
        let mut h = HashMap::new();
        h.insert("CALM.OL".to_string(), wobble(0.005, 400));
        h.insert("WILD.OL".to_string(), wobble(0.03, 400));
        let p = crate::types::Portfolio {
            id: "p1".into(),
            name: "P".into(),
            owner: String::new(),
            band_pct: 3.0,
            holdings: vec![holding("CALM.OL", 10.0), holding("WILD.OL", 90.0)],
        };
        let out = suggest(&p, "NOK", &h, &HashMap::new(), "minvar");
        assert!(out.vol_suggested < out.vol_current, "{out:?}");
        assert!(out.observations > MIN_OBS);
    }

    #[test]
    fn a_symbol_with_too_little_history_is_excluded_and_named() {
        let mut h = HashMap::new();
        h.insert("LONG.OL".to_string(), series("NOK", &[100.0; 400]));
        h.insert("SHORT.OL".to_string(), series("NOK", &[100.0; 10]));
        let (m, excluded) = build(
            &["LONG.OL".to_string(), "SHORT.OL".to_string()],
            "NOK",
            &h,
            &HashMap::new(),
        );
        assert_eq!(excluded, vec!["SHORT.OL".to_string()]);
        assert_eq!(m.symbols, vec!["LONG.OL".to_string()]);
    }

    #[test]
    fn a_missing_fx_rate_excludes_the_symbol_rather_than_valuing_it_as_base() {
        let mut h = HashMap::new();
        h.insert("AAPL".to_string(), series("USD", &[100.0; 400]));
        let (m, excluded) = build(&["AAPL".to_string()], "NOK", &h, &HashMap::new());
        assert_eq!(excluded, vec!["AAPL".to_string()]);
        assert!(m.is_empty());
    }

    #[test]
    fn returns_are_computed_in_base_currency_not_in_the_quoted_one() {
        // Price flat in USD, dollar doubling against the krone: a NOK investor doubled their
        // money, and a covariance computed on the quoted price would see no movement at all.
        let mut h = HashMap::new();
        h.insert("AAPL".to_string(), series("USD", &[100.0; 400]));
        let mut fx = HashMap::new();
        let rates: Vec<f64> = (0..400).map(|i| 10.0 + i as f64 * 0.01).collect();
        fx.insert("USDNOK=X".to_string(), series("NOK", &rates));
        let (m, excluded) = build(&["AAPL".to_string()], "NOK", &h, &fx);
        assert!(excluded.is_empty());
        assert!(m.rets.iter().all(|r| r[0] > 0.0), "every day should gain");
    }

    #[test]
    fn days_before_the_rate_series_begins_are_dropped_not_valued_at_one() {
        // The dollar price exists from day one; the USDNOK history starts ten days later. Those
        // ten days have no honest krone value, and treating the rate as 1.0 would invent a 90%
        // crash on the eleventh day.
        let mut h = HashMap::new();
        h.insert("AAPL".to_string(), series("USD", &[100.0; 400]));
        let mut fx = HashMap::new();
        let mut rates = series("NOK", &[10.0; 400]);
        rates.bars.drain(0..10);
        fx.insert("USDNOK=X".to_string(), rates);
        let (m, excluded) = build(&["AAPL".to_string()], "NOK", &h, &fx);
        assert!(
            excluded.is_empty(),
            "a late rate costs days, not the instrument"
        );
        assert_eq!(
            m.len(),
            389,
            "400 days less the 10 unrated, less one to differencing"
        );
        assert!(
            m.rets.iter().all(|r| r[0].abs() < 1e-12),
            "nothing moved: {:?}",
            m.rets
                .iter()
                .map(|r| r[0])
                .filter(|x| x.abs() > 1e-12)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_rate_missing_mid_series_is_forward_filled_rather_than_costing_the_day() {
        // Oslo shut, New York open. The krone did not stop existing, so the last rate stands and
        // the day survives; dropping it would put a hole in the middle of the covariance.
        let mut h = HashMap::new();
        h.insert("AAPL".to_string(), series("USD", &[100.0; 400]));
        let mut fx = HashMap::new();
        let mut rates = series("NOK", &[10.0; 400]);
        rates.bars.remove(200);
        fx.insert("USDNOK=X".to_string(), rates);
        let (m, _) = build(&["AAPL".to_string()], "NOK", &h, &fx);
        assert_eq!(m.len(), 399, "all 400 days rated, one lost to differencing");
    }

    #[test]
    fn only_the_days_every_symbol_has_are_used() {
        let mut h = HashMap::new();
        h.insert("A".to_string(), series("NOK", &[100.0; 400]));
        let mut b = series("NOK", &[100.0; 400]);
        b.bars.remove(50);
        h.insert("B".to_string(), b);
        let (m, excluded) = build(
            &["A".to_string(), "B".to_string()],
            "NOK",
            &h,
            &HashMap::new(),
        );
        assert!(excluded.is_empty());
        // 400 days, one dropped for being unshared, one lost to differencing.
        assert_eq!(m.len(), 398);
        assert_eq!(m.width(), 2);
    }
}
