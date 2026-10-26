//! Paired comparisons of two setups on the same cases (spec, Analysis): an
//! exact McNemar test and a 95% bootstrap interval of the difference in
//! accuracy.

/// The two-sided exact McNemar p-value: `b` cases only the first got
/// right, `c` only the second.
pub fn mcnemar(b: u64, c: u64) -> f64 {
    let n = b + c;
    if n == 0 {
        return 1.0;
    }
    let k = b.min(c);
    // P(X <= k) for X ~ Binomial(n, 1/2), in logs to stay finite.
    let ln_choose = |n: u64, k: u64| -> f64 { (1..=k).map(|i| ((n - k + i) as f64).ln() - (i as f64).ln()).sum() };
    let tail: f64 = (0..=k).map(|i| (ln_choose(n, i) - n as f64 * std::f64::consts::LN_2).exp()).sum();
    (2.0 * tail).min(1.0)
}

/// The 95% percentile bootstrap interval of mean(a) - mean(b), resampling
/// cases with a fixed seed.
pub fn bootstrap(a: &[bool], b: &[bool]) -> (f64, f64) {
    assert_eq!(a.len(), b.len());
    let n = a.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mut seed: u64 = 0x2026_1026;
    let mut diffs: Vec<f64> = (0..10_000)
        .map(|_| {
            let mut d = 0i64;
            for _ in 0..n {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let i = ((seed >> 33) % n as u64) as usize;
                d += i64::from(a[i]) - i64::from(b[i]);
            }
            d as f64 / n as f64
        })
        .collect();
    diffs.sort_by(f64::total_cmp);
    (diffs[249], diffs[9749])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcnemar_matches_the_binomial() {
        // 10 discordant pairs, 9 against 1: 2 * (1 + 10) / 1024.
        assert!((mcnemar(9, 1) - 22.0 / 1024.0).abs() < 1e-12);
        assert_eq!(mcnemar(5, 5), 1.0);
        assert_eq!(mcnemar(0, 0), 1.0);
        assert!(mcnemar(200, 0) < 1e-50);
    }

    #[test]
    fn the_interval_holds_the_difference() {
        let a: Vec<bool> = (0..100).map(|i| i < 80).collect();
        let b: Vec<bool> = (0..100).map(|i| i < 50).collect();
        let (lo, hi) = bootstrap(&a, &b);
        assert!(lo > 0.15 && hi < 0.45 && lo < 0.30 && hi > 0.30, "{lo} {hi}");
    }
}
