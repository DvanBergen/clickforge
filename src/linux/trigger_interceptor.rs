use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use clickforge_sdk::InputTrigger;
use evdev::{EventSummary, InputEvent, KeyCode as EvdevKeyCode, SynchronizationCode};

use crate::linux::evdev_keymap::{evdev_to_trigger, trigger_to_evdev};
use crate::models::runtime_contracts::{InputError, TriggerInterceptControl, TriggerInterceptSpec};

const GRAB_SYNC_ACK_TIMEOUT_MS: u64 = 120;
const KEY_STATE_RELEASED: i32 = 0;
const KEY_STATE_PRESSED: i32 = 1;

/// Decision produced when inspecting an `evdev` event from a grabbed physical pointer device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabbedEventDecision {
    /// Suppress forwarding to the virtual mouse; still report the physical trigger transition.
    SuppressTrigger {
        trigger: InputTrigger,
        is_pressed: bool,
    },
    /// Forward the event to `/dev/uinput`, optionally also reporting a mapped physical transition.
    ForwardEvent {
        mapped_transition: Option<(InputTrigger, bool)>,
    },
    /// Flush accumulated forwarded events immediately as one synchronized `SYN_REPORT` frame.
    FlushForwardedFrame,
    /// Ignore raw scan/repeat/non-forwarded events.
    Ignore,
}

#[derive(Debug, Default)]
struct InterceptSyncState {
    desired_spec: Option<TriggerInterceptSpec>,
    active_spec: Option<TriggerInterceptSpec>,
    acknowledged_generation: u64,
    last_grab_error: Option<String>,
}

/// Safe RAII controller for optional `EVIOCGRAB` suppression of physical mouse triggers.
pub struct TriggerInterceptor {
    is_uinput_ready: bool,
    shutdown_flag: Arc<AtomicBool>,
    is_active_atomic: AtomicBool,
    desired_generation: AtomicU64,
    sync_state: Mutex<InterceptSyncState>,
    sync_condvar: Condvar,
}

impl TriggerInterceptor {
    /// Creates a new interceptor controller tied to `uinput` readiness and global shutdown.
    pub fn new(is_uinput_ready: bool, shutdown_flag: Arc<AtomicBool>) -> Self {
        Self {
            is_uinput_ready,
            shutdown_flag,
            is_active_atomic: AtomicBool::new(false),
            desired_generation: AtomicU64::new(0),
            sync_state: Mutex::new(InterceptSyncState::default()),
            sync_condvar: Condvar::new(),
        }
    }

    /// Returns the currently active hardware interception spec, if any pointer grab is active.
    pub fn active_intercept(&self) -> Option<TriggerInterceptSpec> {
        let guard = self
            .sync_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.active_spec
    }

    /// Returns the desired interception spec requested by the coordinator (`None` on shutdown).
    pub(crate) fn desired_intercept_snapshot(&self) -> (Option<TriggerInterceptSpec>, u64) {
        if self.shutdown_flag.load(Ordering::Acquire) {
            return (None, self.desired_generation.load(Ordering::Acquire));
        }
        let guard = self
            .sync_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let generation = self.desired_generation.load(Ordering::Acquire);
        (guard.desired_spec, generation)
    }

    /// Returns the monotonic request generation counter so the listener polls lock-free.
    pub(crate) fn desired_generation(&self) -> u64 {
        self.desired_generation.load(Ordering::Acquire)
    }

    /// Records the actual grab outcome applied by the evdev listener and wakes waiting callers.
    pub(crate) fn report_applied_state(
        &self,
        applied_spec: Option<TriggerInterceptSpec>,
        applied_generation: u64,
        grab_error: Option<String>,
    ) {
        let mut guard = self
            .sync_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.active_spec = applied_spec;
        if applied_generation > guard.acknowledged_generation {
            guard.acknowledged_generation = applied_generation;
        }
        guard.last_grab_error = grab_error;
        self.is_active_atomic
            .store(applied_spec.is_some(), Ordering::Release);
        self.sync_condvar.notify_all();
    }

    /// Forces the desired spec back to `None` after a runtime forwarding or device error.
    pub(crate) fn force_deactivate_on_error(&self, error_detail: String) {
        let next_generation = self
            .desired_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let mut guard = self
            .sync_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.desired_spec = None;
        guard.active_spec = None;
        guard.acknowledged_generation = next_generation;
        guard.last_grab_error = Some(error_detail);
        self.is_active_atomic.store(false, Ordering::Release);
        self.sync_condvar.notify_all();
    }
}

impl TriggerInterceptControl for TriggerInterceptor {
    fn set_intercept(&self, spec: Option<TriggerInterceptSpec>) -> Result<(), InputError> {
        if let Some(requested_spec) = spec {
            if !self.is_uinput_ready {
                return Err(InputError::DeviceUnavailable {
                    detail: "Cannot enable trigger interception while /dev/uinput is unavailable; refusing to grab physical mouse.".to_string(),
                });
            }
            if self.shutdown_flag.load(Ordering::Acquire) {
                return Ok(());
            }
            // Only grab physical pointer devices for mouse button triggers; never grab keyboards.
            if matches!(requested_spec.trigger_input, InputTrigger::Key(_)) {
                return self.set_intercept(None);
            }
        }

        let target_generation = self
            .desired_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);

        let mut guard = self
            .sync_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.desired_spec = spec;
        guard.last_grab_error = None;
        if spec.is_none() {
            self.is_active_atomic.store(false, Ordering::Release);
        }

        let deadline = Instant::now() + Duration::from_millis(GRAB_SYNC_ACK_TIMEOUT_MS);
        while guard.acknowledged_generation < target_generation {
            let Some(remaining_duration) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let (updated_guard, wait_result) = self
                .sync_condvar
                .wait_timeout(guard, remaining_duration)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = updated_guard;
            if wait_result.timed_out() {
                break;
            }
        }

        if let Some(error_detail) = guard.last_grab_error.take() {
            return Err(InputError::DeviceUnavailable {
                detail: error_detail,
            });
        }
        Ok(())
    }

    fn is_intercept_active(&self) -> bool {
        self.is_active_atomic.load(Ordering::Acquire)
    }
}

impl Drop for TriggerInterceptor {
    fn drop(&mut self) {
        let _ = self.set_intercept(None);
    }
}

/// Classifies a single `evdev` event received from a grabbed physical pointer device.
pub fn classify_grabbed_pointer_event(
    input_event: InputEvent,
    suppressed_trigger: InputTrigger,
) -> GrabbedEventDecision {
    let suppressed_evdev_code: EvdevKeyCode = trigger_to_evdev(suppressed_trigger);

    match input_event.destructure() {
        EventSummary::Key(_, evdev_code, event_value) => {
            if evdev_code == suppressed_evdev_code {
                return classify_suppressed_key_value(suppressed_trigger, event_value);
            }
            let mapped_transition = map_key_transition_if_press_or_release(evdev_code, event_value);
            GrabbedEventDecision::ForwardEvent { mapped_transition }
        }
        EventSummary::RelativeAxis(_, _, _) => GrabbedEventDecision::ForwardEvent {
            mapped_transition: None,
        },
        EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
            GrabbedEventDecision::FlushForwardedFrame
        }
        _ => GrabbedEventDecision::Ignore,
    }
}

fn classify_suppressed_key_value(
    suppressed_trigger: InputTrigger,
    event_value: i32,
) -> GrabbedEventDecision {
    if event_value == KEY_STATE_PRESSED {
        return GrabbedEventDecision::SuppressTrigger {
            trigger: suppressed_trigger,
            is_pressed: true,
        };
    }
    if event_value == KEY_STATE_RELEASED {
        return GrabbedEventDecision::SuppressTrigger {
            trigger: suppressed_trigger,
            is_pressed: false,
        };
    }
    GrabbedEventDecision::Ignore
}

fn map_key_transition_if_press_or_release(
    evdev_code: EvdevKeyCode,
    event_value: i32,
) -> Option<(InputTrigger, bool)> {
    let is_pressed = match event_value {
        KEY_STATE_PRESSED => true,
        KEY_STATE_RELEASED => false,
        _ => return None,
    };
    evdev_to_trigger(evdev_code).map(|trigger| (trigger, is_pressed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickforge_sdk::{KeyCode, MouseButton};
    use evdev::{KeyEvent, RelativeAxisCode, RelativeAxisEvent, SynchronizationEvent};

    #[test]
    fn refuses_intercept_when_uinput_is_unavailable() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let interceptor = TriggerInterceptor::new(false, shutdown_flag);
        let result = interceptor.set_intercept(Some(TriggerInterceptSpec {
            trigger_input: InputTrigger::Mouse(MouseButton::Left),
        }));
        assert!(result.is_err());
        assert!(!interceptor.is_intercept_active());
    }

    #[test]
    fn keyboard_trigger_does_not_grab_pointer_devices() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let interceptor = TriggerInterceptor::new(true, shutdown_flag);
        interceptor.report_applied_state(None, 1, None);
        let result = interceptor.set_intercept(Some(TriggerInterceptSpec {
            trigger_input: InputTrigger::Key(KeyCode::F6),
        }));
        assert!(result.is_ok());
        assert!(!interceptor.is_intercept_active());
        assert_eq!(interceptor.active_intercept(), None);
    }

    #[test]
    fn grabbed_pointer_event_classifier_suppresses_trigger_and_forwards_motion_and_other_buttons() {
        let suppressed_trigger = InputTrigger::Mouse(MouseButton::Left);

        let left_down = *KeyEvent::new(EvdevKeyCode::BTN_LEFT, KEY_STATE_PRESSED);
        assert_eq!(
            classify_grabbed_pointer_event(left_down, suppressed_trigger),
            GrabbedEventDecision::SuppressTrigger {
                trigger: suppressed_trigger,
                is_pressed: true,
            }
        );

        let right_down = *KeyEvent::new(EvdevKeyCode::BTN_RIGHT, KEY_STATE_PRESSED);
        assert_eq!(
            classify_grabbed_pointer_event(right_down, suppressed_trigger),
            GrabbedEventDecision::ForwardEvent {
                mapped_transition: Some((InputTrigger::Mouse(MouseButton::Right), true)),
            }
        );

        let motion_x = *RelativeAxisEvent::new(RelativeAxisCode::REL_X, 14);
        assert_eq!(
            classify_grabbed_pointer_event(motion_x, suppressed_trigger),
            GrabbedEventDecision::ForwardEvent {
                mapped_transition: None,
            }
        );

        let wheel = *RelativeAxisEvent::new(RelativeAxisCode::REL_WHEEL, -1);
        assert_eq!(
            classify_grabbed_pointer_event(wheel, suppressed_trigger),
            GrabbedEventDecision::ForwardEvent {
                mapped_transition: None,
            }
        );

        let syn_report = *SynchronizationEvent::new(SynchronizationCode::SYN_REPORT, 0);
        assert_eq!(
            classify_grabbed_pointer_event(syn_report, suppressed_trigger),
            GrabbedEventDecision::FlushForwardedFrame
        );
    }
}
