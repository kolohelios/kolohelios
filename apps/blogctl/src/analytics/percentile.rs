#![forbid(unsafe_code)]

//! Tiny nearest-rank percentile helper — no external crate, no
//! interpolation. Nearest-rank picks the value at index
//! `ceil(p × n) - 1` of the sorted input; equivalent across most
//! small-n stats packages and accurate enough for the few-dozen-
//! post regime analytics commands operate on.

/// Three percentiles for one metric. The same shape ships to JSON
/// (matches the documented `{p25, p50, p75}` schema in #439).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Percentiles<T> {
    pub p25: T,
    pub p50: T,
    pub p75: T,
}

/// `Percentiles<u64>` over a slice. `None` when empty.
pub fn percentiles_u64(values: &[u64]) -> Option<Percentiles<u64>> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<u64> = values.to_vec();
    sorted.sort_unstable();
    Some(Percentiles {
        p25: nearest_rank(&sorted, 0.25),
        p50: nearest_rank(&sorted, 0.50),
        p75: nearest_rank(&sorted, 0.75),
    })
}

/// `Percentiles<f64>` over a slice. Assumes no NaN (callers
/// upstream filter `engagement_rate.is_some()` so the f64 stream is
/// well-defined). `None` when empty.
pub fn percentiles_f64(values: &[f64]) -> Option<Percentiles<f64>> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaNs in engagement_rate stream"));
    Some(Percentiles {
        p25: nearest_rank(&sorted, 0.25),
        p50: nearest_rank(&sorted, 0.50),
        p75: nearest_rank(&sorted, 0.75),
    })
}

fn nearest_rank<T: Copy>(sorted: &[T], p: f64) -> T {
    // Nearest-rank: index = ceil(p * n) - 1, clamped to [0, n-1].
    // For n=1 this collapses every percentile to the single value.
    let n = sorted.len();
    let raw = (p * n as f64).ceil() as isize - 1;
    let idx = raw.clamp(0, (n as isize) - 1) as usize;
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_none() {
        assert!(percentiles_u64(&[]).is_none());
        assert!(percentiles_f64(&[]).is_none());
    }

    #[test]
    fn single_value_collapses_to_that_value() {
        let p = percentiles_u64(&[42]).unwrap();
        assert_eq!(p.p25, 42);
        assert_eq!(p.p50, 42);
        assert_eq!(p.p75, 42);
    }

    #[test]
    fn four_values_quartiles() {
        // [10, 20, 30, 40]
        // p25: ceil(0.25*4) - 1 = 0  → 10
        // p50: ceil(0.5 *4) - 1 = 1  → 20
        // p75: ceil(0.75*4) - 1 = 2  → 30
        let p = percentiles_u64(&[10, 20, 30, 40]).unwrap();
        assert_eq!(p.p25, 10);
        assert_eq!(p.p50, 20);
        assert_eq!(p.p75, 30);
    }

    #[test]
    fn unsorted_input_is_handled() {
        // Same as above but scrambled.
        let p = percentiles_u64(&[40, 10, 30, 20]).unwrap();
        assert_eq!(p.p50, 20);
    }

    #[test]
    fn duplicates_are_preserved() {
        // Median of [5, 5, 5, 5, 100] should be 5, not 100.
        let p = percentiles_u64(&[5, 5, 5, 5, 100]).unwrap();
        assert_eq!(p.p50, 5);
    }

    #[test]
    fn f64_percentiles_handle_fractions() {
        let p = percentiles_f64(&[0.01, 0.02, 0.03, 0.04, 0.05]).unwrap();
        // p50 of 5 values: ceil(2.5)-1 = 2 → index 2 → 0.03.
        assert!((p.p50 - 0.03).abs() < 1e-12);
    }
}
