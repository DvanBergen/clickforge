use std::time::{Duration, Instant};

use clickforge_sdk::{GaussianRangeMs, KeyCode, MouseButton};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::models::InputSink;

/// Sleep polling granularity in milliseconds so kill-switch and focus-loss abort promptly.
pub const INTERRUPT_POLL_SLICE_MS: u64 = 2;

/// Upper safety bound for any single interruptible wait in milliseconds (10 minutes).
pub const MAX_WAIT_DURATION_MS: u64 = 600_000;

/// Maximum sample count processed when computing a UI Gaussian histogram preview.
pub const MAX_HISTOGRAM_SAMPLES: usize = 50_000;

/// Maximum bucket count allowed when computing a UI Gaussian histogram preview.
pub const MAX_HISTOGRAM_BUCKETS: usize = 64;

/// Hold duration in milliseconds applied per typed character in `TypeText` actions.
pub const TYPE_TEXT_KEY_HOLD_MS: u64 = 8;

/// Sleeps for `duration_ms` in short slices, returning `false` immediately if `should_abort` trips.
pub fn wait_interruptible_ms(duration_ms: u64, should_abort: &dyn Fn() -> bool) -> bool {
    if should_abort() {
        return false;
    }
    if duration_ms == 0 {
        return true;
    }
    let clamped_duration_ms = duration_ms.min(MAX_WAIT_DURATION_MS);
    let target_duration = Duration::from_millis(clamped_duration_ms);
    let started_at = Instant::now();

    loop {
        let elapsed_duration = started_at.elapsed();
        if elapsed_duration >= target_duration {
            return true;
        }
        let remaining_duration = target_duration.saturating_sub(elapsed_duration);
        let slice_duration = remaining_duration.min(Duration::from_millis(INTERRUPT_POLL_SLICE_MS));
        std::thread::sleep(slice_duration);
        if should_abort() {
            return false;
        }
    }
}

/// Samples a uniform integer delay in milliseconds inside `[min_delay_ms, max_delay_ms]`.
pub fn sample_uniform_delay_ms(min_delay_ms: u64, max_delay_ms: u64) -> u64 {
    if min_delay_ms >= max_delay_ms {
        return min_delay_ms;
    }
    rand::rng().random_range(min_delay_ms..=max_delay_ms)
}

/// Samples a symmetric jittered delay around `base_delay_ms` within `+/- jitter_ms`.
pub fn sample_jittered_delay_ms(base_delay_ms: u64, jitter_ms: u64) -> u64 {
    if jitter_ms == 0 {
        return base_delay_ms;
    }
    let lower_bound_ms = base_delay_ms.saturating_sub(jitter_ms);
    let upper_bound_ms = base_delay_ms.saturating_add(jitter_ms);
    sample_uniform_delay_ms(lower_bound_ms, upper_bound_ms)
}

/// Samples `base_delay_ms + clamped_gaussian_offset_ms` using `clickforge_sdk::GaussianRangeMs`.
pub fn sample_clamped_gaussian_delay_ms(
    base_delay_ms: u64,
    min_offset_ms: f64,
    max_offset_ms: f64,
    mean_offset_ms: f64,
    std_dev_ms: f64,
) -> Result<f64, String> {
    let offset_range_ms =
        GaussianRangeMs::new(min_offset_ms, max_offset_ms, mean_offset_ms, std_dev_ms)
            .map_err(|range_error| range_error.to_string())?;
    let sampled_offset_ms = offset_range_ms.sample_clamped_ms();
    let total_delay_ms = (base_delay_ms as f64) + sampled_offset_ms;
    if !total_delay_ms.is_finite() || total_delay_ms < 0.0 {
        return Err("sampled Gaussian delay produced a non-finite or negative value".to_string());
    }
    Ok(total_delay_ms)
}

/// Generates deterministic histogram bucket counts for UI visualization of a Gaussian timing block.
pub fn compute_gaussian_histogram_buckets(
    min_offset_ms: f64,
    max_offset_ms: f64,
    mean_offset_ms: f64,
    std_dev_ms: f64,
    sample_count: usize,
    bucket_count: usize,
) -> Vec<u32> {
    if bucket_count == 0 {
        return Vec::new();
    }
    let clamped_bucket_count = bucket_count.min(MAX_HISTOGRAM_BUCKETS);
    let mut bucket_counts = vec![0_u32; clamped_bucket_count];
    let clamped_sample_count = sample_count.min(MAX_HISTOGRAM_SAMPLES);
    if clamped_sample_count == 0 {
        return bucket_counts;
    }

    let Ok(offset_range_ms) =
        GaussianRangeMs::new(min_offset_ms, max_offset_ms, mean_offset_ms, std_dev_ms)
    else {
        return bucket_counts;
    };

    let span_ms = max_offset_ms - min_offset_ms;
    if span_ms <= f64::EPSILON {
        let center_bucket_index = clamped_bucket_count / 2;
        bucket_counts[center_bucket_index] =
            u32::try_from(clamped_sample_count).unwrap_or(u32::MAX);
        return bucket_counts;
    }

    let seed = derive_histogram_seed(min_offset_ms, max_offset_ms, mean_offset_ms, std_dev_ms);
    let mut seeded_rng = StdRng::seed_from_u64(seed);
    let max_bucket_index = clamped_bucket_count.saturating_sub(1);

    for _sample_index in 0..clamped_sample_count {
        let sampled_ms = offset_range_ms.sample_clamped_with_rng(&mut seeded_rng);
        let normalized_position = ((sampled_ms - min_offset_ms) / span_ms).clamp(0.0, 1.0);
        let scaled_index = (normalized_position * (clamped_bucket_count as f64)).floor() as usize;
        let bucket_index = scaled_index.min(max_bucket_index);
        bucket_counts[bucket_index] = bucket_counts[bucket_index].saturating_add(1);
    }

    bucket_counts
}

fn derive_histogram_seed(
    min_offset_ms: f64,
    max_offset_ms: f64,
    mean_offset_ms: f64,
    std_dev_ms: f64,
) -> u64 {
    const SEED_MIX_CONSTANT: u64 = 0x9E37_79B9_7F4A_7C15;
    min_offset_ms.to_bits()
        ^ max_offset_ms.to_bits().rotate_left(13)
        ^ mean_offset_ms.to_bits().rotate_left(29)
        ^ std_dev_ms.to_bits().rotate_left(47)
        ^ SEED_MIX_CONSTANT
}

/// Executes an interruptible mouse click, always releasing the button even when aborted mid-hold.
pub(crate) fn perform_mouse_click_on_sink(
    input_sink: &dyn InputSink,
    mouse_button: MouseButton,
    hold_duration_ms: u64,
    should_abort: &dyn Fn() -> bool,
) -> Result<bool, String> {
    if should_abort() {
        return Ok(false);
    }
    input_sink
        .mouse_down(mouse_button)
        .map_err(|input_error| input_error.to_string())?;
    let did_complete_hold = wait_interruptible_ms(hold_duration_ms, should_abort);
    let release_result = input_sink
        .mouse_up(mouse_button)
        .map_err(|input_error| input_error.to_string());
    release_result?;
    Ok(did_complete_hold)
}

/// Executes an interruptible keyboard tap, always releasing the key even when aborted mid-hold.
pub(crate) fn perform_key_tap_on_sink(
    input_sink: &dyn InputSink,
    key_code: KeyCode,
    hold_duration_ms: u64,
    should_abort: &dyn Fn() -> bool,
) -> Result<bool, String> {
    if should_abort() {
        return Ok(false);
    }
    input_sink
        .key_down(key_code)
        .map_err(|input_error| input_error.to_string())?;
    let did_complete_hold = wait_interruptible_ms(hold_duration_ms, should_abort);
    let release_result = input_sink
        .key_up(key_code)
        .map_err(|input_error| input_error.to_string());
    release_result?;
    Ok(did_complete_hold)
}

/// Types ASCII text with optional Shift handling and humanized per-keystroke jitter.
pub(crate) fn perform_type_text_on_sink(
    input_sink: &dyn InputSink,
    text: &str,
    keystroke_delay_ms: u64,
    jitter_ms: u64,
    should_abort: &dyn Fn() -> bool,
    on_delay_sampled_ms: &mut dyn FnMut(u64),
    on_action_executed: &mut dyn FnMut(),
) -> Result<bool, String> {
    for character in text.chars() {
        if should_abort() {
            return Ok(false);
        }
        let Some((key_code, requires_shift)) = KeyCode::ascii_char_to_key(character) else {
            continue;
        };
        let did_finish_key =
            emit_single_typed_character(input_sink, key_code, requires_shift, should_abort)?;
        on_action_executed();
        if !did_finish_key {
            return Ok(false);
        }
        let sampled_delay_ms = sample_jittered_delay_ms(keystroke_delay_ms, jitter_ms);
        on_delay_sampled_ms(sampled_delay_ms);
        if !wait_interruptible_ms(sampled_delay_ms, should_abort) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn emit_single_typed_character(
    input_sink: &dyn InputSink,
    key_code: KeyCode,
    requires_shift: bool,
    should_abort: &dyn Fn() -> bool,
) -> Result<bool, String> {
    if requires_shift {
        input_sink
            .key_down(KeyCode::LeftShift)
            .map_err(|input_error| input_error.to_string())?;
    }
    let key_down_result = input_sink
        .key_down(key_code)
        .map_err(|input_error| input_error.to_string());
    if let Err(down_error) = key_down_result {
        if requires_shift {
            let _ = input_sink.key_up(KeyCode::LeftShift);
        }
        return Err(down_error);
    }

    let did_complete_hold = wait_interruptible_ms(TYPE_TEXT_KEY_HOLD_MS, should_abort);
    let key_up_result = input_sink
        .key_up(key_code)
        .map_err(|input_error| input_error.to_string());
    let shift_up_result = if requires_shift {
        input_sink
            .key_up(KeyCode::LeftShift)
            .map_err(|input_error| input_error.to_string())
    } else {
        Ok(())
    };
    key_up_result?;
    shift_up_result?;
    Ok(did_complete_hold)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use super::*;

    #[test]
    fn clamped_gaussian_delay_respects_bounds_and_mean_over_ten_thousand_samples()
    -> Result<(), String> {
        let sample_count: usize = 10_000;
        let mut total_delay_ms = 0.0_f64;
        let mut first_sample_ms: Option<f64> = None;
        let mut has_variance = false;

        for _sample_index in 0..sample_count {
            let sampled_ms = sample_clamped_gaussian_delay_ms(45, 10.0, 20.0, 15.0, 2.5)?;
            assert!(
                (55.0..=65.0).contains(&sampled_ms),
                "sample {sampled_ms} fell outside [55.0, 65.0]"
            );
            let baseline_ms = *first_sample_ms.get_or_insert(sampled_ms);
            if (sampled_ms - baseline_ms).abs() > f64::EPSILON {
                has_variance = true;
            }
            total_delay_ms += sampled_ms;
        }

        let observed_mean_ms = total_delay_ms / (sample_count as f64);
        assert!(has_variance);
        assert!(
            (59.5..=60.5).contains(&observed_mean_ms),
            "observed mean {observed_mean_ms} drifted from expected 60.0 ms"
        );
        Ok(())
    }

    #[test]
    fn clamped_gaussian_rejects_invalid_range() {
        let invalid_result = sample_clamped_gaussian_delay_ms(45, 25.0, 10.0, 15.0, 2.5);
        assert!(invalid_result.is_err());
    }

    #[test]
    fn uniform_delay_stays_within_bounds() {
        for _sample_index in 0..500 {
            let sampled_ms = sample_uniform_delay_ms(12, 24);
            assert!((12..=24).contains(&sampled_ms));
        }
        assert_eq!(sample_uniform_delay_ms(30, 30), 30);
        assert_eq!(sample_uniform_delay_ms(40, 10), 40);
    }

    #[test]
    fn wait_interruptible_aborts_promptly_when_flag_trips() {
        let abort_flag = Arc::new(AtomicBool::new(false));
        let trigger_flag = Arc::clone(&abort_flag);
        let stopper_handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(12));
            trigger_flag.store(true, Ordering::SeqCst);
        });

        let started_at = Instant::now();
        let did_complete = wait_interruptible_ms(1_500, &|| abort_flag.load(Ordering::SeqCst));
        let elapsed_ms = started_at.elapsed().as_millis();
        let _ = stopper_handle.join();

        assert!(!did_complete);
        assert!(
            elapsed_ms < 250,
            "interruptible wait took too long to abort: {elapsed_ms} ms"
        );
    }

    #[test]
    fn histogram_buckets_sum_to_sample_count_and_peak_near_mean() {
        let bucket_counts = compute_gaussian_histogram_buckets(10.0, 20.0, 15.0, 2.0, 2_000, 10);
        assert_eq!(bucket_counts.len(), 10);
        let total_samples: u32 = bucket_counts.iter().copied().sum();
        assert_eq!(total_samples, 2_000);

        let middle_sum = bucket_counts[4].saturating_add(bucket_counts[5]);
        let edge_sum = bucket_counts[0].saturating_add(bucket_counts[9]);
        assert!(middle_sum > edge_sum);
    }
}
