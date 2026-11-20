use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use clickforge::engine::timing::{
    compute_gaussian_histogram_buckets, sample_clamped_gaussian_delay_ms, sample_uniform_delay_ms,
    wait_interruptible_ms,
};
use clickforge_sdk::GaussianRangeMs;

const GAUSSIAN_SAMPLE_COUNT: usize = 10_000;
const BASE_DELAY_MS: u64 = 45;
const MIN_OFFSET_MS: f64 = 10.0;
const MAX_OFFSET_MS: f64 = 20.0;
const MEAN_OFFSET_MS: f64 = 15.0;
const STD_DEV_MS: f64 = 2.5;
const EXPECTED_MIN_TOTAL_MS: f64 = 55.0;
const EXPECTED_MAX_TOTAL_MS: f64 = 65.0;
const EXPECTED_MEAN_TOTAL_MS: f64 = 60.0;
const ALLOWED_MEAN_DRIFT_MS: f64 = 0.5;
const UNIFORM_SAMPLE_COUNT: usize = 2_000;

#[test]
fn ten_thousand_clamped_gaussian_samples_respect_bounds_mean_and_variance() -> Result<(), String> {
    let mut sum_delay_ms = 0.0_f64;
    let mut sum_squared_delay_ms = 0.0_f64;
    let mut observed_min_ms = f64::INFINITY;
    let mut observed_max_ms = f64::NEG_INFINITY;

    for _sample_index in 0..GAUSSIAN_SAMPLE_COUNT {
        let sampled_delay_ms = sample_clamped_gaussian_delay_ms(
            BASE_DELAY_MS,
            MIN_OFFSET_MS,
            MAX_OFFSET_MS,
            MEAN_OFFSET_MS,
            STD_DEV_MS,
        )?;
        assert!(
            (EXPECTED_MIN_TOTAL_MS..=EXPECTED_MAX_TOTAL_MS).contains(&sampled_delay_ms),
            "sampled Gaussian delay {sampled_delay_ms} ms fell outside [55.0, 65.0]"
        );
        observed_min_ms = observed_min_ms.min(sampled_delay_ms);
        observed_max_ms = observed_max_ms.max(sampled_delay_ms);
        sum_delay_ms += sampled_delay_ms;
        sum_squared_delay_ms += sampled_delay_ms * sampled_delay_ms;
    }

    let sample_count_f64 = GAUSSIAN_SAMPLE_COUNT as f64;
    let observed_mean_ms = sum_delay_ms / sample_count_f64;
    let observed_variance =
        (sum_squared_delay_ms / sample_count_f64) - (observed_mean_ms * observed_mean_ms);

    assert!(
        (observed_mean_ms - EXPECTED_MEAN_TOTAL_MS).abs() <= ALLOWED_MEAN_DRIFT_MS,
        "observed mean {observed_mean_ms:.3} ms drifted more than +/- {ALLOWED_MEAN_DRIFT_MS} ms from {EXPECTED_MEAN_TOTAL_MS} ms"
    );
    assert!(
        observed_variance > 0.5,
        "expected non-zero Gaussian variance, observed {observed_variance:.4}"
    );
    assert!(observed_max_ms > observed_min_ms);
    Ok(())
}

#[test]
fn gaussian_range_validation_rejects_invalid_parameters() -> Result<(), String> {
    assert!(GaussianRangeMs::new(20.0, 10.0, 15.0, 2.5).is_err());
    assert!(GaussianRangeMs::new(-1.0, 10.0, 5.0, 2.5).is_err());
    assert!(GaussianRangeMs::new(10.0, 20.0, 25.0, 2.5).is_err());
    assert!(GaussianRangeMs::new(10.0, 20.0, 15.0, -1.0).is_err());
    assert!(GaussianRangeMs::new(f64::NAN, 20.0, 15.0, 2.5).is_err());
    assert!(sample_clamped_gaussian_delay_ms(45, 25.0, 10.0, 15.0, 2.5).is_err());

    let deterministic_range = GaussianRangeMs::new(10.0, 20.0, 15.0, 0.0)
        .map_err(|range_error| range_error.to_string())?;
    assert!((deterministic_range.sample_clamped_ms() - 15.0).abs() <= f64::EPSILON);
    Ok(())
}

#[test]
fn uniform_delay_samples_remain_strictly_bounded() {
    let lower_bound_ms: u64 = 18;
    let upper_bound_ms: u64 = 42;
    let mut saw_distinct_values = false;
    let first_sample_ms = sample_uniform_delay_ms(lower_bound_ms, upper_bound_ms);

    for _sample_index in 0..UNIFORM_SAMPLE_COUNT {
        let sampled_ms = sample_uniform_delay_ms(lower_bound_ms, upper_bound_ms);
        assert!(
            (lower_bound_ms..=upper_bound_ms).contains(&sampled_ms),
            "uniform sample {sampled_ms} ms fell outside [{lower_bound_ms}, {upper_bound_ms}]"
        );
        if sampled_ms != first_sample_ms {
            saw_distinct_values = true;
        }
    }

    assert!(saw_distinct_values);
    assert_eq!(sample_uniform_delay_ms(25, 25), 25);
    assert_eq!(sample_uniform_delay_ms(50, 10), 50);
}

#[test]
fn interruptible_wait_aborts_early_and_handles_zero_and_pre_aborted_states() {
    assert!(wait_interruptible_ms(0, &|| false));
    assert!(!wait_interruptible_ms(500, &|| true));

    let abort_flag = Arc::new(AtomicBool::new(false));
    let trigger_flag = Arc::clone(&abort_flag);
    let stopper_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(15));
        trigger_flag.store(true, Ordering::SeqCst);
    });

    let started_at = Instant::now();
    let did_complete = wait_interruptible_ms(2_000, &|| abort_flag.load(Ordering::SeqCst));
    let elapsed_ms = started_at.elapsed().as_millis();
    let _ = stopper_thread.join();

    assert!(!did_complete);
    assert!(
        elapsed_ms < 250,
        "wait_interruptible_ms took {elapsed_ms} ms to abort"
    );
}

#[test]
fn gaussian_histogram_buckets_cover_all_samples_and_peak_near_center() {
    let bucket_counts = compute_gaussian_histogram_buckets(
        MIN_OFFSET_MS,
        MAX_OFFSET_MS,
        MEAN_OFFSET_MS,
        STD_DEV_MS,
        4_000,
        20,
    );
    assert_eq!(bucket_counts.len(), 20);
    let total_samples: u32 = bucket_counts.iter().copied().sum();
    assert_eq!(total_samples, 4_000);

    let center_mass = bucket_counts[9].saturating_add(bucket_counts[10]);
    let edge_mass = bucket_counts[0].saturating_add(bucket_counts[19]);
    assert!(center_mass > edge_mass);
}
