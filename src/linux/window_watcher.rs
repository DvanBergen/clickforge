use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tracing::{debug, warn};

use crate::linux::niri_ipc::{self, NIRI_COMPOSITOR_NAME, NiriEventStream, NiriStreamEvent};
use crate::linux::process_scanner;
use crate::models::target::{ActiveWindowSnapshot, RunningProcessEntry};

const FOCUS_EVENT_CHANNEL_CAPACITY: usize = 64;
const MAX_TRACKED_WINDOWS: usize = 512;
const PROCESS_REFRESH_INTERVAL_MS: u64 = 2_000;
const FALLBACK_POLL_INTERVAL_MS: u64 = 250;
const RECONNECT_BACKOFF_MS: u64 = 1_000;
const COMPOSITOR_SOCKET_TIMEOUT_MS: u64 = 1_000;
const MAX_SWAY_IPC_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
const MAX_SWAY_TREE_DEPTH: usize = 64;
const SWAY_IPC_MAGIC: &[u8; 6] = b"i3-ipc";
const SWAY_IPC_GET_TREE_TYPE: u32 = 4;
const HYPRLAND_COMPOSITOR_NAME: &str = "hyprland";
const SWAY_COMPOSITOR_NAME: &str = "sway";
const X11_COMPOSITOR_NAME: &str = "x11";
const UNKNOWN_COMPOSITOR_NAME: &str = "unknown";

#[derive(Debug, Clone)]
enum CompositorBackend {
    Niri { socket_path: String },
    Hyprland { socket_path: PathBuf },
    Sway { socket_path: String },
    X11,
    Unknown,
}

impl CompositorBackend {
    fn compositor_name(&self) -> &'static str {
        match self {
            Self::Niri { .. } => NIRI_COMPOSITOR_NAME,
            Self::Hyprland { .. } => HYPRLAND_COMPOSITOR_NAME,
            Self::Sway { .. } => SWAY_COMPOSITOR_NAME,
            Self::X11 => X11_COMPOSITOR_NAME,
            Self::Unknown => UNKNOWN_COMPOSITOR_NAME,
        }
    }
}

#[derive(Debug, Default)]
struct WatcherSharedState {
    focused_window: Option<ActiveWindowSnapshot>,
    open_windows: Vec<ActiveWindowSnapshot>,
    running_processes: Vec<RunningProcessEntry>,
}

/// Live Wayland/X11 window focus watcher and `/proc` process cache.
pub struct WindowWatcher {
    backend: CompositorBackend,
    shared_state: Arc<RwLock<WatcherSharedState>>,
    focus_event_sender: SyncSender<ActiveWindowSnapshot>,
    shutdown_flag: Arc<AtomicBool>,
}

impl WindowWatcher {
    /// Detects the active Linux compositor, loads initial snapshots, and starts the watcher thread.
    pub fn start(shutdown_flag: Arc<AtomicBool>) -> (Arc<Self>, Receiver<ActiveWindowSnapshot>) {
        let backend = detect_compositor_backend();
        let (focus_event_sender, focus_event_receiver) =
            mpsc::sync_channel(FOCUS_EVENT_CHANNEL_CAPACITY);
        let shared_state = Arc::new(RwLock::new(WatcherSharedState::default()));

        let watcher = Arc::new(Self {
            backend,
            shared_state,
            focus_event_sender,
            shutdown_flag,
        });

        watcher.refresh_now();
        let watcher_for_thread = Arc::clone(&watcher);
        let spawn_result = thread::Builder::new()
            .name("clickforge-window-watcher".to_string())
            .spawn(move || watcher_for_thread.run_watcher_loop());
        if let Err(spawn_error) = spawn_result {
            warn!("Failed to spawn window watcher thread: {spawn_error}");
        }

        (watcher, focus_event_receiver)
    }

    /// Returns the name of the detected compositor or windowing backend.
    pub fn compositor_name(&self) -> &str {
        self.backend.compositor_name()
    }

    /// Returns the latest focused desktop window snapshot, if any.
    pub fn snapshot_focused_window(&self) -> Option<ActiveWindowSnapshot> {
        let state_guard = self
            .shared_state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state_guard.focused_window.clone()
    }

    /// Returns the latest list of open desktop windows reported by the compositor.
    pub fn snapshot_open_windows(&self) -> Vec<ActiveWindowSnapshot> {
        let state_guard = self
            .shared_state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state_guard.open_windows.clone()
    }

    /// Returns the latest `/proc` snapshot of running Linux processes.
    pub fn snapshot_running_processes(&self) -> Vec<RunningProcessEntry> {
        let state_guard = self
            .shared_state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state_guard.running_processes.clone()
    }

    /// Synchronously refreshes `/proc` processes and live compositor windows immediately.
    pub fn refresh_now(&self) {
        let running_processes = process_scanner::scan_running_processes();
        let (mut open_windows, mut focused_window) = query_windows_for_backend(&self.backend);

        enrich_windows_with_processes(&mut open_windows, &running_processes);
        if let Some(ref mut focused_snapshot) = focused_window {
            enrich_single_window_with_processes(focused_snapshot, &running_processes);
        }

        let focus_update = {
            let mut state_guard = self
                .shared_state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous_focused = state_guard.focused_window.clone();
            state_guard.running_processes = running_processes;
            state_guard.open_windows = open_windows;
            state_guard.focused_window = focused_window.clone();
            compute_focus_change_notification(
                previous_focused.as_ref(),
                focused_window.as_ref(),
                self.compositor_name(),
            )
        };

        if let Some(changed_snapshot) = focus_update {
            self.emit_focus_snapshot(changed_snapshot);
        }
    }

    fn run_watcher_loop(&self) {
        match &self.backend {
            CompositorBackend::Niri { socket_path } => self.run_niri_event_loop(socket_path),
            _ => self.run_polling_loop(),
        }
    }

    fn run_niri_event_loop(&self, socket_path: &str) {
        let process_refresh_interval = Duration::from_millis(PROCESS_REFRESH_INTERVAL_MS);
        let mut last_process_refresh = Instant::now();

        while !self.shutdown_flag.load(Ordering::Acquire) {
            let mut event_stream = match NiriEventStream::connect(socket_path) {
                Ok(stream) => stream,
                Err(connect_error) => {
                    debug!("niri EventStream reconnect pending: {connect_error}");
                    self.sleep_interruptible_ms(RECONNECT_BACKOFF_MS);
                    continue;
                }
            };

            while !self.shutdown_flag.load(Ordering::Acquire) {
                if last_process_refresh.elapsed() >= process_refresh_interval {
                    self.refresh_processes_only();
                    last_process_refresh = Instant::now();
                }

                match event_stream.read_next_event() {
                    Ok(Some(stream_event)) => self.apply_niri_stream_event(stream_event),
                    Ok(None) => continue,
                    Err(stream_error) => {
                        debug!("niri EventStream interrupted: {stream_error}");
                        break;
                    }
                }
            }
        }
    }

    fn run_polling_loop(&self) {
        let process_refresh_interval = Duration::from_millis(PROCESS_REFRESH_INTERVAL_MS);
        let mut last_process_refresh = Instant::now();

        while !self.shutdown_flag.load(Ordering::Acquire) {
            self.sleep_interruptible_ms(FALLBACK_POLL_INTERVAL_MS);
            if self.shutdown_flag.load(Ordering::Acquire) {
                break;
            }

            let should_refresh_processes =
                last_process_refresh.elapsed() >= process_refresh_interval;
            if should_refresh_processes {
                self.refresh_now();
                last_process_refresh = Instant::now();
            } else {
                self.refresh_windows_only();
            }
        }
    }

    fn refresh_processes_only(&self) {
        let running_processes = process_scanner::scan_running_processes();
        let mut state_guard = self
            .shared_state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state_guard.running_processes = running_processes;
    }

    fn refresh_windows_only(&self) {
        let (mut open_windows, mut focused_window) = query_windows_for_backend(&self.backend);
        let focus_update = {
            let mut state_guard = self
                .shared_state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let processes_snapshot = state_guard.running_processes.clone();
            enrich_windows_with_processes(&mut open_windows, &processes_snapshot);
            if let Some(ref mut focused_snapshot) = focused_window {
                enrich_single_window_with_processes(focused_snapshot, &processes_snapshot);
            }
            let previous_focused = state_guard.focused_window.clone();
            state_guard.open_windows = open_windows;
            state_guard.focused_window = focused_window.clone();
            compute_focus_change_notification(
                previous_focused.as_ref(),
                focused_window.as_ref(),
                self.compositor_name(),
            )
        };

        if let Some(changed_snapshot) = focus_update {
            self.emit_focus_snapshot(changed_snapshot);
        }
    }

    fn apply_niri_stream_event(&self, stream_event: NiriStreamEvent) {
        if matches!(stream_event, NiriStreamEvent::Ignored) {
            return;
        }

        let focus_update = {
            let mut state_guard = self
                .shared_state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous_focused = state_guard.focused_window.clone();
            apply_niri_event_to_state(&mut state_guard, stream_event);
            compute_focus_change_notification(
                previous_focused.as_ref(),
                state_guard.focused_window.as_ref(),
                NIRI_COMPOSITOR_NAME,
            )
        };

        if let Some(changed_snapshot) = focus_update {
            self.emit_focus_snapshot(changed_snapshot);
        }
    }

    fn emit_focus_snapshot(&self, snapshot: ActiveWindowSnapshot) {
        let _ = self.focus_event_sender.try_send(snapshot);
    }

    fn sleep_interruptible_ms(&self, total_delay_ms: u64) {
        let slice_ms = 50u64;
        let mut remaining_ms = total_delay_ms;
        while remaining_ms > 0 && !self.shutdown_flag.load(Ordering::Acquire) {
            let sleep_step_ms = remaining_ms.min(slice_ms);
            thread::sleep(Duration::from_millis(sleep_step_ms));
            remaining_ms = remaining_ms.saturating_sub(sleep_step_ms);
        }
    }
}

fn apply_niri_event_to_state(state: &mut WatcherSharedState, stream_event: NiriStreamEvent) {
    match stream_event {
        NiriStreamEvent::WindowsChanged(mut windows) => {
            windows.truncate(MAX_TRACKED_WINDOWS);
            enrich_windows_with_processes(&mut windows, &state.running_processes);
            state.focused_window = windows.iter().find(|window| window.is_focused).cloned();
            state.open_windows = windows;
        }
        NiriStreamEvent::WindowOpenedOrChanged(mut updated_window) => {
            enrich_single_window_with_processes(&mut updated_window, &state.running_processes);
            if updated_window.is_focused {
                for existing_window in &mut state.open_windows {
                    existing_window.is_focused =
                        existing_window.window_id == updated_window.window_id;
                }
            }
            if let Some(existing) = state
                .open_windows
                .iter_mut()
                .find(|window| window.window_id == updated_window.window_id)
            {
                *existing = updated_window.clone();
            } else if state.open_windows.len() < MAX_TRACKED_WINDOWS {
                state.open_windows.push(updated_window.clone());
            }
            if updated_window.is_focused {
                state.focused_window = Some(updated_window);
            } else if state
                .focused_window
                .as_ref()
                .is_some_and(|focused| focused.window_id == updated_window.window_id)
            {
                state.focused_window = None;
            }
        }
        NiriStreamEvent::WindowClosed { window_id } => {
            state
                .open_windows
                .retain(|window| window.window_id != Some(window_id));
            if state
                .focused_window
                .as_ref()
                .is_some_and(|focused| focused.window_id == Some(window_id))
            {
                state.focused_window = None;
            }
        }
        NiriStreamEvent::WindowFocusChanged { focused_window_id } => {
            let mut next_focused = None;
            for window in &mut state.open_windows {
                let is_now_focused =
                    focused_window_id.is_some() && window.window_id == focused_window_id;
                window.is_focused = is_now_focused;
                if is_now_focused {
                    next_focused = Some(window.clone());
                }
            }
            state.focused_window = next_focused;
        }
        NiriStreamEvent::Ignored => {}
    }
}

fn compute_focus_change_notification(
    previous_focused: Option<&ActiveWindowSnapshot>,
    current_focused: Option<&ActiveWindowSnapshot>,
    compositor_name: &str,
) -> Option<ActiveWindowSnapshot> {
    if previous_focused == current_focused {
        return None;
    }
    if let Some(active_snapshot) = current_focused {
        return Some(active_snapshot.clone());
    }
    Some(ActiveWindowSnapshot {
        window_id: None,
        title: String::new(),
        app_id: String::new(),
        pid: None,
        is_focused: false,
        compositor_name: compositor_name.to_string(),
    })
}

fn enrich_windows_with_processes(
    windows: &mut [ActiveWindowSnapshot],
    running_processes: &[RunningProcessEntry],
) {
    for window in windows {
        enrich_single_window_with_processes(window, running_processes);
    }
}

fn enrich_single_window_with_processes(
    window: &mut ActiveWindowSnapshot,
    running_processes: &[RunningProcessEntry],
) {
    if !window.app_id.trim().is_empty() {
        return;
    }
    let Some(window_pid) = window.pid else {
        return;
    };
    let Some(direct_process) = running_processes
        .iter()
        .find(|entry| entry.pid == window_pid && !entry.is_xwayland_proxy())
    else {
        return;
    };
    if let Some(steam_app_id) = &direct_process.steam_app_id {
        window.app_id = format!("steam_app_{steam_app_id}");
    } else if !direct_process.executable_name.trim().is_empty() {
        window.app_id = direct_process.executable_name.clone();
    }
}

fn detect_compositor_backend() -> CompositorBackend {
    if let Some(socket_path) = niri_ipc::detect_niri_socket_path() {
        return CompositorBackend::Niri { socket_path };
    }
    if let Some(socket_path) = detect_hyprland_socket_path() {
        return CompositorBackend::Hyprland { socket_path };
    }
    if let Ok(sway_socket) = env::var("SWAYSOCK")
        && !sway_socket.trim().is_empty()
    {
        return CompositorBackend::Sway {
            socket_path: sway_socket.trim().to_string(),
        };
    }
    let is_x11_session = env::var("XDG_SESSION_TYPE")
        .map(|session_type| session_type.eq_ignore_ascii_case("x11"))
        .unwrap_or(false);
    let has_display = env::var("DISPLAY")
        .map(|display| !display.trim().is_empty())
        .unwrap_or(false);
    if is_x11_session || (has_display && env::var("WAYLAND_DISPLAY").is_err()) {
        return CompositorBackend::X11;
    }
    CompositorBackend::Unknown
}

fn detect_hyprland_socket_path() -> Option<PathBuf> {
    let instance_signature = env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let trimmed_signature = instance_signature.trim();
    if trimmed_signature.is_empty() {
        return None;
    }
    if let Ok(runtime_directory) = env::var("XDG_RUNTIME_DIR") {
        let runtime_socket = PathBuf::from(runtime_directory)
            .join("hypr")
            .join(trimmed_signature)
            .join(".socket.sock");
        if runtime_socket.exists() {
            return Some(runtime_socket);
        }
    }
    let fallback_socket = PathBuf::from("/tmp/hypr")
        .join(trimmed_signature)
        .join(".socket.sock");
    Some(fallback_socket)
}

fn query_windows_for_backend(
    backend: &CompositorBackend,
) -> (Vec<ActiveWindowSnapshot>, Option<ActiveWindowSnapshot>) {
    match backend {
        CompositorBackend::Niri { socket_path } => query_niri_windows(socket_path),
        CompositorBackend::Hyprland { socket_path } => query_hyprland_windows(socket_path),
        CompositorBackend::Sway { socket_path } => query_sway_windows(socket_path),
        CompositorBackend::X11 => query_x11_active_window(),
        CompositorBackend::Unknown => (Vec::new(), None),
    }
}

fn query_niri_windows(
    socket_path: &str,
) -> (Vec<ActiveWindowSnapshot>, Option<ActiveWindowSnapshot>) {
    let mut open_windows = niri_ipc::query_open_windows(socket_path).unwrap_or_default();
    open_windows.truncate(MAX_TRACKED_WINDOWS);
    let focused_window = niri_ipc::query_focused_window(socket_path)
        .ok()
        .flatten()
        .or_else(|| {
            open_windows
                .iter()
                .find(|window| window.is_focused)
                .cloned()
        });
    (open_windows, focused_window)
}

fn query_hyprland_windows(
    socket_path: &Path,
) -> (Vec<ActiveWindowSnapshot>, Option<ActiveWindowSnapshot>) {
    let active_address = send_hyprland_command(socket_path, "j/activewindow")
        .and_then(|json_text| serde_json::from_str::<Value>(&json_text).ok())
        .and_then(|value| value.get("address")?.as_str().map(ToString::to_string));

    let Some(clients_json) = send_hyprland_command(socket_path, "j/clients") else {
        return (Vec::new(), None);
    };
    let Ok(Value::Array(client_entries)) = serde_json::from_str::<Value>(&clients_json) else {
        return (Vec::new(), None);
    };

    let mut open_windows = Vec::with_capacity(client_entries.len().min(MAX_TRACKED_WINDOWS));
    let mut focused_window = None;

    for client_value in client_entries.into_iter().take(MAX_TRACKED_WINDOWS) {
        let address_text = client_value
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let is_focused = active_address
            .as_deref()
            .is_some_and(|focused_address| focused_address == address_text);
        let parsed_id = u64::from_str_radix(address_text.trim_start_matches("0x"), 16).ok();
        let pid = client_value
            .get("pid")
            .and_then(Value::as_i64)
            .and_then(|raw_pid| u32::try_from(raw_pid).ok());
        let snapshot = ActiveWindowSnapshot {
            window_id: parsed_id,
            title: client_value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            app_id: client_value
                .get("class")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            pid,
            is_focused,
            compositor_name: HYPRLAND_COMPOSITOR_NAME.to_string(),
        };
        if is_focused {
            focused_window = Some(snapshot.clone());
        }
        open_windows.push(snapshot);
    }

    (open_windows, focused_window)
}

fn send_hyprland_command(socket_path: &Path, command_text: &str) -> Option<String> {
    let mut stream = UnixStream::connect(socket_path).ok()?;
    let timeout = Duration::from_millis(COMPOSITOR_SOCKET_TIMEOUT_MS);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    stream.write_all(command_text.as_bytes()).ok()?;

    let mut response_bytes = Vec::new();
    let max_bytes = u64::try_from(MAX_SWAY_IPC_PAYLOAD_BYTES).unwrap_or(u64::MAX);
    stream
        .take(max_bytes)
        .read_to_end(&mut response_bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&response_bytes).to_string())
}

fn query_sway_windows(
    socket_path: &str,
) -> (Vec<ActiveWindowSnapshot>, Option<ActiveWindowSnapshot>) {
    let Some(tree_json) = query_sway_tree_json(socket_path) else {
        return (Vec::new(), None);
    };
    let Ok(root_node) = serde_json::from_str::<Value>(&tree_json) else {
        return (Vec::new(), None);
    };
    let mut open_windows = Vec::new();
    let mut focused_window = None;
    collect_sway_nodes(&root_node, 0, &mut open_windows, &mut focused_window);
    (open_windows, focused_window)
}

fn query_sway_tree_json(socket_path: &str) -> Option<String> {
    let mut stream = UnixStream::connect(socket_path).ok()?;
    let timeout = Duration::from_millis(COMPOSITOR_SOCKET_TIMEOUT_MS);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let mut request_header = [0u8; 14];
    request_header[0..6].copy_from_slice(SWAY_IPC_MAGIC);
    request_header[6..10].copy_from_slice(&0u32.to_le_bytes());
    request_header[10..14].copy_from_slice(&SWAY_IPC_GET_TREE_TYPE.to_le_bytes());
    stream.write_all(&request_header).ok()?;

    let mut response_header = [0u8; 14];
    stream.read_exact(&mut response_header).ok()?;
    if &response_header[0..6] != SWAY_IPC_MAGIC {
        return None;
    }
    let payload_length_bytes = u32::from_le_bytes([
        response_header[6],
        response_header[7],
        response_header[8],
        response_header[9],
    ]) as usize;
    if payload_length_bytes > MAX_SWAY_IPC_PAYLOAD_BYTES {
        return None;
    }

    let mut payload_bytes = vec![0u8; payload_length_bytes];
    stream.read_exact(&mut payload_bytes).ok()?;
    Some(String::from_utf8_lossy(&payload_bytes).to_string())
}

fn collect_sway_nodes(
    node_value: &Value,
    recursion_depth: usize,
    open_windows: &mut Vec<ActiveWindowSnapshot>,
    focused_window: &mut Option<ActiveWindowSnapshot>,
) {
    if recursion_depth >= MAX_SWAY_TREE_DEPTH || open_windows.len() >= MAX_TRACKED_WINDOWS {
        return;
    }
    let node_type = node_value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let pid = node_value
        .get("pid")
        .and_then(Value::as_i64)
        .and_then(|raw_pid| u32::try_from(raw_pid).ok());
    let has_window_identity = (node_type == "con" || node_type == "floating_con") && pid.is_some();

    if has_window_identity {
        let app_id = node_value
            .get("app_id")
            .and_then(Value::as_str)
            .or_else(|| {
                node_value
                    .get("window_properties")
                    .and_then(|properties| properties.get("class"))
                    .and_then(Value::as_str)
            })
            .unwrap_or_default()
            .to_string();
        let title = node_value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let is_focused = node_value
            .get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let snapshot = ActiveWindowSnapshot {
            window_id: node_value.get("id").and_then(Value::as_u64),
            title,
            app_id,
            pid,
            is_focused,
            compositor_name: SWAY_COMPOSITOR_NAME.to_string(),
        };
        if is_focused {
            *focused_window = Some(snapshot.clone());
        }
        open_windows.push(snapshot);
    }

    let next_depth = recursion_depth.saturating_add(1);
    for array_key in ["nodes", "floating_nodes"] {
        if let Some(Value::Array(children)) = node_value.get(array_key) {
            for child_node in children {
                collect_sway_nodes(child_node, next_depth, open_windows, focused_window);
            }
        }
    }
}

fn query_x11_active_window() -> (Vec<ActiveWindowSnapshot>, Option<ActiveWindowSnapshot>) {
    let Ok(root_output) = Command::new("xprop")
        .args(["-root", "_NET_ACTIVE_WINDOW"])
        .output()
    else {
        return (Vec::new(), None);
    };
    if !root_output.status.success() {
        return (Vec::new(), None);
    }
    let root_stdout = String::from_utf8_lossy(&root_output.stdout);
    let Some(active_window_hex) = parse_xprop_active_window_id(&root_stdout) else {
        return (Vec::new(), None);
    };

    let Ok(detail_output) = Command::new("xprop")
        .args([
            "-id",
            &active_window_hex,
            "_NET_WM_NAME",
            "WM_NAME",
            "WM_CLASS",
            "_NET_WM_PID",
        ])
        .output()
    else {
        return (Vec::new(), None);
    };
    if !detail_output.status.success() {
        return (Vec::new(), None);
    }

    let detail_stdout = String::from_utf8_lossy(&detail_output.stdout);
    let window_id = u64::from_str_radix(active_window_hex.trim_start_matches("0x"), 16).ok();
    let title = parse_xprop_quoted_value(&detail_stdout, "_NET_WM_NAME")
        .or_else(|| parse_xprop_quoted_value(&detail_stdout, "WM_NAME"))
        .unwrap_or_default();
    let app_id = parse_xprop_quoted_value(&detail_stdout, "WM_CLASS").unwrap_or_default();
    let pid = parse_xprop_numeric_value(&detail_stdout, "_NET_WM_PID");

    let snapshot = ActiveWindowSnapshot {
        window_id,
        title,
        app_id,
        pid,
        is_focused: true,
        compositor_name: X11_COMPOSITOR_NAME.to_string(),
    };
    (vec![snapshot.clone()], Some(snapshot))
}

fn parse_xprop_active_window_id(stdout_text: &str) -> Option<String> {
    let raw_id = stdout_text.split('#').nth(1)?.trim();
    let hex_token = raw_id.split_whitespace().next()?;
    if hex_token.is_empty() || hex_token == "0x0" {
        return None;
    }
    Some(hex_token.to_string())
}

fn parse_xprop_quoted_value(stdout_text: &str, property_name: &str) -> Option<String> {
    for line in stdout_text.lines() {
        if !line.starts_with(property_name) {
            continue;
        }
        let (_, value_part) = line.split_once('=')?;
        let first_quote = value_part.find('"')?;
        let remainder = value_part.get(first_quote.saturating_add(1)..)?;
        let closing_quote = remainder.find('"')?;
        return Some(remainder[..closing_quote].to_string());
    }
    None
}

fn parse_xprop_numeric_value(stdout_text: &str, property_name: &str) -> Option<u32> {
    for line in stdout_text.lines() {
        if !line.starts_with(property_name) {
            continue;
        }
        let (_, value_part) = line.split_once('=')?;
        return value_part.trim().parse::<u32>().ok();
    }
    None
}
