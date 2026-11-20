use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::blocks::BlockId;
use crate::models::script::ScriptDocument;
use crate::models::target::{ActiveWindowSnapshot, RunningProcessEntry};

/// Maximum number of telemetry log lines retained in memory for the UI drawer.
pub const MAX_TELEMETRY_ENTRIES: usize = 500;

/// Maximum number of recent sampled delay measurements retained for the histogram.
pub const MAX_DELAY_HISTORY_SAMPLES: usize = 128;

/// Errors emitted by virtual input injection or physical input interception.
#[derive(Debug, Error)]
pub enum InputError {
    #[error("Virtual or physical input device is unavailable: {detail}")]
    DeviceUnavailable { detail: String },

    #[error("Input event injection failed: {detail}")]
    InjectionFailed { detail: String },

    #[error("Permission denied accessing '{path}': {detail}")]
    PermissionDenied { path: String, detail: String },

    #[error("Unsupported input code: {detail}")]
    UnsupportedInput { detail: String },
}

/// Thread-safe virtual input injection sink implemented by Linux `uinput` and test mocks.
pub trait InputSink: Send + Sync {
    fn mouse_down(&self, mouse_button: MouseButton) -> Result<(), InputError>;
    fn mouse_up(&self, mouse_button: MouseButton) -> Result<(), InputError>;
    fn mouse_move_relative(&self, delta_x_px: i32, delta_y_px: i32) -> Result<(), InputError>;
    fn mouse_scroll(&self, vertical_steps: i32) -> Result<(), InputError>;
    fn key_down(&self, key_code: KeyCode) -> Result<(), InputError>;
    fn key_up(&self, key_code: KeyCode) -> Result<(), InputError>;
    fn release_all_pressed(&self) -> Result<(), InputError>;
}

/// Thread-safe physical input state reader backed by `evdev` or test doubles.
pub trait PhysicalInputReader: Send + Sync {
    fn is_mouse_pressed(&self, mouse_button: MouseButton) -> bool;
    fn is_key_pressed(&self, key_code: KeyCode) -> bool;

    /// Checks whether the supplied mouse or keyboard trigger is currently held physically.
    fn is_trigger_pressed(&self, trigger: InputTrigger) -> bool {
        match trigger {
            InputTrigger::Mouse(mouse_button) => self.is_mouse_pressed(mouse_button),
            InputTrigger::Key(key_code) => self.is_key_pressed(key_code),
        }
    }

    /// Returns the most recently pressed physical key and monotonic sequence number, if tracked.
    fn last_pressed_key_snapshot(&self) -> Option<(KeyCode, u64)> {
        None
    }

    /// Returns all currently pressed physical keyboard keys, if tracked.
    fn active_pressed_keys(&self) -> Vec<KeyCode> {
        Vec::new()
    }
}

/// Active trigger interception configuration for optional `EVIOCGRAB` suppression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TriggerInterceptSpec {
    pub trigger_input: InputTrigger,
}

/// Control surface used by the coordinator to toggle safe `EVIOCGRAB` trigger interception.
pub trait TriggerInterceptControl: Send + Sync {
    fn set_intercept(&self, spec: Option<TriggerInterceptSpec>) -> Result<(), InputError>;
    fn is_intercept_active(&self) -> bool;
}

/// Fallback no-op interceptor used in headless tests or when `/dev/input` is unavailable.
#[derive(Debug, Default)]
pub struct NoopTriggerInterceptControl;

impl TriggerInterceptControl for NoopTriggerInterceptControl {
    fn set_intercept(&self, _spec: Option<TriggerInterceptSpec>) -> Result<(), InputError> {
        Ok(())
    }

    fn is_intercept_active(&self) -> bool {
        false
    }
}

/// Lifecycle execution phase of a single script inside the background coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScriptExecutionPhase {
    #[default]
    Disabled,
    WaitingForFocus,
    Armed,
    Running,
    Compiling,
    StoppedByKillSwitch,
    Error,
}

impl ScriptExecutionPhase {
    /// Returns the concise UI and tray label for this execution phase.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::WaitingForFocus => "Waiting for Target Focus",
            Self::Armed => "Armed",
            Self::Running => "Running",
            Self::Compiling => "Compiling",
            Self::StoppedByKillSwitch => "Stopped by Kill-Switch",
            Self::Error => "Error",
        }
    }
}

/// Live runtime telemetry for a single managed script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptRuntimeStatus {
    pub script_id: String,
    pub phase: ScriptExecutionPhase,
    pub is_focus_matched: bool,
    pub is_armed_by_toggle: bool,
    pub is_intercept_grab_active: bool,
    pub active_block_id: Option<BlockId>,
    pub last_sampled_delay_ms: Option<u64>,
    pub total_actions_emitted: u64,
    pub last_error: Option<String>,
    pub updated_at_unix_ms: u64,
}

impl ScriptRuntimeStatus {
    /// Creates an initial disabled status entry for `script_id`.
    pub fn disabled(script_id: impl Into<String>) -> Self {
        Self {
            script_id: script_id.into(),
            phase: ScriptExecutionPhase::Disabled,
            is_focus_matched: false,
            is_armed_by_toggle: false,
            is_intercept_grab_active: false,
            active_block_id: None,
            last_sampled_delay_ms: None,
            total_actions_emitted: 0,
            last_error: None,
            updated_at_unix_ms: 0,
        }
    }
}

/// Severity of a structured `rustc` compiler diagnostic for Advanced Rust scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
}

/// Parsed compiler diagnostic displayed inline in the Advanced Rust editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerDiagnostic {
    pub severity: DiagnosticSeverity,
    pub file_name: Option<String>,
    pub line_number: Option<u32>,
    pub column_number: Option<u32>,
    pub message: String,
    pub rendered_excerpt: Option<String>,
}

/// Outcome of compiling an Advanced Rust script via the pre-warmed Cargo workspace cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileResultSnapshot {
    pub script_id: String,
    pub is_success: bool,
    pub duration_ms: u64,
    pub binary_path: Option<String>,
    pub diagnostics: Vec<CompilerDiagnostic>,
    pub completed_at_unix_ms: u64,
}

/// Log severity level for bounded coordinator telemetry events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryLevel {
    Info,
    Warning,
    Error,
    Action,
}

/// Single entry inside the bounded telemetry drawer ring buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryLogEntry {
    pub sequence_number: u64,
    pub timestamp_unix_ms: u64,
    pub level: TelemetryLevel,
    pub script_id: Option<String>,
    pub message: String,
}

/// Readiness report for Linux `/dev/uinput`, `/dev/input/event*`, and compositor watchers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InputSubsystemStatus {
    pub is_uinput_ready: bool,
    pub uinput_error: Option<String>,
    pub is_evdev_ready: bool,
    pub monitored_device_count: usize,
    pub evdev_error: Option<String>,
    pub is_trigger_intercept_active: bool,
    pub compositor_name: String,
}

/// Immutable UI snapshot published periodically or on state change by the coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CoordinatorSnapshot {
    pub focused_window: Option<ActiveWindowSnapshot>,
    pub open_windows: Vec<ActiveWindowSnapshot>,
    pub running_processes: Vec<RunningProcessEntry>,
    pub script_statuses: Vec<ScriptRuntimeStatus>,
    pub recent_delay_samples_ms: Vec<u64>,
    pub telemetry_logs: Vec<TelemetryLogEntry>,
    pub input_status: InputSubsystemStatus,
    pub last_compile_result: Option<CompileResultSnapshot>,
    pub emergency_stop_count: u64,
    pub is_emergency_stopped: bool,
}

impl CoordinatorSnapshot {
    /// Looks up the latest runtime status for a specific script identifier.
    pub fn status_for_script(&self, script_id: &str) -> Option<&ScriptRuntimeStatus> {
        self.script_statuses
            .iter()
            .find(|status| status.script_id == script_id)
    }
}

/// Control messages sent from the `egui` workbench or system tray to the coordinator thread.
#[derive(Debug, Clone)]
pub enum CoordinatorCommand {
    SyncScripts(Vec<ScriptDocument>),
    UpsertScript(ScriptDocument),
    RemoveScript { script_id: String },
    SetScriptEnabled { script_id: String, is_enabled: bool },
    CompileRustScript(ScriptDocument),
    EmergencyStopAll { reason: String },
    DisableAllScripts,
    ClearEmergencyStop,
    RefreshWindowsAndProcesses,
    Shutdown,
}

/// Menu actions emitted by the `ksni` Linux system tray toward the main application window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrayUiEvent {
    ShowMainWindow,
    ToggleMainWindow,
    EmergencyStopAll,
    DisableAllScripts,
    QuitApplication,
}

/// Condensed runtime summary pushed from the coordinator to the `ksni` tray icon.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayRuntimeState {
    pub overall_phase: ScriptExecutionPhase,
    pub enabled_script_count: usize,
    pub active_script_count: usize,
    pub focused_window_title: Option<String>,
    pub status_tooltip: String,
}
