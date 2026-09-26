//! What has changed about each holding, and which holdings are really one bet.
//!
//! Two questions over the same daily returns in base currency. First, against its own past: is
//! a holding swinging harder than it used to, has it stopped moving with the rest of the
//! portfolio, did it just move further than it normally does. Second, across holdings: which of
//! them move together closely enough to be one position under several names.
//!
//! Every holding is measured against ITS OWN baseline. A shipping stock that swings 3% a day is
//! not an outlier for doing so; it is one when it starts swinging 6%.

use std::collections::HashMap;

use serde::Serialize;

use crate::history::Series;
use crate::optimize::{self, Matrix, YEAR};

/// The recent window: about three months of trading days.
pub const RECENT: usize = 63;
/// What the recent window is compared against: the year before it.
pub const BASELINE: usize = 252;

/// Volatility this many times the baseline, or its inverse, is a change worth naming.
pub const VOL_RATIO: f64 = 1.5;
/// A correlation with the rest of the portfolio that moved this far is a change of behaviour.
pub const CORR_SHIFT: f64 = 0.3;
/// A move this many baseline deviations out. Three is rare enough to mean something for a
/// normal distribution and still fires on fat-tailed days that are genuinely unusual.
pub const MOVE_Z: f64 = 3.0;
/// Holdings whose average correlation is at least this are grouped together.
pub const CLUSTER_CORR: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    Volatility,
    Decoupling,
    Move,
}

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub symbol: String,
    /// Annualised, in percent.
    pub vol_baseline: f64,
    pub vol_recent: f64,
    /// Recent over baseline. Zero when the baseline never moved, meaning "nothing to compare".
    pub vol_ratio: f64,
    /// Correlation with the rest of the portfolio, weighted as it is held. None with one holding.
    pub corr_baseline: Option<f64>,
    pub corr_recent: Option<f64>,
    /// The last day in the series, and the moves ending on it, in percent.
    pub day: String,
    pub move_1d: f64,
    pub move_5d: f64,
    /// Each move in baseline deviations, scaled for its length.
    pub z_1d: f64,
    pub z_5d: f64,
    pub flags: Vec<Flag>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Cluster {
    pub symbols: Vec<String>,
    /// The average correlation between every pair in the group.
    pub avg_corr: f64,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Report {
    pub rows: Vec<Row>,
    /// Groups of two or more. A holding in none of them moves on its own.
    pub clusters: Vec<Cluster>,
    /// Holdings with too little shared history to have a baseline, named rather than dropped.
    pub excluded: Vec<String>,
    /// First and last day the figures cover, and where the recent window starts.
    pub from: String,
    pub recent_from: String,
    pub through: String,
}

/// The whole report for a set of holdings. `weights` are the portfolio's current value weights by
/// symbol; a symbol missing from it, or a portfolio worth nothing yet, is weighted equally.
pub fn analyse(
    symbols: &[String],
    weights: &HashMap<String, f64>,
    base: &str,
    histories: &HashMap<String, Series>,
    fx: &HashMap<String, Series>,
) -> Report {
    let need = RECENT + BASELINE;
    // Checked one at a time first: the matrix keeps only days every symbol shares, so one
    // holding listed last spring would otherwise cut every other holding's baseline short.
    let (long, mut excluded): (Vec<String>, Vec<String>) = symbols.iter().cloned().partition(|s| {
        optimize::build(std::slice::from_ref(s), base, histories, fx)
            .0
            .len()
            >= need
    });
    let (m, dropped) = optimize::build(&long, base, histories, fx);
    excluded.extend(dropped);
    if m.len() < need {
        excluded.extend(m.symbols.iter().cloned());
        excluded.sort();
        return Report {
            excluded,
            ..Report::default()
        };
    }
    let m = m.slice(m.len() - need, m.len());
    let (baseline, recent) = (m.slice(0, BASELINE), m.slice(BASELINE, need));

    let held: Vec<f64> = m
        .symbols
        .iter()
        .map(|s| weights.get(s).copied().unwrap_or(0.0))
        .collect();
    let w = if held.iter().sum::<f64>() > 0.0 {
        held
    } else {
        vec![1.0; m.width()]
    };

    let rows = (0..m.width())
        .map(|i| row(i, &baseline, &recent, &w))
        .collect();
    let corr = optimize::correlation(&optimize::covariance(&m));
    Report {
        rows,
        clusters: clusters(&m.symbols, &corr),
        excluded,
        from: m.days[0].clone(),
        recent_from: recent.days[0].clone(),
        through: m.days[m.len() - 1].clone(),
    }
}

fn row(i: usize, baseline: &Matrix, recent: &Matrix, w: &[f64]) -> Row {
    let col = |m: &Matrix| -> Vec<f64> { m.rets.iter().map(|r| r[i]).collect() };
    let (b, r) = (col(baseline), col(recent));
    let (sd_b, sd_r) = (sd(&b), sd(&r));
    let vol_ratio = if sd_b > 0.0 { sd_r / sd_b } else { 0.0 };

    let (corr_baseline, corr_recent) = if w.len() > 1 {
        (
            pearson(&b, &rest(baseline, i, w)),
            pearson(&r, &rest(recent, i, w)),
        )
    } else {
        (None, None)
    };

    let compound = |xs: &[f64]| xs.iter().fold(1.0, |a, x| a * (1.0 + x)) - 1.0;
    let move_1d = r[r.len() - 1];
    let move_5d = compound(&r[r.len() - 5..]);
    let z = |mv: f64, days: f64| {
        if sd_b > 0.0 {
            mv / (sd_b * days.sqrt())
        } else {
            0.0
        }
    };
    let (z_1d, z_5d) = (z(move_1d, 1.0), z(move_5d, 5.0));

    let mut flags = Vec::new();
    if vol_ratio >= VOL_RATIO || (vol_ratio > 0.0 && vol_ratio <= 1.0 / VOL_RATIO) {
        flags.push(Flag::Volatility);
    }
    if let (Some(a), Some(b)) = (corr_baseline, corr_recent) {
        if (b - a).abs() >= CORR_SHIFT {
            flags.push(Flag::Decoupling);
        }
    }
    if z_1d.abs() >= MOVE_Z || z_5d.abs() >= MOVE_Z {
        flags.push(Flag::Move);
    }

    Row {
        symbol: baseline.symbols[i].clone(),
        vol_baseline: sd_b * YEAR.sqrt() * 100.0,
        vol_recent: sd_r * YEAR.sqrt() * 100.0,
        vol_ratio,
        corr_baseline,
        corr_recent,
        day: recent.days[recent.len() - 1].clone(),
        move_1d: move_1d * 100.0,
        move_5d: move_5d * 100.0,
        z_1d,
        z_5d,
        flags,
    }
}

/// The rest of the portfolio's daily return, without holding `i`, weighted as held.
fn rest(m: &Matrix, i: usize, w: &[f64]) -> Vec<f64> {
    let total: f64 = w
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, x)| x)
        .sum();
    m.rets
        .iter()
        .map(|r| {
            if total <= 0.0 {
                return 0.0;
            }
            r.iter()
                .zip(w)
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, (x, wj))| x * wj)
                .sum::<f64>()
                / total
        })
        .collect()
}

/// Sample standard deviation of daily returns.
fn sd(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (xs.len() as f64 - 1.0)).sqrt()
}

/// None when either side never moved: a correlation with a flat line is undefined, not zero.
fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len()) as f64;
    if n < 2.0 {
        return None;
    }
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma).powi(2);
        sbb += (y - mb).powi(2);
    }
    (saa > 0.0 && sbb > 0.0).then(|| sab / (saa * sbb).sqrt())
}

/// Average-linkage hierarchical clustering on correlation, stopped at `CLUSTER_CORR`.
///
/// Deterministic: the closest pair merges first, and a tie goes to the lower indices, so the same
/// holdings always group the same way. Every group has a plain reason, its average correlation,
/// which is the point of using this over a projection with axes that mean nothing.
pub fn clusters(symbols: &[String], corr: &[Vec<f64>]) -> Vec<Cluster> {
    let mut groups: Vec<Vec<usize>> = (0..symbols.len()).map(|i| vec![i]).collect();
    let link = |a: &[usize], b: &[usize]| -> f64 {
        let s: f64 = a
            .iter()
            .flat_map(|i| b.iter().map(move |j| corr[*i][*j]))
            .sum();
        s / (a.len() * b.len()) as f64
    };
    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for a in 0..groups.len() {
            for b in a + 1..groups.len() {
                let c = link(&groups[a], &groups[b]);
                if best.is_none_or(|(_, _, x)| c > x) {
                    best = Some((a, b, c));
                }
            }
        }
        match best {
            Some((a, b, c)) if c >= CLUSTER_CORR => {
                let moved = groups.remove(b);
                groups[a].extend(moved);
                groups[a].sort();
            }
            _ => break,
        }
    }
    let mut out: Vec<Cluster> = groups
        .into_iter()
        .filter(|g| g.len() > 1)
        .map(|g| {
            let pairs: Vec<f64> = g
                .iter()
                .enumerate()
                .flat_map(|(k, i)| g[k + 1..].iter().map(move |j| corr[*i][*j]))
                .collect();
            Cluster {
                symbols: g.iter().map(|i| symbols[*i].clone()).collect(),
                avg_corr: pairs.iter().sum::<f64>() / pairs.len() as f64,
            }
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.symbols.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Bar;

    /// A series from daily returns, on consecutive made-up days that sort in order.
    fn from_returns(rets: &[f64]) -> Series {
        let mut close = 100.0;
        let mut bars = vec![Bar {
            day: "d00000".into(),
            close,
        }];
        for (i, r) in rets.iter().enumerate() {
            close *= 1.0 + r;
            bars.push(Bar {
                day: format!("d{:05}", i + 1),
                close,
            });
        }
        Series {
            currency: "NOK".into(),
            bars,
        }
    }

    /// Alternating +a, -a: a known daily deviation with no drift.
    fn swing(a: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| if i % 2 == 0 { a } else { -a }).collect()
    }

    fn one(sym: &str, rets: Vec<f64>) -> Report {
        let mut h = HashMap::new();
        h.insert(sym.to_string(), from_returns(&rets));
        analyse(
            &[sym.to_string()],
            &HashMap::new(),
            "NOK",
            &h,
            &HashMap::new(),
        )
    }

    #[test]
    fn a_holding_swinging_twice_as_hard_as_its_own_year_is_flagged() {
        let mut r = swing(0.01, BASELINE + 1);
        r.extend(swing(0.02, RECENT - 1));
        let rep = one("A.OL", r);
        let row = &rep.rows[0];
        assert!((row.vol_ratio - 2.0).abs() < 0.05, "{}", row.vol_ratio);
        assert!(row.flags.contains(&Flag::Volatility));
    }

    #[test]
    fn a_volatile_holding_that_stays_volatile_is_not_an_outlier() {
        let rep = one("A.OL", swing(0.04, BASELINE + RECENT + 1));
        assert!(rep.rows[0].flags.is_empty(), "{:?}", rep.rows[0]);
    }

    #[test]
    fn a_day_far_outside_the_baseline_is_a_move_and_says_how_far() {
        let mut r = swing(0.01, BASELINE + RECENT);
        *r.last_mut().expect("a day") = -0.06;
        let rep = one("A.OL", r);
        let row = &rep.rows[0];
        assert!((row.move_1d + 6.0).abs() < 1e-9);
        assert!(row.z_1d < -5.0, "{}", row.z_1d);
        assert!(row.flags.contains(&Flag::Move));
    }

    #[test]
    fn too_little_history_is_named_rather_than_measured() {
        let rep = one("NEW.OL", swing(0.01, 100));
        assert!(rep.rows.is_empty());
        assert_eq!(rep.excluded, vec!["NEW.OL".to_string()]);
    }

    #[test]
    fn a_short_holding_does_not_cut_the_others_baseline() {
        let mut h = HashMap::new();
        h.insert("OLD.OL".to_string(), from_returns(&swing(0.01, 400)));
        h.insert("NEW.OL".to_string(), from_returns(&swing(0.01, 100)));
        let rep = analyse(
            &["OLD.OL".to_string(), "NEW.OL".to_string()],
            &HashMap::new(),
            "NOK",
            &h,
            &HashMap::new(),
        );
        assert_eq!(rep.rows.len(), 1);
        assert_eq!(rep.rows[0].symbol, "OLD.OL");
        assert_eq!(rep.excluded, vec!["NEW.OL".to_string()]);
    }

    #[test]
    fn a_holding_that_stops_moving_with_the_rest_is_decoupled() {
        // B follows A for the baseline year, then moves against it.
        let n = BASELINE + RECENT;
        let a: Vec<f64> = (0..n)
            .map(|i| 0.01 * ((i * 7 % 11) as f64 - 5.0) / 5.0)
            .collect();
        let b: Vec<f64> = a
            .iter()
            .enumerate()
            .map(|(i, x)| if i < BASELINE { *x } else { -x })
            .collect();
        let mut h = HashMap::new();
        h.insert("A.OL".to_string(), from_returns(&a));
        h.insert("B.OL".to_string(), from_returns(&b));
        let rep = analyse(
            &["A.OL".to_string(), "B.OL".to_string()],
            &HashMap::new(),
            "NOK",
            &h,
            &HashMap::new(),
        );
        let row = rep.rows.iter().find(|r| r.symbol == "B.OL").expect("B");
        assert!(row.corr_baseline.expect("defined") > 0.99);
        assert!(row.corr_recent.expect("defined") < -0.99);
        assert!(row.flags.contains(&Flag::Decoupling));
    }

    #[test]
    fn holdings_that_move_together_cluster_and_the_loner_does_not() {
        let s = |x: &str| x.to_string();
        let corr = vec![
            vec![1.0, 0.8, 0.7, 0.1],
            vec![0.8, 1.0, 0.75, 0.0],
            vec![0.7, 0.75, 1.0, 0.2],
            vec![0.1, 0.0, 0.2, 1.0],
        ];
        let got = clusters(&[s("A"), s("B"), s("C"), s("D")], &corr);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].symbols, vec![s("A"), s("B"), s("C")]);
        assert!((got[0].avg_corr - 0.75).abs() < 1e-9);
    }

    #[test]
    fn clustering_the_same_input_twice_gives_the_same_groups() {
        let s = |x: &str| x.to_string();
        // Two pairs tied at exactly the threshold.
        let corr = vec![
            vec![1.0, 0.5, 0.0, 0.0],
            vec![0.5, 1.0, 0.0, 0.0],
            vec![0.0, 0.0, 1.0, 0.5],
            vec![0.0, 0.0, 0.5, 1.0],
        ];
        let names = [s("A"), s("B"), s("C"), s("D")];
        let a = clusters(&names, &corr);
        assert_eq!(a, clusters(&names, &corr));
        assert_eq!(a.len(), 2, "a correlation at the threshold groups");
    }
}
