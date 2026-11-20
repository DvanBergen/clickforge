use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::{NativeOptions, egui};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use clickforge::engine::{CoordinatorStartConfig, start_coordinator};
use clickforge::linux::create_linux_runtime;
use clickforge::models::TrayUiEvent;
use clickforge::storage::{AppSettings, CustomBlockRepository, ScriptRepository, SettingsStore};
use clickforge::tray::spawn_system_tray;
use clickforge::ui::{ClickForgeWorkbenchApp, WorkbenchLaunchBundle, apply_forge_slate_theme};

const WINDOW_TITLE: &str = "ClickForge";
const WAYLAND_APP_ID: &str = "clickforge";
const MIN_WINDOW_WIDTH_PX: f32 = 960.0;
const MIN_WINDOW_HEIGHT_PX: f32 = 620.0;
const MAX_TRAY_EVENT_QUEUE_CAPACITY: usize = 32;
const SIGNAL_BRIDGE_POLL_INTERVAL_MS: u64 = 50;

const POSIX_SIGINT: i32 = 2;
const POSIX_SIGTERM: i32 = 15;

static SIGNAL_TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn signal(signal_number: i32, handler: extern "C" fn(i32)) -> usize;
}

extern "C" fn handle_termination_signal(_signal_number: i32) {
    SIGNAL_TERMINATION_REQUESTED.store(true, Ordering::SeqCst);
}

fn install_termination_signal_handlers() {
    // SAFETY: `handle_termination_signal` only performs an async-signal-safe atomic store.
    unsafe {
        signal(POSIX_SIGINT, handle_termination_signal);
        signal(POSIX_SIGTERM, handle_termination_signal);
    }
}

fn initialize_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("clickforge=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .try_init();
}

fn main() -> ExitCode {
    initialize_tracing();
    install_termination_signal_handlers();

    let repository = match ScriptRepository::new_default() {
        Ok(repository) => repository,
        Err(storage_error) => {
            error!("Failed to initialize ClickForge script repository: {storage_error}");
            return ExitCode::FAILURE;
        }
    };

    let settings_store = match SettingsStore::new_default() {
        Ok(store) => store,
        Err(storage_error) => {
            error!("Failed to initialize ClickForge settings store: {storage_error}");
            return ExitCode::FAILURE;
        }
    };

    let settings = match settings_store.load() {
        Ok(loaded_settings) => loaded_settings,
        Err(load_error) => {
            warn!("Failed to read settings.json, falling back to defaults: {load_error}");
            AppSettings::default()
        }
    };

    let initial_scripts = match repository.load_or_seed_default_scripts() {
        Ok(loaded_scripts) => loaded_scripts,
        Err(load_error) => {
            warn!("Failed to load saved scripts, starting with empty list: {load_error}");
            Vec::new()
        }
    };

    let custom_block_repository = match CustomBlockRepository::new_default() {
        Ok(custom_repository) => custom_repository,
        Err(storage_error) => {
            error!("Failed to initialize ClickForge custom block repository: {storage_error}");
            return ExitCode::FAILURE;
        }
    };

    let initial_custom_blocks = match custom_block_repository.load_all() {
        Ok(loaded_tiles) => loaded_tiles,
        Err(load_error) => {
            warn!("Failed to read custom_blocks.json, starting with empty library: {load_error}");
            Vec::new()
        }
    };

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shared_egui_context: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    spawn_signal_bridge_thread(Arc::clone(&shutdown_flag), Arc::clone(&shared_egui_context));

    let linux_runtime = create_linux_runtime(Arc::clone(&shutdown_flag));
    let (tray_event_sender, tray_event_receiver) =
        mpsc::sync_channel::<TrayUiEvent>(MAX_TRAY_EVENT_QUEUE_CAPACITY);
    let tray_handle = spawn_system_tray(tray_event_sender).map(Arc::new);

    let window_watcher_for_provider = Arc::clone(&linux_runtime.window_watcher);
    let window_snapshot_provider = Arc::new(move || {
        (
            window_watcher_for_provider.snapshot_focused_window(),
            window_watcher_for_provider.snapshot_open_windows(),
            window_watcher_for_provider.snapshot_running_processes(),
        )
    });

    let on_tray_state_changed = tray_handle.as_ref().map(|tray_arc| {
        let tray_clone = Arc::clone(tray_arc);
        Arc::new(move |tray_state| {
            tray_clone.update_state(tray_state);
        }) as Arc<dyn Fn(clickforge::models::TrayRuntimeState) + Send + Sync>
    });

    let egui_context_for_repaint = Arc::clone(&shared_egui_context);
    let on_ui_repaint_needed = Arc::new(move || {
        if let Ok(guard) = egui_context_for_repaint.lock()
            && let Some(egui_context) = guard.as_ref()
        {
            egui_context.request_repaint();
        }
    });

    let coordinator_config = CoordinatorStartConfig {
        initial_scripts: initial_scripts.clone(),
        initial_custom_blocks: initial_custom_blocks.clone(),
        input_sink: linux_runtime.input_sink,
        physical_input: linux_runtime.physical_input,
        trigger_intercept: linux_runtime.trigger_intercept,
        input_status: linux_runtime.input_status,
        window_snapshot_provider,
        input_event_receiver: Some(linux_runtime.input_event_receiver),
        focus_event_receiver: Some(linux_runtime.focus_event_receiver),
        on_tray_state_changed,
        on_ui_repaint_needed: Some(on_ui_repaint_needed),
        shutdown_flag: Arc::clone(&shutdown_flag),
        script_build_cache_dir: None,
    };
    let coordinator = start_coordinator(coordinator_config);

    let native_options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_app_id(WAYLAND_APP_ID)
            .with_inner_size([settings.window_width_px, settings.window_height_px])
            .with_min_inner_size([MIN_WINDOW_WIDTH_PX, MIN_WINDOW_HEIGHT_PX]),
        ..NativeOptions::default()
    };

    let launch_bundle = WorkbenchLaunchBundle {
        scripts: initial_scripts,
        custom_blocks: initial_custom_blocks,
        repository,
        custom_block_repository,
        settings_store,
        settings,
        coordinator,
        window_watcher: linux_runtime.window_watcher,
        tray_handle,
        tray_event_receiver,
        shutdown_flag: Arc::clone(&shutdown_flag),
    };

    let shared_context_for_creator = Arc::clone(&shared_egui_context);
    info!("Launching ClickForge native desktop workbench");

    let run_result = eframe::run_native(
        WINDOW_TITLE,
        native_options,
        Box::new(move |creation_context| {
            apply_forge_slate_theme(&creation_context.egui_ctx);
            if let Ok(mut guard) = shared_context_for_creator.lock() {
                *guard = Some(creation_context.egui_ctx.clone());
            }
            Ok(Box::new(ClickForgeWorkbenchApp::new(launch_bundle)))
        }),
    );

    shutdown_flag.store(true, Ordering::SeqCst);

    if let Err(eframe_error) = run_result {
        error!("ClickForge UI exited with error: {eframe_error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn spawn_signal_bridge_thread(
    shutdown_flag: Arc<AtomicBool>,
    shared_egui_context: Arc<Mutex<Option<egui::Context>>>,
) {
    let _ = thread::Builder::new()
        .name("clickforge-signal-bridge".to_string())
        .spawn(move || {
            while !shutdown_flag.load(Ordering::Relaxed) {
                if SIGNAL_TERMINATION_REQUESTED.load(Ordering::SeqCst) {
                    shutdown_flag.store(true, Ordering::SeqCst);
                    if let Ok(guard) = shared_egui_context.lock()
                        && let Some(egui_context) = guard.as_ref()
                    {
                        egui_context.request_repaint();
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(SIGNAL_BRIDGE_POLL_INTERVAL_MS));
            }
        });
}
