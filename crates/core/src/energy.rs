//! Energy and shipping: what moves the Oslo market, and how much of it you are holding.
//!
//! Oslo Børs is an oil, gas and shipping index wearing a national flag, so the useful question
//! for a Nordic portfolio is not "what did my holdings do" but "what are they a bet on". This
//! module answers it the only way that survives scrutiny: the slope of each holding's daily
//! returns against a driver's, measured over a stated window, with the correlation beside it so
//! a slope fitted to noise can be recognised as one.
//!
//! **Everything here is keyless.** The Baltic Exchange's freight indices are licensed and are not
//! in this app. The two Breakwave funds hold freight futures directly and are ordinary Yahoo
//! tickers, which makes them the closest thing to a freight rate that can be had without a
//! contract. They are funds, with a fund's costs and roll, and the screen says so.

use crate::optimize::{covariance, Matrix};

/// Something a Nordic portfolio is, whether or not anyone chose it to be, a bet on.
pub struct Driver {
    pub symbol: &'static str,
    pub name: &'static str,
    /// What it actually is, since half of these are futures and two are funds.
    pub kind: &'static str,
}

pub const DRIVERS: &[Driver] = &[
    Driver {
        symbol: "BZ=F",
        name: "Brent crude",
        kind: "Front-month future",
    },
    Driver {
        symbol: "CL=F",
        name: "WTI crude",
        kind: "Front-month future",
    },
    Driver {
        symbol: "TTF=F",
        name: "Dutch TTF gas",
        kind: "Front-month future",
    },
    Driver {
        symbol: "NG=F",
        name: "Henry Hub gas",
        kind: "Front-month future",
    },
    Driver {
        symbol: "BDRY",
        name: "Dry bulk freight",
        kind: "Fund holding freight futures",
    },
    Driver {
        symbol: "BWET",
        name: "Tanker freight",
        kind: "Fund holding freight futures",
    },
];

pub fn driver_of(symbol: &str) -> Option<&'static Driver> {
    DRIVERS.iter().find(|d| d.symbol == symbol)
}

/// The Nordic energy and shipping listings, by what they carry.
///
/// ponytail: a hand-kept list, like `universe`. The alternative is a sector taxonomy from a data
/// vendor, and there is no keyless one worth having. A ticker that stops working is reported as
/// unusable rather than silently dropped, which is how a wrong one gets found: seven of these
/// were dead on the first run, either delisted or moved to New York, and the screen named them.
pub const LISTINGS: &[(&str, &str, &str)] = &[
    // Oil and gas producers
    ("EQNR.OL", "Equinor", "Oil & gas"),
    ("AKRBP.OL", "Aker BP", "Oil & gas"),
    ("VAR.OL", "Vår Energi", "Oil & gas"),
    ("DNO.OL", "DNO", "Oil & gas"),
    ("OKEA.OL", "OKEA", "Oil & gas"),
    ("BWO.OL", "BW Offshore", "Oil & gas"),
    // Oilfield services and drilling
    ("SUBC.OL", "Subsea 7", "Oil services"),
    ("AKSO.OL", "Aker Solutions", "Oil services"),
    ("TGS.OL", "TGS", "Oil services"),
    ("ODL.OL", "Odfjell Drilling", "Drilling"),
    ("BORR.OL", "Borr Drilling", "Drilling"),
    // Not SDRL.OL: Seadrill's Oslo line is gone and New York is where it trades.
    ("SDRL", "Seadrill", "Drilling"),
    // Crude and product tankers
    ("FRO.OL", "Frontline", "Tankers"),
    ("HAFNI.OL", "Hafnia", "Tankers"),
    ("OET.OL", "Okeanis Eco Tankers", "Tankers"),
    ("ODF.OL", "Odfjell A", "Tankers"),
    ("ODFB.OL", "Odfjell B", "Tankers"),
    ("TRMD-A.CO", "Torm", "Tankers"),
    ("DHT", "DHT Holdings", "Tankers"),
    ("INSW", "International Seaways", "Tankers"),
    ("SNI.OL", "Stolt-Nielsen", "Tankers"),
    // Dry bulk and combination carriers
    ("2020.OL", "2020 Bulkers", "Dry bulk"),
    ("HSHP.OL", "Himalaya Shipping", "Dry bulk"),
    ("JIN.OL", "Jinhui Shipping", "Dry bulk"),
    ("KCC.OL", "Klaveness Combination Carriers", "Dry bulk"),
    // CMB.TECH absorbed Golden Ocean, so GOGL.OL is where that exposure went.
    ("CMBT", "CMB.TECH", "Dry bulk"),
    // Gas carriers
    ("BWLPG.OL", "BW LPG", "Gas carriers"),
    ("ALNG.OL", "Awilco LNG", "Gas carriers"),
    // Flex LNG left Oslo for New York.
    ("FLNG", "Flex LNG", "Gas carriers"),
    // Container, car carriers and liner
    ("MPCC.OL", "MPC Container Ships", "Container"),
    ("HAUTO.OL", "Höegh Autoliners", "Car carriers"),
    ("WAWI.OL", "Wallenius Wilhelmsen", "Car carriers"),
    ("MAERSK-B.CO", "A.P. Møller Mærsk B", "Container"),
    // Owners and offshore support
    ("SFL", "SFL Corporation", "Ship owners"),
    ("SOFF.OL", "Solstad Offshore", "Offshore support"),
    ("DOFG.OL", "DOF Group", "Offshore support"),
];

pub fn name_of(ticker: &str) -> Option<&'static str> {
    LISTINGS
        .iter()
        .find(|(t, _, _)| *t == ticker)
        .map(|(_, n, _)| *n)
}

/// One listing measured against the driver.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Row {
    pub ticker: String,
    pub name: String,
    pub kind: String,
    /// Percent move in this for a 1% move in the driver, over the window.
    pub beta: f64,
    /// How much of the movement that slope actually explains, -1 to 1.
    pub correlation: f64,
    /// Annualised, percent.
    pub vol_pct: f64,
    /// Compounded over the window, percent.
    pub total_pct: f64,
    /// Whether the portfolio holds it.
    pub owned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Report {
    pub driver: String,
    pub driver_name: String,
    pub driver_kind: String,
    pub driver_total_pct: f64,
    pub driver_vol_pct: f64,
    pub from: String,
    pub to: String,
    pub observations: usize,
    pub rows: Vec<Row>,
    /// Asked for and not measurable: no history, or not enough of it to reach the window.
    pub unusable: Vec<String>,
}

/// Compound a column of daily returns into one percent figure.
fn total(m: &Matrix, col: usize) -> f64 {
    (m.rets.iter().fold(1.0, |acc, r| acc * (1.0 + r[col])) - 1.0) * 100.0
}

/// Every symbol in the matrix measured against the driver in it.
///
/// The slope is the covariance divided by the driver's own variance, which is the ordinary
/// least-squares fit of one return series on another with no intercept worth reporting. The
/// correlation beside it is not decoration: a beta of 1.4 with a correlation of 0.1 is a number
/// fitted to noise, and the screen shows both so that it can be read as one.
pub fn analyse(m: &Matrix, driver: &str, owned: &[String], unusable: Vec<String>) -> Report {
    let mut out = Report {
        driver: driver.to_string(),
        driver_name: driver_of(driver)
            .map(|d| d.name)
            .unwrap_or(driver)
            .to_string(),
        driver_kind: driver_of(driver).map(|d| d.kind).unwrap_or("").to_string(),
        unusable,
        ..Report::default()
    };
    let Some(d) = m.symbols.iter().position(|s| s == driver) else {
        // The driver itself could not be measured, so nothing can be measured against it.
        out.unusable.push(driver.to_string());
        return out;
    };
    let cov = covariance(m);
    let var = cov[d][d];
    out.observations = m.len();
    out.from = m.days.first().cloned().unwrap_or_default();
    out.to = m.days.last().cloned().unwrap_or_default();
    out.driver_total_pct = total(m, d);
    out.driver_vol_pct = var.sqrt() * 100.0;

    for (i, sym) in m.symbols.iter().enumerate() {
        if i == d {
            continue;
        }
        let sd = cov[i][i].sqrt();
        out.rows.push(Row {
            ticker: sym.clone(),
            name: name_of(sym).unwrap_or(sym).to_string(),
            kind: LISTINGS
                .iter()
                .find(|(t, _, _)| t == sym)
                .map(|(_, _, k)| *k)
                .unwrap_or("Held")
                .to_string(),
            // A driver that never moved has no slope. Zero says "no relationship measured",
            // which is exactly what a flat driver gives you.
            beta: if var > 0.0 { cov[i][d] / var } else { 0.0 },
            correlation: if sd > 0.0 && var > 0.0 {
                cov[i][d] / (sd * var.sqrt())
            } else {
                0.0
            },
            vol_pct: sd * 100.0,
            total_pct: total(m, i),
            owned: owned.contains(sym),
        });
    }
    // Most exposed first: that is the question the screen exists to answer.
    out.rows.sort_by(|a, b| {
        b.beta
            .partial_cmp(&a.beta)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A matrix built by hand: column 0 is the driver, and every other column is a stated
    /// multiple of it plus, where asked for, a wobble of its own.
    fn matrix(cols: Vec<(&str, Vec<f64>)>) -> Matrix {
        let days: Vec<String> = (0..cols[0].1.len())
            .map(|i| format!("2026-01-{:02}", i + 1))
            .collect();
        let rets = (0..cols[0].1.len())
            .map(|t| cols.iter().map(|(_, v)| v[t]).collect())
            .collect();
        Matrix {
            symbols: cols.iter().map(|(s, _)| s.to_string()).collect(),
            days,
            rets,
        }
    }

    fn driver_returns() -> Vec<f64> {
        (0..40)
            .map(|i| if i % 2 == 0 { 0.02 } else { -0.015 })
            .collect()
    }

    #[test]
    fn beta_is_the_slope_against_the_driver_and_a_perfect_follower_correlates_at_one() {
        let d = driver_returns();
        let twice: Vec<f64> = d.iter().map(|r| r * 2.0).collect();
        let half: Vec<f64> = d.iter().map(|r| r * 0.5).collect();
        let m = matrix(vec![("BZ=F", d), ("DOUBLE", twice), ("HALF", half)]);
        let r = analyse(&m, "BZ=F", &[], Vec::new());

        assert_eq!(r.rows.len(), 2, "the driver is not measured against itself");
        // Sorted by beta, most exposed first.
        assert_eq!(r.rows[0].ticker, "DOUBLE");
        assert!((r.rows[0].beta - 2.0).abs() < 1e-9, "{:?}", r.rows[0]);
        assert!((r.rows[1].beta - 0.5).abs() < 1e-9, "{:?}", r.rows[1]);
        for row in &r.rows {
            assert!(
                (row.correlation - 1.0).abs() < 1e-9,
                "a multiple of the driver moves with it exactly: {row:?}"
            );
        }
    }

    #[test]
    fn something_moving_against_the_driver_has_a_negative_beta() {
        let d = driver_returns();
        let against: Vec<f64> = d.iter().map(|r| -r).collect();
        let m = matrix(vec![("BZ=F", d), ("SHORT", against)]);
        let r = analyse(&m, "BZ=F", &[], Vec::new());
        assert!((r.rows[0].beta + 1.0).abs() < 1e-9, "{:?}", r.rows[0]);
        assert!((r.rows[0].correlation + 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_driver_that_never_moved_produces_no_slope_rather_than_a_division_by_zero() {
        // An infinity or a NaN would reach the screen and be rendered. Zero is the honest answer:
        // nothing was measured.
        let flat = vec![0.0; 40];
        let m = matrix(vec![("BZ=F", flat), ("ANY", driver_returns())]);
        let r = analyse(&m, "BZ=F", &[], Vec::new());
        assert_eq!(r.rows[0].beta, 0.0);
        assert_eq!(r.rows[0].correlation, 0.0);
    }

    #[test]
    fn a_missing_driver_measures_nothing_and_says_so() {
        // Otherwise the screen would show an empty table and let it read as "no exposure".
        let m = matrix(vec![("EQNR.OL", driver_returns())]);
        let r = analyse(&m, "BZ=F", &[], Vec::new());
        assert!(r.rows.is_empty());
        assert_eq!(r.unusable, vec!["BZ=F".to_string()]);
    }

    #[test]
    fn a_held_symbol_is_marked_and_named_even_when_it_is_not_on_the_list() {
        let d = driver_returns();
        let same = d.clone();
        let m = matrix(vec![("BZ=F", d), ("EQNR.OL", same.clone()), ("AAPL", same)]);
        let r = analyse(&m, "BZ=F", &["AAPL".to_string()], Vec::new());
        let aapl = r.rows.iter().find(|x| x.ticker == "AAPL").expect("aapl");
        assert!(aapl.owned);
        assert_eq!(
            aapl.kind, "Held",
            "not on the energy list, but still measured"
        );
        let eqnr = r.rows.iter().find(|x| x.ticker == "EQNR.OL").expect("eqnr");
        assert!(!eqnr.owned);
        assert_eq!(eqnr.name, "Equinor", "named from the list");
    }

    #[test]
    fn the_total_is_compounded_rather_than_summed() {
        // Ten days of +10% is +159%, not +100%. Summing returns is the oldest mistake here.
        let d = vec![0.1; 10];
        let m = matrix(vec![("BZ=F", d.clone()), ("SAME", d)]);
        let r = analyse(&m, "BZ=F", &[], Vec::new());
        let expect = (1.1f64.powi(10) - 1.0) * 100.0;
        assert!((r.driver_total_pct - expect).abs() < 1e-9, "{r:?}");
        assert!((r.rows[0].total_pct - expect).abs() < 1e-9);
    }
}
