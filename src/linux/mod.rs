pub mod evdev_keymap;
pub mod evdev_listener;
pub mod niri_ipc;
pub mod process_scanner;
pub mod trigger_interceptor;
pub mod uinput_injector;
pub mod window_watcher;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;

use clickforge_sdk::InputTrigger;
use tracing::{info, warn};

use crate::models::runtime_contracts::{
    InputSink, InputSubsystemStatus, PhysicalInputReader, TriggerInterceptControl,
};
use crate::models::target::ActiveWindowSnapshot;

pub use evdev_keymap::{
    all_virtual_device_keys, evdev_to_key_code, evdev_to_mouse_button, evdev_to_trigger,
    key_code_to_evdev, mouse_button_to_evdev, trigger_to_evdev,
};
pub use process_scanner::{MAX_PROCESS_SCAN_ENTRIES, scan_running_processes};
pub use window_watcher::WindowWatcher;

use evdev_listener::SharedPhysicalInputState;
use trigger_interceptor::TriggerInterceptor;
use uinput_injector::{NoopInputSink, UinputInjector};

/// Kernel device name assigned to ClickForge's persistent virtual `/dev/uinput` device.
pub const VIRTUAL_DEVICE_NAME: &str = "ClickForge Virtual Input";

/// Initialized Linux platform handles consumed by the background coordinator and UI workbench.
pub struct LinuxRuntimeHandles {
    pub input_sink: Arc<dyn InputSink>,
    pub physical_input: Arc<dyn PhysicalInputReader>,
    pub trigger_intercept: Arc<dyn TriggerInterceptControl>,
    pub input_status: InputSubsystemStatus,
    pub window_watcher: Arc<WindowWatcher>,
    pub input_event_receiver: Receiver<(InputTrigger, bool)>,
    pub focus_event_receiver: Receiver<ActiveWindowSnapshot>,
}

/// Boots the Linux `/dev/uinput` injector, physical `evdev` listener, RAII interceptor, and window watcher.
pub fn create_linux_runtime(shutdown_flag: Arc<AtomicBool>) -> LinuxRuntimeHandles {
    let (input_sink, uinput_for_forwarding, is_uinput_ready, uinput_error) =
        initialize_uinput_sink();

    let trigger_interceptor = Arc::new(TriggerInterceptor::new(
        is_uinput_ready,
        Arc::clone(&shutdown_flag),
    ));
    let shared_physical_state = Arc::new(SharedPhysicalInputState::new());

    let (listener_status, input_event_receiver) = evdev_listener::start_evdev_listener(
        Arc::clone(&shutdown_flag),
        Arc::clone(&shared_physical_state),
        Arc::clone(&trigger_interceptor),
        uinput_for_forwarding,
    );

    let (window_watcher, focus_event_receiver) = WindowWatcher::start(Arc::clone(&shutdown_flag));

    let input_status = InputSubsystemStatus {
        is_uinput_ready,
        uinput_error,
        is_evdev_ready: listener_status.is_evdev_ready,
        monitored_device_count: listener_status.monitored_device_count,
        evdev_error: listener_status.evdev_error,
        is_trigger_intercept_active: trigger_interceptor.is_intercept_active(),
        compositor_name: window_watcher.compositor_name().to_string(),
    };

    info!(
        "Linux runtime initialized: uinput_ready={}, evdev_devices={}, compositor={}",
        input_status.is_uinput_ready,
        input_status.monitored_device_count,
        input_status.compositor_name
    );

    LinuxRuntimeHandles {
        input_sink,
        physical_input: shared_physical_state,
        trigger_intercept: trigger_interceptor,
        input_status,
        window_watcher,
        input_event_receiver,
        focus_event_receiver,
    }
}

fn initialize_uinput_sink() -> (
    Arc<dyn InputSink>,
    Option<Arc<UinputInjector>>,
    bool,
    Option<String>,
) {
    match UinputInjector::new() {
        Ok(injector) => {
            let shared_injector = Arc::new(injector);
            let sink_trait: Arc<dyn InputSink> = shared_injector.clone();
            (sink_trait, Some(shared_injector), true, None)
        }
        Err(init_error) => {
            let error_message = init_error.to_string();
            warn!("Virtual uinput device unavailable: {error_message}");
            let fallback_sink: Arc<dyn InputSink> =
                Arc::new(NoopInputSink::new(error_message.clone()));
            (fallback_sink, None, false, Some(error_message))
        }
    }
}
