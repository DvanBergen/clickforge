use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use evdev::{Device, EventSummary, InputEvent, KeyCode as EvdevKeyCode, RelativeAxisCode};
use tracing::{debug, error, info, warn};

use crate::linux::VIRTUAL_DEVICE_NAME;
use crate::linux::evdev_keymap::{evdev_to_trigger, key_code_to_evdev, trigger_to_evdev};
use crate::linux::trigger_interceptor::{
    GrabbedEventDecision, TriggerInterceptor, classify_grabbed_pointer_event,
};
use crate::linux::uinput_injector::UinputInjector;
use crate::models::runtime_contracts::{PhysicalInputReader, TriggerInterceptSpec};

const INPUT_DEVICES_DIRECTORY: &str = "/dev/input";
const INPUT_EVENT_CHANNEL_CAPACITY: usize = 256;
const MAX_MONITORED_INPUT_DEVICES: usize = 32;
const PASSIVE_IDLE_POLL_MS: u64 = 4;
const GRAB_IDLE_POLL_MICROS: u64 = 500;
const DEVICE_RESCAN_INTERVAL_MS: u64 = 3_000;
const MOUSE_BUTTON_SLOT_COUNT: usize = 5;
const KEY_CODE_SLOT_COUNT: usize = 103;
const KEY_STATE_RELEASED: i32 = 0;
const KEY_STATE_PRESSED: i32 = 1;

/// Initial readiness report returned synchronously when starting the `evdev` listener.
#[derive(Debug, Clone)]
pub struct EvdevListenerStatus {
    pub is_evdev_ready: bool,
    pub monitored_device_count: usize,
    pub evdev_error: Option<String>,
}

/// Lock-free physical keyboard and mouse button state shared with the script engine.
pub struct SharedPhysicalInputState {
    mouse_states: [AtomicBool; MOUSE_BUTTON_SLOT_COUNT],
    key_states: Vec<AtomicBool>,
    last_key_sequence: AtomicU64,
    last_pressed_key: Mutex<Option<(KeyCode, u64)>>,
}

impl Default for SharedPhysicalInputState {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedPhysicalInputState {
    /// Initializes all physical mouse button and keyboard key states to released.
    pub fn new() -> Self {
        let mut key_states = Vec::with_capacity(KEY_CODE_SLOT_COUNT);
        for _slot_index in 0..KEY_CODE_SLOT_COUNT {
            key_states.push(AtomicBool::new(false));
        }
        Self {
            mouse_states: std::array::from_fn(|_button_index| AtomicBool::new(false)),
            key_states,
            last_key_sequence: AtomicU64::new(0),
            last_pressed_key: Mutex::new(None),
        }
    }

    /// Records a physical press or release transition from `evdev`.
    pub fn record_transition(&self, input_trigger: InputTrigger, is_pressed: bool) {
        match input_trigger {
            InputTrigger::Mouse(mouse_button) => {
                let button_slot = mouse_button_slot_index(mouse_button);
                if let Some(slot_atomic) = self.mouse_states.get(button_slot) {
                    slot_atomic.store(is_pressed, Ordering::Release);
                }
            }
            InputTrigger::Key(key_code) => {
                if let Some(key_slot) = key_code_slot_index(key_code)
                    && let Some(slot_atomic) = self.key_states.get(key_slot)
                {
                    slot_atomic.store(is_pressed, Ordering::Release);
                }
                if is_pressed {
                    let next_sequence = self
                        .last_key_sequence
                        .fetch_add(1, Ordering::AcqRel)
                        .saturating_add(1);
                    let mut last_key_guard = self
                        .last_pressed_key
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *last_key_guard = Some((key_code, next_sequence));
                }
            }
        }
    }
}

impl PhysicalInputReader for SharedPhysicalInputState {
    fn is_mouse_pressed(&self, mouse_button: MouseButton) -> bool {
        let button_slot = mouse_button_slot_index(mouse_button);
        self.mouse_states
            .get(button_slot)
            .is_some_and(|slot_atomic| slot_atomic.load(Ordering::Acquire))
    }

    fn is_key_pressed(&self, key_code: KeyCode) -> bool {
        let Some(key_slot) = key_code_slot_index(key_code) else {
            return false;
        };
        self.key_states
            .get(key_slot)
            .is_some_and(|slot_atomic| slot_atomic.load(Ordering::Acquire))
    }

    fn last_pressed_key_snapshot(&self) -> Option<(KeyCode, u64)> {
        let last_key_guard = self
            .last_pressed_key
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *last_key_guard
    }

    fn active_pressed_keys(&self) -> Vec<KeyCode> {
        let mut pressed_keys = Vec::new();
        for (slot_index, key_code) in KeyCode::all_keys().iter().enumerate() {
            let is_down = self
                .key_states
                .get(slot_index)
                .is_some_and(|slot_atomic| slot_atomic.load(Ordering::Acquire));
            if is_down {
                pressed_keys.push(*key_code);
            }
        }
        pressed_keys
    }
}

struct MonitoredInputDevice {
    device_path: PathBuf,
    device_name: String,
    device: Device,
    is_pointer: bool,
    is_grabbed: bool,
}

struct MonitoredDevicePool {
    devices: Vec<MonitoredInputDevice>,
    uinput_injector: Option<Arc<UinputInjector>>,
}

impl MonitoredDevicePool {
    fn release_all_grabs(&mut self) {
        let mut released_any = false;
        for monitored in &mut self.devices {
            if !monitored.is_grabbed {
                continue;
            }
            if let Err(ungrab_error) = monitored.device.ungrab() {
                warn!(
                    "Failed releasing EVIOCGRAB on '{}': {ungrab_error}",
                    monitored.device_name
                );
            }
            monitored.is_grabbed = false;
            released_any = true;
        }
        if released_any && let Some(injector) = &self.uinput_injector {
            let _ = injector.release_forwarded_pressed();
        }
    }

    fn has_any_grabbed_pointer(&self) -> bool {
        self.devices.iter().any(|monitored| monitored.is_grabbed)
    }

    fn is_trigger_physically_held_on_pointer(&self, trigger: InputTrigger) -> bool {
        let target_evdev_code = trigger_to_evdev(trigger);
        for monitored in &self.devices {
            if !monitored.is_pointer {
                continue;
            }
            if let Ok(key_state) = monitored.device.get_key_state()
                && key_state.contains(target_evdev_code)
            {
                return true;
            }
        }
        false
    }

    fn try_grab_all_pointers(&mut self) -> Result<usize, String> {
        let mut grabbed_count = 0usize;
        let mut grab_failure: Option<String> = None;

        for monitored in &mut self.devices {
            if !monitored.is_pointer {
                continue;
            }
            if monitored.is_grabbed {
                grabbed_count = grabbed_count.saturating_add(1);
                continue;
            }
            match monitored.device.grab() {
                Ok(()) => {
                    monitored.is_grabbed = true;
                    grabbed_count = grabbed_count.saturating_add(1);
                }
                Err(grab_error) => {
                    grab_failure = Some(format!(
                        "Failed acquiring EVIOCGRAB on '{}': {grab_error}",
                        monitored.device_name
                    ));
                    break;
                }
            }
        }

        if let Some(error_detail) = grab_failure {
            self.release_all_grabs();
            return Err(error_detail);
        }
        Ok(grabbed_count)
    }

    fn rescan_for_new_devices(&mut self) {
        if self.devices.len() >= MAX_MONITORED_INPUT_DEVICES {
            return;
        }
        let existing_paths: HashSet<PathBuf> = self
            .devices
            .iter()
            .map(|monitored| monitored.device_path.clone())
            .collect();
        let (discovered_devices, _denied_count) = discover_physical_input_devices(&existing_paths);
        for new_device in discovered_devices {
            if self.devices.len() >= MAX_MONITORED_INPUT_DEVICES {
                break;
            }
            info!(
                "Hotplugged physical input device: {} ({})",
                new_device.device_name,
                new_device.device_path.display()
            );
            self.devices.push(new_device);
        }
        self.devices.sort_by_key(|monitored| monitored.is_pointer);
    }
}

impl Drop for MonitoredDevicePool {
    fn drop(&mut self) {
        self.release_all_grabs();
    }
}

/// Scans physical `/dev/input/event*` devices and starts the non-blocking background input thread.
pub fn start_evdev_listener(
    shutdown_flag: Arc<AtomicBool>,
    shared_state: Arc<SharedPhysicalInputState>,
    trigger_interceptor: Arc<TriggerInterceptor>,
    uinput_injector: Option<Arc<UinputInjector>>,
) -> (EvdevListenerStatus, Receiver<(InputTrigger, bool)>) {
    let (event_sender, event_receiver) = mpsc::sync_channel(INPUT_EVENT_CHANNEL_CAPACITY);
    let empty_existing_paths = HashSet::new();
    let (mut initial_devices, permission_denied_count) =
        discover_physical_input_devices(&empty_existing_paths);
    initial_devices.sort_by_key(|monitored| monitored.is_pointer);

    let monitored_device_count = initial_devices.len();
    let is_evdev_ready = monitored_device_count > 0;
    let evdev_error = build_initial_evdev_error(monitored_device_count, permission_denied_count);

    let listener_status = EvdevListenerStatus {
        is_evdev_ready,
        monitored_device_count,
        evdev_error,
    };

    let device_pool = MonitoredDevicePool {
        devices: initial_devices,
        uinput_injector,
    };

    let spawn_result = thread::Builder::new()
        .name("clickforge-evdev-listener".to_string())
        .spawn(move || {
            run_evdev_event_loop(
                device_pool,
                shutdown_flag,
                shared_state,
                trigger_interceptor,
                event_sender,
            );
        });
    if let Err(spawn_error) = spawn_result {
        error!("Failed spawning evdev input listener thread: {spawn_error}");
    }

    (listener_status, event_receiver)
}

/// Returns true when a device name matches ClickForge's own virtual `uinput` device.
pub fn is_virtual_clickforge_device(device_name: &str) -> bool {
    let lower_name = device_name.to_ascii_lowercase();
    let lower_virtual_name = VIRTUAL_DEVICE_NAME.to_ascii_lowercase();
    lower_name.contains(&lower_virtual_name)
}

fn run_evdev_event_loop(
    mut device_pool: MonitoredDevicePool,
    shutdown_flag: Arc<AtomicBool>,
    shared_state: Arc<SharedPhysicalInputState>,
    trigger_interceptor: Arc<TriggerInterceptor>,
    event_sender: SyncSender<(InputTrigger, bool)>,
) {
    let mut active_intercept_spec: Option<TriggerInterceptSpec> = None;
    let mut deferred_intercept_spec: Option<(TriggerInterceptSpec, u64)> = None;
    let mut is_suppressed_trigger_held = false;
    let mut last_applied_generation: u64 = 0;
    let rescan_interval = Duration::from_millis(DEVICE_RESCAN_INTERVAL_MS);
    let mut last_rescan = Instant::now();

    while !shutdown_flag.load(Ordering::Acquire) {
        let current_generation = trigger_interceptor.desired_generation();
        if current_generation != last_applied_generation {
            last_applied_generation = current_generation;
            reconcile_intercept_request(
                &mut device_pool,
                &trigger_interceptor,
                &mut active_intercept_spec,
                &mut deferred_intercept_spec,
                &mut is_suppressed_trigger_held,
            );
        } else if let Some((pending_spec, pending_generation)) = deferred_intercept_spec
            && !device_pool.is_trigger_physically_held_on_pointer(pending_spec.trigger_input)
        {
            deferred_intercept_spec = None;
            engage_pointer_grab(
                &mut device_pool,
                &trigger_interceptor,
                &mut active_intercept_spec,
                &mut is_suppressed_trigger_held,
                pending_spec,
                pending_generation,
            );
        }

        if !device_pool.has_any_grabbed_pointer()
            && deferred_intercept_spec.is_none()
            && last_rescan.elapsed() >= rescan_interval
        {
            device_pool.rescan_for_new_devices();
            last_rescan = Instant::now();
        }

        let processed_any_event = poll_all_devices(
            &mut device_pool,
            &shared_state,
            &trigger_interceptor,
            &mut active_intercept_spec,
            &mut is_suppressed_trigger_held,
            &event_sender,
        );

        if processed_any_event {
            continue;
        }
        if device_pool.has_any_grabbed_pointer() || deferred_intercept_spec.is_some() {
            thread::sleep(Duration::from_micros(GRAB_IDLE_POLL_MICROS));
        } else {
            thread::sleep(Duration::from_millis(PASSIVE_IDLE_POLL_MS));
        }
    }

    device_pool.release_all_grabs();
    trigger_interceptor.report_applied_state(None, last_applied_generation, None);
}

fn reconcile_intercept_request(
    device_pool: &mut MonitoredDevicePool,
    trigger_interceptor: &TriggerInterceptor,
    active_intercept_spec: &mut Option<TriggerInterceptSpec>,
    deferred_intercept_spec: &mut Option<(TriggerInterceptSpec, u64)>,
    is_suppressed_trigger_held: &mut bool,
) {
    let (desired_spec, generation) = trigger_interceptor.desired_intercept_snapshot();
    *deferred_intercept_spec = None;
    if desired_spec != *active_intercept_spec {
        *is_suppressed_trigger_held = false;
    }

    let Some(requested_spec) = desired_spec else {
        device_pool.release_all_grabs();
        *active_intercept_spec = None;
        *is_suppressed_trigger_held = false;
        trigger_interceptor.report_applied_state(None, generation, None);
        return;
    };

    if device_pool.uinput_injector.is_none() {
        device_pool.release_all_grabs();
        *active_intercept_spec = None;
        *is_suppressed_trigger_held = false;
        trigger_interceptor.report_applied_state(
            None,
            generation,
            Some(
                "Cannot grab physical pointer without an active /dev/uinput injector.".to_string(),
            ),
        );
        return;
    }

    // Defer grab until the trigger button is physically released so focus-click releases reach the compositor.
    if !device_pool.has_any_grabbed_pointer()
        && device_pool.is_trigger_physically_held_on_pointer(requested_spec.trigger_input)
    {
        *deferred_intercept_spec = Some((requested_spec, generation));
        *is_suppressed_trigger_held = false;
        trigger_interceptor.report_applied_state(None, generation, None);
        return;
    }

    engage_pointer_grab(
        device_pool,
        trigger_interceptor,
        active_intercept_spec,
        is_suppressed_trigger_held,
        requested_spec,
        generation,
    );
}

fn engage_pointer_grab(
    device_pool: &mut MonitoredDevicePool,
    trigger_interceptor: &TriggerInterceptor,
    active_intercept_spec: &mut Option<TriggerInterceptSpec>,
    is_suppressed_trigger_held: &mut bool,
    requested_spec: TriggerInterceptSpec,
    generation: u64,
) {
    let should_reset_suppressed_hold =
        !device_pool.has_any_grabbed_pointer() || *active_intercept_spec != Some(requested_spec);
    match device_pool.try_grab_all_pointers() {
        Ok(grabbed_count) if grabbed_count > 0 => {
            if should_reset_suppressed_hold {
                *is_suppressed_trigger_held = false;
            }
            *active_intercept_spec = Some(requested_spec);
            trigger_interceptor.report_applied_state(Some(requested_spec), generation, None);
        }
        Ok(_) => {
            *active_intercept_spec = None;
            *is_suppressed_trigger_held = false;
            trigger_interceptor.report_applied_state(
                None,
                generation,
                Some("No physical pointer device found in /dev/input to intercept.".to_string()),
            );
        }
        Err(grab_error) => {
            *active_intercept_spec = None;
            *is_suppressed_trigger_held = false;
            trigger_interceptor.report_applied_state(None, generation, Some(grab_error));
        }
    }
}

fn poll_all_devices(
    device_pool: &mut MonitoredDevicePool,
    shared_state: &SharedPhysicalInputState,
    trigger_interceptor: &TriggerInterceptor,
    active_intercept_spec: &mut Option<TriggerInterceptSpec>,
    is_suppressed_trigger_held: &mut bool,
    event_sender: &SyncSender<(InputTrigger, bool)>,
) -> bool {
    let mut processed_any_event = false;
    let mut disconnected_indices = Vec::new();
    let mut forward_error_message: Option<String> = None;

    for (device_index, monitored) in device_pool.devices.iter_mut().enumerate() {
        let fetched_events = match drain_nonblocking_events(&mut monitored.device) {
            Ok(events) => events,
            Err(read_error) => {
                debug!(
                    "Disconnecting unreadable input device '{}': {read_error}",
                    monitored.device_name
                );
                disconnected_indices.push(device_index);
                continue;
            }
        };
        if fetched_events.is_empty() {
            continue;
        }
        processed_any_event = true;

        if monitored.is_grabbed
            && let Some(active_spec) = *active_intercept_spec
        {
            if let Err(forward_error) = process_grabbed_batch(
                &fetched_events,
                active_spec,
                device_pool.uinput_injector.as_deref(),
                shared_state,
                is_suppressed_trigger_held,
                event_sender,
            ) {
                forward_error_message = Some(forward_error);
                break;
            }
        } else {
            process_passive_batch(&fetched_events, shared_state, event_sender);
        }
    }

    if let Some(error_detail) = forward_error_message {
        error!("Emergency ungrab after forwarding failure: {error_detail}");
        device_pool.release_all_grabs();
        *active_intercept_spec = None;
        *is_suppressed_trigger_held = false;
        trigger_interceptor.force_deactivate_on_error(error_detail);
    }

    for disconnected_index in disconnected_indices.into_iter().rev() {
        let mut removed = device_pool.devices.remove(disconnected_index);
        if removed.is_grabbed {
            let _ = removed.device.ungrab();
            *is_suppressed_trigger_held = false;
        }
    }

    processed_any_event
}

fn drain_nonblocking_events(device: &mut Device) -> io::Result<Vec<InputEvent>> {
    match device.fetch_events() {
        Ok(event_iterator) => Ok(event_iterator.collect()),
        Err(io_error) if io_error.kind() == io::ErrorKind::WouldBlock => Ok(Vec::new()),
        Err(io_error) => Err(io_error),
    }
}

fn process_passive_batch(
    events: &[InputEvent],
    shared_state: &SharedPhysicalInputState,
    event_sender: &SyncSender<(InputTrigger, bool)>,
) {
    for input_event in events {
        let EventSummary::Key(_, evdev_code, event_value) = input_event.destructure() else {
            continue;
        };
        let is_pressed = match event_value {
            KEY_STATE_PRESSED => true,
            KEY_STATE_RELEASED => false,
            _ => continue,
        };
        let Some(input_trigger) = evdev_to_trigger(evdev_code) else {
            continue;
        };
        shared_state.record_transition(input_trigger, is_pressed);
        let _ = event_sender.try_send((input_trigger, is_pressed));
    }
}

fn process_grabbed_batch(
    events: &[InputEvent],
    intercept_spec: TriggerInterceptSpec,
    uinput_injector: Option<&UinputInjector>,
    shared_state: &SharedPhysicalInputState,
    is_suppressed_trigger_held: &mut bool,
    event_sender: &SyncSender<(InputTrigger, bool)>,
) -> Result<(), String> {
    let Some(injector) = uinput_injector else {
        return Err("Missing virtual uinput injector while pointer grab was active.".to_string());
    };
    let mut forwarded_frame = Vec::with_capacity(16);

    for input_event in events {
        match classify_grabbed_pointer_event(
            *input_event,
            intercept_spec,
            shared_state,
            is_suppressed_trigger_held,
        ) {
            GrabbedEventDecision::SuppressTrigger {
                trigger,
                is_pressed,
            } => {
                shared_state.record_transition(trigger, is_pressed);
                let _ = event_sender.try_send((trigger, is_pressed));
            }
            GrabbedEventDecision::ForwardEvent { mapped_transition } => {
                if let Some((trigger, is_pressed)) = mapped_transition {
                    shared_state.record_transition(trigger, is_pressed);
                    let _ = event_sender.try_send((trigger, is_pressed));
                }
                forwarded_frame.push(*input_event);
            }
            GrabbedEventDecision::FlushForwardedFrame => {
                if !forwarded_frame.is_empty() {
                    injector
                        .forward_raw_events(&forwarded_frame)
                        .map_err(|input_error| input_error.to_string())?;
                    forwarded_frame.clear();
                }
            }
            GrabbedEventDecision::Ignore => {}
        }
    }

    if !forwarded_frame.is_empty() {
        injector
            .forward_raw_events(&forwarded_frame)
            .map_err(|input_error| input_error.to_string())?;
    }
    Ok(())
}

fn discover_physical_input_devices(
    existing_paths: &HashSet<PathBuf>,
) -> (Vec<MonitoredInputDevice>, usize) {
    let Ok(directory_entries) = fs::read_dir(Path::new(INPUT_DEVICES_DIRECTORY)) else {
        return (Vec::new(), 0);
    };

    let mut candidate_paths: Vec<PathBuf> = directory_entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name_text| name_text.starts_with("event"))
        })
        .collect();
    candidate_paths.sort();

    let mut opened_devices = Vec::new();
    let mut permission_denied_count = 0usize;

    for device_path in candidate_paths {
        if opened_devices.len() >= MAX_MONITORED_INPUT_DEVICES {
            break;
        }
        if existing_paths.contains(&device_path) {
            continue;
        }
        match Device::open(&device_path) {
            Ok(device) => {
                if let Some(monitored) = classify_and_prepare_device(device_path, device) {
                    opened_devices.push(monitored);
                }
            }
            Err(io_error) if io_error.kind() == io::ErrorKind::PermissionDenied => {
                permission_denied_count = permission_denied_count.saturating_add(1);
            }
            Err(_) => {}
        }
    }

    (opened_devices, permission_denied_count)
}

fn classify_and_prepare_device(
    device_path: PathBuf,
    device: Device,
) -> Option<MonitoredInputDevice> {
    let device_name = device.name().unwrap_or("Unknown Input Device").to_string();
    if is_virtual_clickforge_device(&device_name) {
        debug!(
            "Skipping virtual ClickForge device '{}' at {} to prevent feedback loops",
            device_name,
            device_path.display()
        );
        return None;
    }

    let is_pointer = device_is_physical_pointer(&device);
    let is_keyboard = device_is_physical_keyboard(&device);
    if !is_pointer && !is_keyboard {
        return None;
    }

    if let Err(nonblock_error) = device.set_nonblocking(true) {
        warn!(
            "Could not set O_NONBLOCK on '{}': {nonblock_error}",
            device_name
        );
        return None;
    }

    Some(MonitoredInputDevice {
        device_path,
        device_name,
        device,
        is_pointer,
        is_grabbed: false,
    })
}

fn device_is_physical_pointer(device: &Device) -> bool {
    let has_relative_xy = device.supported_relative_axes().is_some_and(|axes| {
        axes.contains(RelativeAxisCode::REL_X) && axes.contains(RelativeAxisCode::REL_Y)
    });
    let has_left_button = device
        .supported_keys()
        .is_some_and(|keys| keys.contains(EvdevKeyCode::BTN_LEFT));
    has_relative_xy && has_left_button
}

fn device_is_physical_keyboard(device: &Device) -> bool {
    let Some(supported_keys) = device.supported_keys() else {
        return false;
    };
    KeyCode::all_keys()
        .iter()
        .any(|key_code| supported_keys.contains(key_code_to_evdev(*key_code)))
}

fn build_initial_evdev_error(
    monitored_device_count: usize,
    permission_denied_count: usize,
) -> Option<String> {
    if monitored_device_count > 0 {
        return None;
    }
    if permission_denied_count > 0 {
        return Some(format!(
            "Permission denied reading {permission_denied_count} /dev/input/event* devices. Add your user to the 'input' group (see scripts/install_udev_rules.sh)."
        ));
    }
    Some("No physical keyboard or mouse devices were found under /dev/input/event*.".to_string())
}

fn mouse_button_slot_index(mouse_button: MouseButton) -> usize {
    match mouse_button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Side => 3,
        MouseButton::Extra => 4,
    }
}

fn key_code_slot_index(target_key_code: KeyCode) -> Option<usize> {
    KeyCode::all_keys()
        .iter()
        .position(|candidate| *candidate == target_key_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_virtual_clickforge_device_names_case_insensitively() {
        assert!(is_virtual_clickforge_device(VIRTUAL_DEVICE_NAME));
        assert!(is_virtual_clickforge_device("clickforge virtual input"));
        assert!(is_virtual_clickforge_device(
            "ClickForge Virtual Input Mouse"
        ));
        assert!(!is_virtual_clickforge_device("MX Ergo S Mouse"));
        assert!(!is_virtual_clickforge_device("Keychron K10 Pro"));
    }

    #[test]
    fn shared_physical_state_tracks_mouse_keyboard_and_last_key_sequence() {
        let state = SharedPhysicalInputState::new();
        assert!(!state.is_mouse_pressed(MouseButton::Left));
        assert!(!state.is_key_pressed(KeyCode::F8));
        assert_eq!(state.last_pressed_key_snapshot(), None);

        state.record_transition(InputTrigger::Mouse(MouseButton::Left), true);
        state.record_transition(InputTrigger::Key(KeyCode::F8), true);
        assert!(state.is_mouse_pressed(MouseButton::Left));
        assert!(state.is_key_pressed(KeyCode::F8));
        assert_eq!(state.last_pressed_key_snapshot(), Some((KeyCode::F8, 1)));
        assert_eq!(state.active_pressed_keys(), vec![KeyCode::F8]);

        state.record_transition(InputTrigger::Key(KeyCode::F8), false);
        assert!(!state.is_key_pressed(KeyCode::F8));
        assert!(state.active_pressed_keys().is_empty());
        assert_eq!(state.last_pressed_key_snapshot(), Some((KeyCode::F8, 1)));
    }
}
