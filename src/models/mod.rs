mod block_tree;
pub mod blocks;
pub mod custom_blocks;
pub mod runtime_contracts;
pub mod script;
pub mod target;
pub mod validation;

pub use blocks::{
    BlockCategory, BlockId, BlockSlotTarget, ComparisonOperator, ConditionExpr,
    MAX_TRIGGER_AND_INPUTS, StatementBlock, StatementKind, TriggerBlock, VisualProgram,
};
pub use custom_blocks::{
    CustomBlockColor, CustomBlockDefinition, MAX_CUSTOM_BLOCK_CALL_DEPTH,
    MAX_CUSTOM_BLOCK_NAME_CHARS, MAX_CUSTOM_BLOCK_STEPS, MAX_CUSTOM_BLOCKS, lookup_custom_block,
    validate_custom_block_definition,
};
pub use runtime_contracts::{
    CompileResultSnapshot, CompilerDiagnostic, CoordinatorCommand, CoordinatorSnapshot,
    DiagnosticSeverity, InputError, InputSink, InputSubsystemStatus, MAX_DELAY_HISTORY_SAMPLES,
    MAX_INTERCEPT_REQUIRED_INPUTS, MAX_TELEMETRY_ENTRIES, NoopTriggerInterceptControl,
    PhysicalInputReader, ScriptExecutionPhase, ScriptRuntimeStatus, TelemetryLevel,
    TelemetryLogEntry, TrayRuntimeState, TrayUiEvent, TriggerInterceptControl,
    TriggerInterceptSpec,
};
pub use script::{ScriptDocument, ScriptMode};
pub use target::{
    ActiveWindowSnapshot, GameTargetRule, RunningProcessEntry, TargetMatchMode, TargetScope,
    correlate_process_for_window,
};
pub use validation::{
    ValidationError, ValidationIssue, ValidationIssueCode, ValidationReport,
    inspect_script_validation, inspect_script_validation_with_custom_blocks,
    validate_script_for_persistence_and_execution, validate_script_with_custom_blocks,
};
