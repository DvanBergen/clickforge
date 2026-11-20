use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::sync::Mutex;

use clickforge_sdk::{KeyCode, MouseButton};
use evdev::uinput::VirtualDevice;
use evdev::{
    AttributeSet, EventSummary, InputEvent, KeyCode as EvdevKeyCode, KeyEvent, RelativeAxisCode,
    RelativeAxisEvent,
};
use tracing::warn;

use crate::linux::VIRTUAL_DEVICE_NAME;
use crate::linux::evdev_keymap::{
    all_virtual_device_keys, key_code_to_evdev, mouse_button_to_evdev,
};
use crate::models::runtime_contracts::{InputError, InputSink};

const UINPUT_DEVICE_PATH: &str = "/dev/uinput";
const KERNEL_RELEASE_FILE_PATH: &str = "/proc/sys/kernel/osrelease";
const KERNEL_MODULES_ROOT_PATH: &str = "/lib/modules";
const LINUX_ERRNO_ENODEV: i32 = 19;
const HI_RES_WHEEL_UNITS_PER_DETENT: i32 = 120;
const KEY_STATE_RELEASED: i32 = 0;
const KEY_STATE_PRESSED: i32 = 1;

struct UinputDeviceState {
    virtual_device: VirtualDevice,
    script_pressed_codes: HashSet<EvdevKeyCode>,
    forwarded_pressed_codes: HashSet<EvdevKeyCode>,
}

/// Persistent Linux `/dev/uinput` virtual mouse and keyboard emitter.
pub struct UinputInjector {
    device_state: Mutex<UinputDeviceState>,
}

impl UinputInjector {
    /// Creates and registers the persistent ClickForge virtual mouse and keyboard device.
    pub fn new() -> Result<Self, InputError> {
        let supported_keys = all_virtual_device_keys();
        let supported_axes = build_supported_relative_axes();

        let builder = VirtualDevice::builder().map_err(map_uinput_open_error)?;
        let virtual_device = builder
            .name(VIRTUAL_DEVICE_NAME)
            .with_keys(&supported_keys)
            .map_err(map_uinput_setup_error)?
            .with_relative_axes(&supported_axes)
            .map_err(map_uinput_setup_error)?
            .build()
            .map_err(map_uinput_setup_error)?;

        Ok(Self {
            device_state: Mutex::new(UinputDeviceState {
                virtual_device,
                script_pressed_codes: HashSet::new(),
                forwarded_pressed_codes: HashSet::new(),
            }),
        })
    }

    /// Forwards a batch of non-trigger physical pointer events atomically through `/dev/uinput`.
    pub fn forward_raw_events(&self, raw_events: &[InputEvent]) -> Result<(), InputError> {
        if raw_events.is_empty() {
            return Ok(());
        }
        let mut guard = self
            .device_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        guard
            .virtual_device
            .emit(raw_events)
            .map_err(|io_error| InputError::InjectionFailed {
                detail: format!("Failed forwarding grabbed pointer events: {io_error}"),
            })?;

        for input_event in raw_events {
            if let EventSummary::Key(_, evdev_code, event_value) = input_event.destructure() {
                if event_value == KEY_STATE_PRESSED {
                    guard.forwarded_pressed_codes.insert(evdev_code);
                } else if event_value == KEY_STATE_RELEASED {
                    guard.forwarded_pressed_codes.remove(&evdev_code);
                }
            }
        }
        Ok(())
    }

    /// Releases any non-trigger buttons that were held while pointer grab forwarding was active.
    pub fn release_forwarded_pressed(&self) -> Result<(), InputError> {
        let mut guard = self
            .device_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let codes_to_release: Vec<EvdevKeyCode> = guard.forwarded_pressed_codes.drain().collect();
        emit_release_batch(&mut guard.virtual_device, &codes_to_release)
    }

    fn emit_script_key_transition(
        &self,
        evdev_code: EvdevKeyCode,
        is_pressed: bool,
    ) -> Result<(), InputError> {
        let mut guard = self
            .device_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let event_value = if is_pressed {
            KEY_STATE_PRESSED
        } else {
            KEY_STATE_RELEASED
        };
        let key_event = *KeyEvent::new(evdev_code, event_value);
        guard
            .virtual_device
            .emit(&[key_event])
            .map_err(|io_error| InputError::InjectionFailed {
                detail: format!("Failed emitting key event {:?}: {io_error}", evdev_code),
            })?;

        if is_pressed {
            guard.script_pressed_codes.insert(evdev_code);
        } else {
            guard.script_pressed_codes.remove(&evdev_code);
        }
        Ok(())
    }
}

impl InputSink for UinputInjector {
    fn mouse_down(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        self.emit_script_key_transition(mouse_button_to_evdev(mouse_button), true)
    }

    fn mouse_up(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        self.emit_script_key_transition(mouse_button_to_evdev(mouse_button), false)
    }

    fn mouse_move_relative(&self, delta_x_px: i32, delta_y_px: i32) -> Result<(), InputError> {
        let mut motion_events = Vec::with_capacity(2);
        if delta_x_px != 0 {
            motion_events.push(*RelativeAxisEvent::new(RelativeAxisCode::REL_X, delta_x_px));
        }
        if delta_y_px != 0 {
            motion_events.push(*RelativeAxisEvent::new(RelativeAxisCode::REL_Y, delta_y_px));
        }
        if motion_events.is_empty() {
            return Ok(());
        }
        let mut guard = self
            .device_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .virtual_device
            .emit(&motion_events)
            .map_err(|io_error| InputError::InjectionFailed {
                detail: format!("Failed emitting relative mouse move: {io_error}"),
            })
    }

    fn mouse_scroll(&self, vertical_steps: i32) -> Result<(), InputError> {
        if vertical_steps == 0 {
            return Ok(());
        }
        let hi_res_units = vertical_steps.saturating_mul(HI_RES_WHEEL_UNITS_PER_DETENT);
        let scroll_events = [
            *RelativeAxisEvent::new(RelativeAxisCode::REL_WHEEL, vertical_steps),
            *RelativeAxisEvent::new(RelativeAxisCode::REL_WHEEL_HI_RES, hi_res_units),
        ];
        let mut guard = self
            .device_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .virtual_device
            .emit(&scroll_events)
            .map_err(|io_error| InputError::InjectionFailed {
                detail: format!("Failed emitting mouse scroll: {io_error}"),
            })
    }

    fn key_down(&self, key_code: KeyCode) -> Result<(), InputError> {
        self.emit_script_key_transition(key_code_to_evdev(key_code), true)
    }

    fn key_up(&self, key_code: KeyCode) -> Result<(), InputError> {
        self.emit_script_key_transition(key_code_to_evdev(key_code), false)
    }

    fn release_all_pressed(&self) -> Result<(), InputError> {
        let mut guard = self
            .device_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut combined_codes = std::mem::take(&mut guard.script_pressed_codes);
        combined_codes.extend(guard.forwarded_pressed_codes.drain());
        let release_list: Vec<EvdevKeyCode> = combined_codes.into_iter().collect();
        emit_release_batch(&mut guard.virtual_device, &release_list)
    }
}

impl Drop for UinputInjector {
    fn drop(&mut self) {
        if let Err(release_error) = self.release_all_pressed() {
            warn!("Failed releasing virtual keys during UinputInjector drop: {release_error}");
        }
    }
}

/// Graceful fallback sink returned when `/dev/uinput` cannot be opened on the host system.
#[derive(Debug, Clone)]
pub struct NoopInputSink {
    unavailable_reason: String,
}

impl NoopInputSink {
    /// Constructs a fallback input sink capturing the exact `/dev/uinput` initialization error.
    pub fn new(unavailable_reason: impl Into<String>) -> Self {
        Self {
            unavailable_reason: unavailable_reason.into(),
        }
    }

    fn unavailable_error(&self) -> InputError {
        InputError::DeviceUnavailable {
            detail: self.unavailable_reason.clone(),
        }
    }
}

impl InputSink for NoopInputSink {
    fn mouse_down(&self, _mouse_button: MouseButton) -> Result<(), InputError> {
        Err(self.unavailable_error())
    }

    fn mouse_up(&self, _mouse_button: MouseButton) -> Result<(), InputError> {
        Err(self.unavailable_error())
    }

    fn mouse_move_relative(&self, _delta_x_px: i32, _delta_y_px: i32) -> Result<(), InputError> {
        Err(self.unavailable_error())
    }

    fn mouse_scroll(&self, _vertical_steps: i32) -> Result<(), InputError> {
        Err(self.unavailable_error())
    }

    fn key_down(&self, _key_code: KeyCode) -> Result<(), InputError> {
        Err(self.unavailable_error())
    }

    fn key_up(&self, _key_code: KeyCode) -> Result<(), InputError> {
        Err(self.unavailable_error())
    }

    fn release_all_pressed(&self) -> Result<(), InputError> {
        Ok(())
    }
}

fn build_supported_relative_axes() -> AttributeSet<RelativeAxisCode> {
    let mut supported_axes = AttributeSet::<RelativeAxisCode>::new();
    supported_axes.insert(RelativeAxisCode::REL_X);
    supported_axes.insert(RelativeAxisCode::REL_Y);
    supported_axes.insert(RelativeAxisCode::REL_WHEEL);
    supported_axes.insert(RelativeAxisCode::REL_HWHEEL);
    supported_axes.insert(RelativeAxisCode::REL_WHEEL_HI_RES);
    supported_axes.insert(RelativeAxisCode::REL_HWHEEL_HI_RES);
    supported_axes
}

fn emit_release_batch(
    virtual_device: &mut VirtualDevice,
    codes_to_release: &[EvdevKeyCode],
) -> Result<(), InputError> {
    if codes_to_release.is_empty() {
        return Ok(());
    }
    let release_events: Vec<InputEvent> = codes_to_release
        .iter()
        .map(|evdev_code| *KeyEvent::new(*evdev_code, KEY_STATE_RELEASED))
        .collect();
    virtual_device
        .emit(&release_events)
        .map_err(|io_error| InputError::InjectionFailed {
            detail: format!("Failed releasing pressed virtual inputs: {io_error}"),
        })
}

fn map_uinput_open_error(io_error: io::Error) -> InputError {
    map_uinput_open_error_with_paths(
        io_error,
        Path::new(KERNEL_RELEASE_FILE_PATH),
        Path::new(KERNEL_MODULES_ROOT_PATH),
    )
}

fn map_uinput_open_error_with_paths(
    io_error: io::Error,
    kernel_release_file_path: &Path,
    modules_root_path: &Path,
) -> InputError {
    if io_error.kind() == io::ErrorKind::PermissionDenied {
        return InputError::PermissionDenied {
            path: UINPUT_DEVICE_PATH.to_string(),
            detail: format!(
                "{io_error}. Run scripts/install_udev_rules.sh and ensure your user belongs to the 'input' group."
            ),
        };
    }
    if io_error.raw_os_error() == Some(LINUX_ERRNO_ENODEV) {
        return InputError::DeviceUnavailable {
            detail: format_uinput_enodev_detail(kernel_release_file_path, modules_root_path),
        };
    }
    InputError::DeviceUnavailable {
        detail: format!("Could not open {UINPUT_DEVICE_PATH}: {io_error}"),
    }
}

fn format_uinput_enodev_detail(
    kernel_release_file_path: &Path,
    modules_root_path: &Path,
) -> String {
    if let Some(kernel_release) = read_trimmed_kernel_release(kernel_release_file_path)
        && !modules_root_path.join(&kernel_release).exists()
    {
        return format!(
            "Kernel module directory {KERNEL_MODULES_ROOT_PATH}/{kernel_release} is missing (likely after a kernel update). Reboot once to load the updated kernel and uinput module."
        );
    }
    format!(
        "{UINPUT_DEVICE_PATH} kernel module is not loaded (No such device). Run 'sudo modprobe uinput' or 'sudo scripts/install_udev_rules.sh'."
    )
}

fn read_trimmed_kernel_release(kernel_release_file_path: &Path) -> Option<String> {
    let raw_release = std::fs::read_to_string(kernel_release_file_path).ok()?;
    let trimmed_release = raw_release.trim();
    if trimmed_release.is_empty() {
        return None;
    }
    Some(trimmed_release.to_string())
}

fn map_uinput_setup_error(io_error: io::Error) -> InputError {
    InputError::DeviceUnavailable {
        detail: format!("Could not configure {UINPUT_DEVICE_PATH} virtual device: {io_error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_input_sink_reports_device_unavailable_and_allows_release_all() {
        let sink = NoopInputSink::new("uinput permission denied");
        assert!(sink.mouse_down(MouseButton::Left).is_err());
        assert!(sink.key_down(KeyCode::A).is_err());
        assert!(sink.mouse_move_relative(5, -5).is_err());
        assert!(sink.mouse_scroll(1).is_err());
        assert!(sink.release_all_pressed().is_ok());
    }

    #[test]
    fn supported_relative_axes_include_pointer_and_scroll_axes() {
        let axes = build_supported_relative_axes();
        assert!(axes.contains(RelativeAxisCode::REL_X));
        assert!(axes.contains(RelativeAxisCode::REL_Y));
        assert!(axes.contains(RelativeAxisCode::REL_WHEEL));
        assert!(axes.contains(RelativeAxisCode::REL_HWHEEL));
    }

    #[test]
    fn map_uinput_open_error_reports_reboot_guidance_when_kernel_modules_directory_is_missing()
    -> Result<(), String> {
        let test_root = std::env::temp_dir().join(format!(
            "clickforge-uinput-missing-modules-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&test_root);
        let modules_root = test_root.join("modules");
        std::fs::create_dir_all(&modules_root).map_err(|io_error| io_error.to_string())?;
        let osrelease_file = test_root.join("osrelease");
        std::fs::write(&osrelease_file, "6.19.1-test-kernel\n")
            .map_err(|io_error| io_error.to_string())?;

        let mapped_error = map_uinput_open_error_with_paths(
            io::Error::from_raw_os_error(LINUX_ERRNO_ENODEV),
            &osrelease_file,
            &modules_root,
        );
        let _ = std::fs::remove_dir_all(&test_root);

        match mapped_error {
            InputError::DeviceUnavailable { detail } => {
                assert_eq!(
                    detail,
                    "Kernel module directory /lib/modules/6.19.1-test-kernel is missing (likely after a kernel update). Reboot once to load the updated kernel and uinput module."
                );
                Ok(())
            }
            other_error => Err(format!("expected DeviceUnavailable, got {other_error:?}")),
        }
    }

    #[test]
    fn map_uinput_open_error_reports_modprobe_guidance_when_kernel_modules_directory_exists()
    -> Result<(), String> {
        let test_root = std::env::temp_dir().join(format!(
            "clickforge-uinput-present-modules-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&test_root);
        let modules_root = test_root.join("modules");
        let release_directory = modules_root.join("6.19.1-test-kernel");
        std::fs::create_dir_all(&release_directory).map_err(|io_error| io_error.to_string())?;
        let osrelease_file = test_root.join("osrelease");
        std::fs::write(&osrelease_file, "6.19.1-test-kernel\n")
            .map_err(|io_error| io_error.to_string())?;

        let mapped_error = map_uinput_open_error_with_paths(
            io::Error::from_raw_os_error(LINUX_ERRNO_ENODEV),
            &osrelease_file,
            &modules_root,
        );
        let _ = std::fs::remove_dir_all(&test_root);

        match mapped_error {
            InputError::DeviceUnavailable { detail } => {
                assert_eq!(
                    detail,
                    "/dev/uinput kernel module is not loaded (No such device). Run 'sudo modprobe uinput' or 'sudo scripts/install_udev_rules.sh'."
                );
                Ok(())
            }
            other_error => Err(format!("expected DeviceUnavailable, got {other_error:?}")),
        }
    }

    #[test]
    fn map_uinput_open_error_falls_back_to_modprobe_guidance_when_osrelease_is_unreadable()
    -> Result<(), String> {
        let missing_osrelease = std::env::temp_dir().join(format!(
            "clickforge-uinput-no-osrelease-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&missing_osrelease);
        let mapped_error = map_uinput_open_error_with_paths(
            io::Error::from_raw_os_error(LINUX_ERRNO_ENODEV),
            &missing_osrelease,
            Path::new(KERNEL_MODULES_ROOT_PATH),
        );

        match mapped_error {
            InputError::DeviceUnavailable { detail } => {
                assert_eq!(
                    detail,
                    "/dev/uinput kernel module is not loaded (No such device). Run 'sudo modprobe uinput' or 'sudo scripts/install_udev_rules.sh'."
                );
                Ok(())
            }
            other_error => Err(format!("expected DeviceUnavailable, got {other_error:?}")),
        }
    }

    #[test]
    fn map_uinput_open_error_preserves_permission_denied_guidance() -> Result<(), String> {
        let mapped_error = map_uinput_open_error_with_paths(
            io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
            Path::new(KERNEL_RELEASE_FILE_PATH),
            Path::new(KERNEL_MODULES_ROOT_PATH),
        );

        match mapped_error {
            InputError::PermissionDenied { path, detail } => {
                assert_eq!(path, UINPUT_DEVICE_PATH);
                assert!(detail.contains("scripts/install_udev_rules.sh"));
                Ok(())
            }
            other_error => Err(format!("expected PermissionDenied, got {other_error:?}")),
        }
    }
}
