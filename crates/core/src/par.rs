//! Running a handful of slow calls at once.
//!
//! ponytail: scoped threads and an atomic cursor, which is a worker pool in a dozen lines. An
//! async runtime would mean making every caller async to await one burst of requests.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// The most calls in flight at once. Yahoo's endpoints are unofficial and unpaid; eight parallel
/// requests is a portfolio asking politely, not a scraper.
pub const MAX_PARALLEL: usize = 8;

/// Apply `f` to every item, `MAX_PARALLEL` at a time.
///
/// Results come back in completion order, not input order: every caller either builds a map from
/// them or sorts them for display.
pub fn map<T, F>(items: &[String], f: F) -> Vec<(String, T)>
where
    T: Send,
    F: Fn(&str) -> T + Sync,
{
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|sc| {
        for _ in 0..items.len().min(MAX_PARALLEL) {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { return };
                let got = f(item);
                out.lock().expect("par::map lock").push((item.clone(), got));
            });
        }
    });
    out.into_inner().expect("par::map lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn every_item_comes_back_exactly_once() {
        let items: Vec<String> = (0..20).map(|i| format!("S{i}")).collect();
        let got = map(&items, |s| s.len());
        assert_eq!(got.len(), 20);
        let mut names: Vec<&str> = got.iter().map(|(s, _)| s.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 20, "an item was processed twice or dropped");
    }

    #[test]
    fn the_work_runs_in_parallel_but_never_more_than_the_cap() {
        let items: Vec<String> = (0..24).map(|i| format!("S{i}")).collect();
        let inflight = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let got = map(&items, |_| {
            let n = inflight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            inflight.fetch_sub(1, Ordering::SeqCst);
        });
        assert_eq!(got.len(), 24);
        let peak = peak.load(Ordering::SeqCst);
        // Serial is the bug this exists to prevent: 24 symbols one at a time is a six second boot.
        assert!(peak > 1, "the work ran one item at a time");
        assert!(
            peak <= MAX_PARALLEL,
            "{peak} in flight, cap is {MAX_PARALLEL}"
        );
    }

    #[test]
    fn an_empty_list_spawns_nothing_and_returns_nothing() {
        assert!(map(&[], |_: &str| 1).is_empty());
    }
}
