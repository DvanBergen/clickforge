mod block_tree;
pub mod blocks;
pub mod runtime_contracts;
pub mod script;
pub mod target;
pub mod validation;

pub use blocks::{
    BlockCategory, BlockId, BlockSlotTarget, ComparisonOperator, ConditionExpr, StatementBlock,
    StatementKind, TriggerBlock, VisualProgram,
};
pub use runtime_contracts::{
    CompileResultSnapshot, CompilerDiagnostic, CoordinatorCommand, CoordinatorSnapshot,
    DiagnosticSeverity, InputError, InputSink, InputSubsystemStatus, MAX_DELAY_HISTORY_SAMPLES,
    MAX_TELEMETRY_ENTRIES, NoopTriggerInterceptControl, PhysicalInputReader, ScriptExecutionPhase,
    ScriptRuntimeStatus, TelemetryLevel, TelemetryLogEntry, TrayRuntimeState, TrayUiEvent,
    TriggerInterceptControl, TriggerInterceptSpec,
};
pub use script::{ScriptDocument, ScriptMode};
pub use target::{
    ActiveWindowSnapshot, GameTargetRule, RunningProcessEntry, TargetMatchMode, TargetScope,
    correlate_process_for_window,
};
pub use validation::{
    ValidationError, ValidationIssue, ValidationIssueCode, ValidationReport,
    inspect_script_validation, validate_script_for_persistence_and_execution,
};
