//! Percentiles over one cell's samples.

use std::time::Duration;

/// Below this many samples a nearest-rank p99 is just the maximum.
pub const P99_MIN_SAMPLES: usize = 100;

/// Nearest-rank percentile: the sample at rank `ceil(p/100 * n)`.
pub fn percentile(samples: &[Duration], p: f64) -> Duration {
    assert!(!samples.is_empty(), "percentile of no samples");
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// The p99, only once there are enough samples for it to mean anything.
pub fn p99(samples: &[Duration]) -> Option<Duration> {
    (samples.len() >= P99_MIN_SAMPLES).then(|| percentile(samples, 99.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles_over_a_hundred_samples() {
        let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(percentile(&samples, 50.0), Duration::from_millis(50));
        assert_eq!(percentile(&samples, 99.0), Duration::from_millis(99));
        assert_eq!(percentile(&samples, 100.0), Duration::from_millis(100));
        assert_eq!(p99(&samples), Some(Duration::from_millis(99)));
    }

    #[test]
    fn a_single_sample_is_every_percentile_but_has_no_p99() {
        let samples = vec![Duration::from_millis(7)];
        assert_eq!(percentile(&samples, 50.0), Duration::from_millis(7));
        assert_eq!(p99(&samples), None);
    }

    #[test]
    fn unsorted_samples_are_sorted_first() {
        let samples: Vec<Duration> = [30, 10, 20].map(Duration::from_millis).to_vec();
        assert_eq!(percentile(&samples, 50.0), Duration::from_millis(20));
    }
}
