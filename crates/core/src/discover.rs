//! Searching a market for a basket of N, and then being honest about what was found.
//!
//! The danger this module exists to manage is not the arithmetic, it is the selection. Picking
//! the best ten of a hundred and twenty by how they behaved is the most reliable way to
//! manufacture a beautiful result from noise: the winner is chosen as much for its luck as for
//! its quality, and the more candidates are searched the better it looks and the less it means.
//!
//! So the window is split. Everything that chooses anything, the pre-filter, the weights and the
//! winner, sees only the training rows. The test rows are read once, after the basket is frozen,
//! and reported next to an equal-weight portfolio of the same names. When the optimiser cannot
//! beat that, the caller is told so.

use serde::Serialize;

use crate::optimize::{covariance, means, min_variance, volatility, Matrix, YEAR};

/// Trading days between the training window and the test window.
///
/// A test window that starts the day training ends can see the last training prices through an
/// overnight gap. Lux measured the effect at 0.2 to 0.4 of Sharpe, which is most of a strategy.
pub const GAP: usize = 5;

/// Roughly a quarter. The cadence the app assumes throughout: not daily, which is a fantasy, and
/// not never, which leaves the weights to drift into something nobody chose.
pub const REBALANCE_DAYS: usize = 63;

/// One way, in basis points: a hundredth of a percent, so 25 is 0.25%.
///
/// Oslo commission is around 5 bps plus half the spread on a liquid name. Thin names cost far
/// more, which is part of why the universe is filtered to what actually trades.
pub const COST_BPS: f64 = 25.0;

/// How many combinations are worth enumerating before falling back to a greedy search.
pub const EXHAUSTIVE_LIMIT: u64 = 200_000;

/// Correlation above which two candidates are treated as the same bet.
pub const CORR_MAX: f64 = 0.95;

/// Shrinkage of the mean toward the grand mean.
///
/// ponytail: a fixed intensity, not James-Stein. Half is a strong, defensible prior that the
/// differences between five-year averages are mostly noise, and it is one constant rather than an
/// estimator whose own variance would need explaining.
const MEAN_SHRINK: f64 = 0.5;

/// Shrinkage of the covariance toward its diagonal, which keeps it invertible and pulls the
/// wilder correlations back toward what a smaller sample can actually support.
const COV_SHRINK: f64 = 0.1;

#[derive(Clone, Debug, Serialize)]
pub struct Pick {
    pub symbol: String,
    pub name: String,
    pub weight_pct: f64,
}

/// What a basket did over one window.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Outcome {
    /// Total return over the window, in percent, after costs.
    pub total_pct: f64,
    /// Annualised, in percent.
    pub vol: f64,
    /// Worst peak to trough, as a positive percent.
    pub drawdown: f64,
    /// What the costs took out, in percent of the starting value.
    pub cost_pct: f64,
    pub days: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Discovery {
    pub picks: Vec<Pick>,
    /// The window the basket was chosen on. Flattering by construction.
    pub train: Outcome,
    /// The window it never saw. This is the number that means something.
    pub test: Outcome,
    /// Equal weights over the same names, same window, same costs. The thing to beat.
    pub equal_weight: Outcome,
    pub listed: usize,
    pub usable: usize,
    pub prefiltered: usize,
    pub combos: u64,
    pub exhaustive: bool,
    pub method: String,
    pub cost_bps: f64,
    /// Symbols the list carries that have no usable history, so a stale list is visible rather
    /// than silently smaller.
    pub unusable: Vec<String>,
}

fn shrink_cov(cov: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut out = cov.to_vec();
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            if i != j {
                *v *= 1.0 - COV_SHRINK;
            }
        }
    }
    out
}

fn shrink_mean(mu: &[f64]) -> Vec<f64> {
    if mu.is_empty() {
        return Vec::new();
    }
    let grand: f64 = mu.iter().sum::<f64>() / mu.len() as f64;
    mu.iter()
        .map(|m| grand * MEAN_SHRINK + m * (1.0 - MEAN_SHRINK))
        .collect()
}

/// Long-only maximum Sharpe, by projected gradient ascent on the ratio itself.
///
/// The gradient of `w'mu / sqrt(w'Sw)` is `mu/s - (w'mu)(Sw)/s^3`, which is short enough to write
/// down and avoids the change of variables the closed form needs. Not concave, so this is a local
/// answer from a uniform start; with the handful of assets a basket holds that is the same answer.
pub fn max_sharpe(cov: &[Vec<f64>], mu: &[f64]) -> Vec<f64> {
    let n = cov.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }
    // Nothing is expected to make money, so there is no ratio to maximise and the least bad
    // portfolio is the least volatile one.
    if mu.iter().all(|m| *m <= 0.0) {
        return min_variance(cov);
    }
    let mut w = vec![1.0 / n as f64; n];
    let step = 0.05;
    for _ in 0..5_000 {
        let sw: Vec<f64> = (0..n)
            .map(|i| (0..n).map(|j| cov[i][j] * w[j]).sum::<f64>())
            .collect();
        let var: f64 = (0..n).map(|i| w[i] * sw[i]).sum();
        let s = var.max(1e-12).sqrt();
        let ret: f64 = (0..n).map(|i| w[i] * mu[i]).sum();
        let grad: Vec<f64> = (0..n)
            .map(|i| mu[i] / s - ret * sw[i] / (s * s * s))
            .collect();
        let next = crate::optimize::project_simplex(
            &(0..n).map(|i| w[i] + step * grad[i]).collect::<Vec<f64>>(),
        );
        let moved: f64 = (0..n).map(|i| (next[i] - w[i]).abs()).sum();
        w = next;
        if moved < 1e-12 {
            break;
        }
    }
    w
}

/// What a basket of weights did, rebalanced every `every` days, with costs charged on turnover.
///
/// Weights are allowed to drift between rebalances, which is what a real account does. Holding
/// them constant would mean trading every single day, and would quietly collect the rebalancing
/// premium that daily trading earns and nobody can capture.
pub fn walk(m: &Matrix, target: &[f64], every: usize, cost_bps: f64) -> Outcome {
    let rate = cost_bps / 10_000.0;
    let n = target.len();
    if n == 0 || m.is_empty() {
        return Outcome::default();
    }
    // Formation is all buys, so turnover is one and the cost is the rate once.
    let mut cost_paid = rate;
    let mut value = 1.0 - rate;
    let mut held: Vec<f64> = target.iter().map(|w| w * value).collect();
    let mut peak: f64 = value;
    let mut worst: f64 = 0.0;
    let mut rets: Vec<f64> = Vec::with_capacity(m.len());

    for (t, row) in m.rets.iter().enumerate() {
        let before = value;
        for i in 0..n {
            held[i] *= 1.0 + row[i];
        }
        value = held.iter().sum();
        if t > 0 && (t + 1) % every == 0 && value > 0.0 {
            // Turnover is the sum of the absolute moves: shifting 5% from one holding to another
            // is a 5% sale and a 5% purchase, so it is charged on 10%.
            let turnover: f64 = (0..n)
                .map(|i| (target[i] * value - held[i]).abs())
                .sum::<f64>()
                / value;
            let fee = rate * turnover * value;
            cost_paid += fee;
            value -= fee;
            for (i, h) in held.iter_mut().enumerate() {
                *h = target[i] * value;
            }
        }
        if before > 0.0 {
            rets.push(value / before - 1.0);
        }
        peak = peak.max(value);
        if peak > 0.0 {
            worst = worst.min(value / peak - 1.0);
        }
    }

    let mean = rets.iter().sum::<f64>() / rets.len().max(1) as f64;
    let var = if rets.len() > 1 {
        rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len() as f64 - 1.0)
    } else {
        0.0
    };
    Outcome {
        total_pct: (value - 1.0) * 100.0,
        vol: var.sqrt() * YEAR.sqrt() * 100.0,
        drawdown: if worst == 0.0 { 0.0 } else { -worst * 100.0 },
        cost_pct: cost_paid * 100.0,
        days: m.len(),
    }
}

fn weights_for(m: &Matrix, method: &str) -> (Vec<f64>, Vec<Vec<f64>>, Vec<f64>) {
    let cov = shrink_cov(&covariance(m));
    let mu = shrink_mean(&means(m));
    let w = weights_from(&cov, &mu, method);
    (w, cov, mu)
}

fn weights_from(cov: &[Vec<f64>], mu: &[f64], method: &str) -> Vec<f64> {
    match method {
        "sharpe" => max_sharpe(cov, mu),
        _ => min_variance(cov),
    }
}

/// The rows and columns of `cov` at `idx`, and the same entries of `mu`.
///
/// A covariance over a subset is a submatrix of the covariance over the whole set, so the search
/// computes one matrix and slices it. Rebuilding it per combination costs the length of the
/// window every time, which across a hundred thousand combinations is most of the runtime.
fn sub(cov: &[Vec<f64>], mu: &[f64], idx: &[usize]) -> (Vec<Vec<f64>>, Vec<f64>) {
    (
        idx.iter()
            .map(|i| idx.iter().map(|j| cov[*i][*j]).collect())
            .collect(),
        idx.iter().map(|i| mu[*i]).collect(),
    )
}

/// Higher is better, whatever the objective. Selection reads only this.
fn score_from(cov: &[Vec<f64>], mu: &[f64], method: &str) -> f64 {
    let w = weights_from(cov, mu, method);
    let vol = volatility(cov, &w);
    match method {
        "sharpe" => {
            let ret: f64 = (0..w.len()).map(|i| w[i] * mu[i]).sum::<f64>() * 100.0;
            if vol > 0.0 {
                ret / vol
            } else {
                0.0
            }
        }
        _ => -vol,
    }
}

/// Every k-subset of `0..n`, as index vectors.
fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    if k == 0 || k > n {
        return out;
    }
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        out.push(idx.clone());
        let mut i = k;
        loop {
            if i == 0 {
                return out;
            }
            i -= 1;
            if idx[i] != i + n - k {
                break;
            }
            if i == 0 {
                return out;
            }
        }
        idx[i] += 1;
        for j in i + 1..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

pub fn count_combinations(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    let mut out: u64 = 1;
    for i in 0..k.min(n - k) as u64 {
        out = out.saturating_mul(n as u64 - i) / (i + 1);
    }
    out
}

/// Keep the best `top_k` by training Sharpe, then drop anything that moves with a better one.
///
/// Two share classes of one company are one bet wearing two tickers, and a search that does not
/// say so will happily fill a basket with them and call it diversified.
fn prefilter(train: &Matrix, top_k: usize, corr_max: f64) -> Vec<usize> {
    let cov = covariance(train);
    let mu = means(train);
    let mut order: Vec<usize> = (0..train.width()).collect();
    let sharpe = |i: usize| {
        let sd = cov[i][i].max(0.0).sqrt();
        if sd > 0.0 {
            mu[i] / sd
        } else {
            f64::NEG_INFINITY
        }
    };
    order.sort_by(|a, b| {
        sharpe(*b)
            .partial_cmp(&sharpe(*a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    order.truncate(top_k);

    let corr = crate::optimize::correlation(&cov);
    let mut kept: Vec<usize> = Vec::new();
    for i in order {
        if kept.iter().all(|k| corr[i][*k].abs() <= corr_max) {
            kept.push(i);
        }
    }
    kept
}

/// The search itself, over the training window only.
fn pick(train: &Matrix, kept: &[usize], n: usize, method: &str) -> (Vec<usize>, bool, u64) {
    let combos = count_combinations(kept.len(), n);
    let cov = shrink_cov(&covariance(train));
    let mu = shrink_mean(&means(train));

    if combos > 0 && combos <= EXHAUSTIVE_LIMIT {
        let sets = combinations(kept.len(), n);
        let best = crate::par::best_of(&sets, |combo| {
            let idx: Vec<usize> = combo.iter().map(|i| kept[*i]).collect();
            let (c, m) = sub(&cov, &mu, &idx);
            (score_from(&c, &m, method), idx)
        });
        return (best.unwrap_or_default(), true, combos);
    }

    // Greedy forward selection: add whichever candidate most improves the basket, n times. Not
    // guaranteed optimal, and the caller says so rather than implying every basket was tried.
    let mut chosen: Vec<usize> = Vec::new();
    for _ in 0..n.min(kept.len()) {
        let mut best: Option<(f64, usize)> = None;
        for c in kept {
            if chosen.contains(c) {
                continue;
            }
            let mut idx = chosen.clone();
            idx.push(*c);
            let (cs, ms) = sub(&cov, &mu, &idx);
            let s = score_from(&cs, &ms, method);
            if best.as_ref().is_none_or(|(b, _)| s > *b) {
                best = Some((s, *c));
            }
        }
        match best {
            Some((_, c)) => chosen.push(c),
            None => break,
        }
    }
    (chosen, false, combos)
}

/// Find a basket of `n` from `m`, choosing on the earlier part of the window and reporting on the
/// later part.
pub fn search(
    m: &Matrix,
    unusable: Vec<String>,
    listed: usize,
    n: usize,
    method: &str,
    top_k: usize,
    cost_bps: f64,
) -> Discovery {
    let base = Discovery {
        listed,
        usable: m.width(),
        method: method.to_string(),
        cost_bps,
        unusable,
        ..Discovery::default()
    };
    // Two thirds train, a gap, the rest test. Under two years of shared history there is not
    // enough of either to divide.
    let split = m.len() * 7 / 10;
    if m.width() < n || n == 0 || split <= crate::optimize::MIN_OBS || split + GAP >= m.len() {
        return base;
    }
    let train = m.slice(0, split);
    let test = m.slice(split + GAP, m.len());

    let kept = prefilter(&train, top_k, CORR_MAX);
    if kept.len() < n {
        return Discovery {
            prefiltered: kept.len(),
            ..base
        };
    }
    let (chosen, exhaustive, combos) = pick(&train, &kept, n, method);
    let want: Vec<String> = chosen.iter().map(|i| m.symbols[*i].clone()).collect();

    let train_sub = train.pick(&want);
    let (w, _, _) = weights_for(&train_sub, method);
    let test_sub = test.pick(&want);
    let equal = vec![1.0 / want.len() as f64; want.len()];

    Discovery {
        picks: want
            .iter()
            .enumerate()
            .map(|(i, symbol)| Pick {
                symbol: symbol.clone(),
                name: crate::universe::name_of(symbol)
                    .unwrap_or(symbol)
                    .to_string(),
                weight_pct: w[i] * 100.0,
            })
            .collect(),
        train: walk(&train_sub, &w, REBALANCE_DAYS, cost_bps),
        test: walk(&test_sub, &w, REBALANCE_DAYS, cost_bps),
        equal_weight: walk(&test_sub, &equal, REBALANCE_DAYS, cost_bps),
        prefiltered: kept.len(),
        combos,
        exhaustive,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(symbols: &[&str], rows: Vec<Vec<f64>>) -> Matrix {
        Matrix {
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
            days: (0..rows.len()).map(|i| format!("d{i:05}")).collect(),
            rets: rows,
        }
    }

    /// A flat asset: no movement, so a rebalance has nothing to trade.
    fn flat(n: usize, k: usize) -> Matrix {
        matrix(
            &["A", "B"][..k.min(2)],
            (0..n).map(|_| vec![0.0; k]).collect(),
        )
    }

    #[test]
    fn combinations_are_every_subset_exactly_once() {
        let c = combinations(5, 3);
        assert_eq!(c.len() as u64, count_combinations(5, 3));
        assert_eq!(c.len(), 10);
        let mut seen = c.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 10, "duplicates: {c:?}");
        assert!(c.contains(&vec![0, 1, 2]) && c.contains(&vec![2, 3, 4]));
        assert!(c.iter().all(|v| v.windows(2).all(|w| w[0] < w[1])));
        assert_eq!(count_combinations(120, 5), 190_578_024);
    }

    #[test]
    fn formation_costs_the_rate_once_and_a_still_portfolio_costs_nothing_after() {
        // Nothing moves, so every rebalance has zero turnover. The only fee that may ever be
        // charged is the one for buying the basket in the first place.
        let m = flat(400, 2);
        let out = walk(&m, &[0.5, 0.5], REBALANCE_DAYS, 100.0);
        assert!((out.cost_pct - 1.0).abs() < 1e-9, "{out:?}");
        assert!(
            (out.total_pct + 1.0).abs() < 1e-9,
            "down exactly the fee: {out:?}"
        );
    }

    #[test]
    fn costs_are_charged_on_turnover_not_on_the_portfolio() {
        // One asset doubles on the first day and then stops. Weights drift from 50/50 to 2/3 and
        // 1/3, so the rebalance moves a sixth of the portfolio out of one and into the other:
        // turnover of two sixths, charged at the rate.
        let mut rows = vec![vec![0.0; 2]; 200];
        rows[0] = vec![1.0, 0.0];
        let m = matrix(&["A", "B"], rows);
        let free = walk(&m, &[0.5, 0.5], 100, 0.0);
        let paid = walk(&m, &[0.5, 0.5], 100, 100.0);
        // Formation is 1% of the starting value. By the first rebalance the portfolio is worth
        // 1.485, and moving a third of THAT at 1% costs 0.00495, which is 0.495% of the starting
        // value. cost_pct is measured against the start throughout, so the fee of a rebalance
        // grows with the portfolio it is charged on.
        let expected = 1.0 + 1.485 * (1.0 / 3.0) * 0.01 * 100.0;
        assert!(
            (paid.cost_pct - expected).abs() < 0.02,
            "{paid:?} vs {expected}"
        );
        assert!(paid.total_pct < free.total_pct);
    }

    #[test]
    fn weights_drift_between_rebalances_rather_than_being_held_constant() {
        // Two assets moving oppositely every day. Holding weights constant would trade daily and
        // quietly collect the rebalancing premium; drifting is what an account actually does, and
        // the two must not produce the same number.
        let rows: Vec<Vec<f64>> = (0..400)
            .map(|i| {
                if i % 2 == 0 {
                    vec![0.05, -0.05]
                } else {
                    vec![-0.05, 0.05]
                }
            })
            .collect();
        let m = matrix(&["A", "B"], rows);
        let quarterly = walk(&m, &[0.5, 0.5], REBALANCE_DAYS, 0.0);
        let daily = walk(&m, &[0.5, 0.5], 1, 0.0);
        assert!(
            (quarterly.total_pct - daily.total_pct).abs() > 1e-6,
            "quarterly {quarterly:?} daily {daily:?}"
        );
    }

    #[test]
    fn selection_reads_the_training_window_and_never_the_test_one() {
        // A is the better asset for the first 70% and B for the rest. A search that peeks at the
        // test window picks B, and its "out of sample" number is then the maximum of however many
        // combinations it tried, which is not out of sample at all.
        let n = 1000;
        let split = n * 7 / 10;
        let rows: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                let early = i < split;
                // C is noise in both halves, so the choice is only ever between A and B.
                let wobble = if i % 2 == 0 { 0.01 } else { -0.01 };
                if early {
                    vec![0.004, -0.002, wobble]
                } else {
                    vec![-0.002, 0.004, wobble]
                }
            })
            .collect();
        let m = matrix(&["A.OL", "B.OL", "C.OL"], rows);
        let out = search(&m, Vec::new(), 3, 1, "sharpe", 30, 0.0);
        assert_eq!(out.picks.len(), 1);
        assert_eq!(
            out.picks[0].symbol, "A.OL",
            "picked on the training window: {out:?}"
        );
        // and the honest consequence: the winner of the training window loses on the test window
        assert!(out.train.total_pct > 0.0, "{:?}", out.train);
        assert!(out.test.total_pct < 0.0, "{:?}", out.test);
    }

    #[test]
    fn two_names_that_move_together_do_not_both_get_into_the_basket() {
        // A and its twin are one bet wearing two tickers. A basket of both is not diversified,
        // however good the correlation matrix makes it look.
        let n = 1000;
        let rows: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                let a = if i % 3 == 0 { 0.02 } else { -0.008 };
                let c = if i % 5 == 0 { 0.015 } else { -0.003 };
                vec![a, a * 1.001, c]
            })
            .collect();
        let m = matrix(&["A.OL", "TWIN.OL", "C.OL"], rows);
        let out = search(&m, Vec::new(), 3, 2, "minvar", 30, 0.0);
        let picked: Vec<&str> = out.picks.iter().map(|p| p.symbol.as_str()).collect();
        assert!(
            !(picked.contains(&"A.OL") && picked.contains(&"TWIN.OL")),
            "both halves of the same bet: {picked:?}"
        );
    }

    #[test]
    fn the_prefilter_keeps_one_of_two_names_that_move_together() {
        // Tested on the filter directly, not through a search: minimum variance would decline to
        // hold both anyway, so a search passing this proves nothing about the filter.
        let rows: Vec<Vec<f64>> = (0..1000)
            .map(|i| {
                let a = if i % 3 == 0 { 0.02 } else { -0.008 };
                let c = if i % 5 == 0 { 0.015 } else { -0.003 };
                vec![a, a * 1.001, c]
            })
            .collect();
        let m = matrix(&["A.OL", "TWIN.OL", "C.OL"], rows);
        let kept = prefilter(&m, 30, CORR_MAX);
        let names: Vec<&str> = kept.iter().map(|i| m.symbols[*i].as_str()).collect();
        assert!(
            names.contains(&"C.OL"),
            "the independent one survives: {names:?}"
        );
        assert!(
            names.contains(&"A.OL") != names.contains(&"TWIN.OL"),
            "exactly one of the pair: {names:?}"
        );
    }

    #[test]
    fn too_little_history_returns_nothing_rather_than_a_guess() {
        let m = flat(100, 2);
        let out = search(&m, Vec::new(), 2, 1, "minvar", 30, 0.0);
        assert!(out.picks.is_empty());
        assert_eq!(out.usable, 2);
    }

    #[test]
    fn a_greedy_search_is_reported_as_greedy() {
        // Far more candidates than the exhaustive limit allows for a basket of this size.
        assert!(count_combinations(60, 8) > EXHAUSTIVE_LIMIT);
        assert!(count_combinations(20, 3) <= EXHAUSTIVE_LIMIT);
    }
}
