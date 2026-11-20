use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clickforge_sdk::InputTrigger;
use rand::RngExt;

use crate::engine::timing::{
    perform_key_tap_on_sink, perform_mouse_click_on_sink, perform_type_text_on_sink,
    sample_clamped_gaussian_delay_ms, sample_uniform_delay_ms, wait_interruptible_ms,
};
use crate::models::{
    BlockId, ConditionExpr, InputSink, PhysicalInputReader, StatementBlock, StatementKind,
    TriggerBlock, VisualProgram,
};

/// Maximum distinct local script variables permitted per Visual Block execution.
pub const MAX_SCRIPT_VARIABLES: usize = 64;

/// Minimum loop yield in microseconds preventing tight zero-delay busy-spin CPU lockups.
pub const MIN_LOOP_YIELD_MICROS: u64 = 250;

/// Idle polling slice in milliseconds while waiting for a trigger input to engage.
pub const TRIGGER_IDLE_POLL_MS: u64 = 4;

/// Optional telemetry callbacks invoked during Visual Block interpretation.
#[derive(Default)]
pub struct BlockExecutionHooks {
    pub on_active_block_changed: Option<Box<dyn FnMut(Option<BlockId>) + Send>>,
    pub on_delay_sampled_ms: Option<Box<dyn FnMut(u64) + Send>>,
    pub on_action_executed: Option<Box<dyn FnMut() + Send>>,
}

impl BlockExecutionHooks {
    fn report_active_block(&mut self, block_id: Option<BlockId>) {
        if let Some(callback) = self.on_active_block_changed.as_mut() {
            callback(block_id);
        }
    }

    fn report_delay_sampled_ms(&mut self, delay_ms: u64) {
        if let Some(callback) = self.on_delay_sampled_ms.as_mut() {
            callback(delay_ms);
        }
    }

    fn report_action_executed(&mut self) {
        if let Some(callback) = self.on_action_executed.as_mut() {
            callback();
        }
    }
}

/// Terminal reason explaining why a Visual Block interpreter invocation exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramStopReason {
    Aborted,
    StoppedByBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockControlFlow {
    Continue,
    BreakLoop,
    StopScript,
    Aborted,
}

struct InputReleaseGuard {
    input_sink: Arc<dyn InputSink>,
}

impl Drop for InputReleaseGuard {
    fn drop(&mut self) {
        let _ = self.input_sink.release_all_pressed();
    }
}

struct InterpreterState<'run> {
    input_sink: Arc<dyn InputSink>,
    physical_input: Arc<dyn PhysicalInputReader>,
    abort_flag: &'run AtomicBool,
    hooks: &'run mut BlockExecutionHooks,
    variables: HashMap<String, f64>,
    last_active_block_id: Option<BlockId>,
}

impl<'run> InterpreterState<'run> {
    fn is_aborted(&self) -> bool {
        self.abort_flag.load(Ordering::SeqCst)
    }

    fn set_active_block(&mut self, block_id: Option<BlockId>) {
        if self.last_active_block_id == block_id {
            return;
        }
        self.last_active_block_id = block_id;
        self.hooks.report_active_block(block_id);
    }

    fn run_statement_slice(
        &mut self,
        statements: &[StatementBlock],
    ) -> Result<BlockControlFlow, String> {
        for block in statements {
            if self.is_aborted() {
                return Ok(BlockControlFlow::Aborted);
            }
            self.set_active_block(Some(block.id));
            let flow_signal = self.execute_single_block(block)?;
            if flow_signal != BlockControlFlow::Continue {
                return Ok(flow_signal);
            }
        }
        Ok(BlockControlFlow::Continue)
    }

    fn execute_single_block(&mut self, block: &StatementBlock) -> Result<BlockControlFlow, String> {
        match &block.kind {
            StatementKind::IfThen {
                condition,
                then_blocks,
            } => self.execute_if_then(condition, then_blocks),
            StatementKind::IfThenElse {
                condition,
                then_blocks,
                else_blocks,
            } => self.execute_if_then_else(condition, then_blocks, else_blocks),
            StatementKind::WhileCondition {
                condition,
                body_blocks,
            } => self.execute_while_condition(condition, body_blocks),
            StatementKind::RepeatCount {
                repeat_count,
                body_blocks,
            } => self.execute_repeat_count(*repeat_count, body_blocks),
            StatementKind::BreakLoop => Ok(BlockControlFlow::BreakLoop),
            StatementKind::StopScript => Ok(BlockControlFlow::StopScript),
            StatementKind::DoNothing => Ok(BlockControlFlow::Continue),
            StatementKind::MouseClick { .. }
            | StatementKind::MouseDown { .. }
            | StatementKind::MouseUp { .. }
            | StatementKind::MouseMoveRelative { .. }
            | StatementKind::MouseScroll { .. }
            | StatementKind::KeyTap { .. }
            | StatementKind::KeyDown { .. }
            | StatementKind::KeyUp { .. }
            | StatementKind::TypeText { .. } => self.execute_input_action(&block.kind),
            StatementKind::WaitFixed { .. }
            | StatementKind::WaitRandomUniform { .. }
            | StatementKind::WaitRandomGaussian { .. } => self.execute_wait_action(&block.kind),
            StatementKind::SetVariable {
                variable_name,
                value,
            } => self.execute_set_variable(variable_name, *value),
            StatementKind::ChangeVariable {
                variable_name,
                delta_value,
            } => self.execute_change_variable(variable_name, *delta_value),
        }
    }

    fn execute_if_then(
        &mut self,
        condition: &ConditionExpr,
        then_blocks: &[StatementBlock],
    ) -> Result<BlockControlFlow, String> {
        if !self.evaluate_condition(condition) {
            return Ok(BlockControlFlow::Continue);
        }
        self.run_statement_slice(then_blocks)
    }

    fn execute_if_then_else(
        &mut self,
        condition: &ConditionExpr,
        then_blocks: &[StatementBlock],
        else_blocks: &[StatementBlock],
    ) -> Result<BlockControlFlow, String> {
        if self.evaluate_condition(condition) {
            return self.run_statement_slice(then_blocks);
        }
        self.run_statement_slice(else_blocks)
    }

    fn execute_while_condition(
        &mut self,
        condition: &ConditionExpr,
        body_blocks: &[StatementBlock],
    ) -> Result<BlockControlFlow, String> {
        while !self.is_aborted() && self.evaluate_condition(condition) {
            let iteration_started_at = Instant::now();
            let flow_signal = self.run_statement_slice(body_blocks)?;
            match flow_signal {
                BlockControlFlow::Continue => enforce_min_loop_yield(iteration_started_at),
                BlockControlFlow::BreakLoop => return Ok(BlockControlFlow::Continue),
                BlockControlFlow::StopScript | BlockControlFlow::Aborted => {
                    return Ok(flow_signal);
                }
            }
        }
        if self.is_aborted() {
            return Ok(BlockControlFlow::Aborted);
        }
        Ok(BlockControlFlow::Continue)
    }

    fn execute_repeat_count(
        &mut self,
        repeat_count: u32,
        body_blocks: &[StatementBlock],
    ) -> Result<BlockControlFlow, String> {
        for _iteration_index in 0..repeat_count {
            if self.is_aborted() {
                return Ok(BlockControlFlow::Aborted);
            }
            let iteration_started_at = Instant::now();
            let flow_signal = self.run_statement_slice(body_blocks)?;
            match flow_signal {
                BlockControlFlow::Continue => enforce_min_loop_yield(iteration_started_at),
                BlockControlFlow::BreakLoop => return Ok(BlockControlFlow::Continue),
                BlockControlFlow::StopScript | BlockControlFlow::Aborted => {
                    return Ok(flow_signal);
                }
            }
        }
        Ok(BlockControlFlow::Continue)
    }

    fn execute_input_action(&mut self, kind: &StatementKind) -> Result<BlockControlFlow, String> {
        let abort_flag = self.abort_flag;
        let check_abort = || abort_flag.load(Ordering::SeqCst);

        match kind {
            StatementKind::MouseClick {
                mouse_button,
                hold_duration_ms,
            } => {
                let completed = perform_mouse_click_on_sink(
                    self.input_sink.as_ref(),
                    *mouse_button,
                    *hold_duration_ms,
                    &check_abort,
                )?;
                self.hooks.report_action_executed();
                Ok(flow_from_completed(completed))
            }
            StatementKind::MouseDown { mouse_button } => {
                self.input_sink
                    .mouse_down(*mouse_button)
                    .map_err(|input_error| input_error.to_string())?;
                self.hooks.report_action_executed();
                Ok(BlockControlFlow::Continue)
            }
            StatementKind::MouseUp { mouse_button } => {
                self.input_sink
                    .mouse_up(*mouse_button)
                    .map_err(|input_error| input_error.to_string())?;
                self.hooks.report_action_executed();
                Ok(BlockControlFlow::Continue)
            }
            StatementKind::MouseMoveRelative {
                delta_x_px,
                delta_y_px,
            } => {
                self.input_sink
                    .mouse_move_relative(*delta_x_px, *delta_y_px)
                    .map_err(|input_error| input_error.to_string())?;
                self.hooks.report_action_executed();
                Ok(BlockControlFlow::Continue)
            }
            StatementKind::MouseScroll { vertical_steps } => {
                self.input_sink
                    .mouse_scroll(*vertical_steps)
                    .map_err(|input_error| input_error.to_string())?;
                self.hooks.report_action_executed();
                Ok(BlockControlFlow::Continue)
            }
            StatementKind::KeyTap {
                key_code,
                hold_duration_ms,
            } => {
                let completed = perform_key_tap_on_sink(
                    self.input_sink.as_ref(),
                    *key_code,
                    *hold_duration_ms,
                    &check_abort,
                )?;
                self.hooks.report_action_executed();
                Ok(flow_from_completed(completed))
            }
            StatementKind::KeyDown { key_code } => {
                self.input_sink
                    .key_down(*key_code)
                    .map_err(|input_error| input_error.to_string())?;
                self.hooks.report_action_executed();
                Ok(BlockControlFlow::Continue)
            }
            StatementKind::KeyUp { key_code } => {
                self.input_sink
                    .key_up(*key_code)
                    .map_err(|input_error| input_error.to_string())?;
                self.hooks.report_action_executed();
                Ok(BlockControlFlow::Continue)
            }
            StatementKind::TypeText {
                text,
                keystroke_delay_ms,
                jitter_ms,
            } => {
                let mut sampled_delays_ms = Vec::new();
                let mut action_count: usize = 0;
                let completed = perform_type_text_on_sink(
                    self.input_sink.as_ref(),
                    text,
                    *keystroke_delay_ms,
                    *jitter_ms,
                    &check_abort,
                    &mut |delay_ms| sampled_delays_ms.push(delay_ms),
                    &mut || action_count = action_count.saturating_add(1),
                )?;
                for sampled_delay_ms in sampled_delays_ms {
                    self.hooks.report_delay_sampled_ms(sampled_delay_ms);
                }
                for _action_index in 0..action_count {
                    self.hooks.report_action_executed();
                }
                Ok(flow_from_completed(completed))
            }
            _ => Ok(BlockControlFlow::Continue),
        }
    }

    fn execute_wait_action(&mut self, kind: &StatementKind) -> Result<BlockControlFlow, String> {
        let delay_ms = match kind {
            StatementKind::WaitFixed { delay_ms } => *delay_ms,
            StatementKind::WaitRandomUniform {
                min_delay_ms,
                max_delay_ms,
            } => sample_uniform_delay_ms(*min_delay_ms, *max_delay_ms),
            StatementKind::WaitRandomGaussian {
                base_delay_ms,
                min_offset_ms,
                max_offset_ms,
                mean_offset_ms,
                std_dev_ms,
            } => {
                let sampled_ms = sample_clamped_gaussian_delay_ms(
                    *base_delay_ms,
                    *min_offset_ms,
                    *max_offset_ms,
                    *mean_offset_ms,
                    *std_dev_ms,
                )?;
                sampled_ms.round().max(0.0) as u64
            }
            _ => return Ok(BlockControlFlow::Continue),
        };
        self.hooks.report_delay_sampled_ms(delay_ms);
        let abort_flag = self.abort_flag;
        let completed = wait_interruptible_ms(delay_ms, &|| abort_flag.load(Ordering::SeqCst));
        Ok(flow_from_completed(completed))
    }

    fn execute_set_variable(
        &mut self,
        variable_name: &str,
        value: f64,
    ) -> Result<BlockControlFlow, String> {
        if !value.is_finite() {
            return Err(format!(
                "variable '{variable_name}' received a non-finite value"
            ));
        }
        self.ensure_variable_capacity(variable_name)?;
        self.variables.insert(variable_name.to_string(), value);
        Ok(BlockControlFlow::Continue)
    }

    fn execute_change_variable(
        &mut self,
        variable_name: &str,
        delta_value: f64,
    ) -> Result<BlockControlFlow, String> {
        if !delta_value.is_finite() {
            return Err(format!(
                "variable '{variable_name}' delta must be a finite number"
            ));
        }
        self.ensure_variable_capacity(variable_name)?;
        let current_value = self.variables.get(variable_name).copied().unwrap_or(0.0);
        let updated_value = current_value + delta_value;
        if !updated_value.is_finite() {
            return Err(format!(
                "variable '{variable_name}' overflowed to a non-finite value"
            ));
        }
        self.variables
            .insert(variable_name.to_string(), updated_value);
        Ok(BlockControlFlow::Continue)
    }

    fn ensure_variable_capacity(&self, variable_name: &str) -> Result<(), String> {
        if self.variables.contains_key(variable_name) {
            return Ok(());
        }
        if self.variables.len() >= MAX_SCRIPT_VARIABLES {
            return Err(format!(
                "script variable limit of {MAX_SCRIPT_VARIABLES} exceeded"
            ));
        }
        Ok(())
    }

    fn evaluate_condition(&self, condition: &ConditionExpr) -> bool {
        match condition {
            ConditionExpr::IsInputPressed { input } => {
                self.physical_input.is_trigger_pressed(*input)
            }
            ConditionExpr::RandomChance { chance_percent } => {
                if !chance_percent.is_finite() || *chance_percent <= 0.0 {
                    return false;
                }
                if *chance_percent >= 100.0 {
                    return true;
                }
                let rolled_percent: f64 = rand::rng().random_range(0.0_f64..100.0_f64);
                rolled_percent < *chance_percent
            }
            ConditionExpr::CompareVariable {
                variable_name,
                operator,
                target_value,
            } => {
                let current_value = self.variables.get(variable_name).copied().unwrap_or(0.0);
                operator.evaluate(current_value, *target_value)
            }
            ConditionExpr::Not { inner_condition } => !self.evaluate_condition(inner_condition),
            ConditionExpr::And {
                left_condition,
                right_condition,
            } => {
                self.evaluate_condition(left_condition) && self.evaluate_condition(right_condition)
            }
            ConditionExpr::Or {
                left_condition,
                right_condition,
            } => {
                self.evaluate_condition(left_condition) || self.evaluate_condition(right_condition)
            }
        }
    }

    fn wait_idle_slice(&self) {
        let abort_flag = self.abort_flag;
        let _ = wait_interruptible_ms(TRIGGER_IDLE_POLL_MS, &|| abort_flag.load(Ordering::SeqCst));
    }
}

/// Executes a Visual Block AST program directly until `abort_flag` trips or `StopScript` runs.
///
/// Guarantees `input_sink.release_all_pressed()` runs on every return path via RAII guard.
pub fn execute_visual_program(
    program: &VisualProgram,
    input_sink: Arc<dyn InputSink>,
    physical_input: Arc<dyn PhysicalInputReader>,
    abort_flag: &AtomicBool,
    hooks: &mut BlockExecutionHooks,
) -> Result<ProgramStopReason, String> {
    let _release_guard = InputReleaseGuard {
        input_sink: Arc::clone(&input_sink),
    };
    let mut state = InterpreterState {
        input_sink,
        physical_input,
        abort_flag,
        hooks,
        variables: HashMap::new(),
        last_active_block_id: None,
    };

    let outcome = match &program.trigger {
        TriggerBlock::WhileInputHeld {
            trigger_input,
            additional_held_inputs,
        } => run_while_input_held(
            &mut state,
            *trigger_input,
            additional_held_inputs,
            &program.statements,
        ),
        TriggerBlock::OnInputToggle {
            trigger_input,
            additional_held_inputs,
        } => run_on_input_toggle(
            &mut state,
            *trigger_input,
            additional_held_inputs,
            &program.statements,
        ),
        TriggerBlock::OnInputPressed {
            trigger_input,
            additional_held_inputs,
        } => run_on_input_pressed(
            &mut state,
            *trigger_input,
            additional_held_inputs,
            &program.statements,
        ),
        TriggerBlock::LoopWhileActive => run_loop_while_active(&mut state, &program.statements),
    };
    state.set_active_block(None);
    outcome
}

fn are_all_inputs_pressed(
    physical_input: &dyn PhysicalInputReader,
    trigger_input: InputTrigger,
    additional_held_inputs: &[InputTrigger],
) -> bool {
    if !physical_input.is_trigger_pressed(trigger_input) {
        return false;
    }
    additional_held_inputs
        .iter()
        .all(|required_input| physical_input.is_trigger_pressed(*required_input))
}

fn run_while_input_held(
    state: &mut InterpreterState<'_>,
    trigger_input: InputTrigger,
    additional_held_inputs: &[InputTrigger],
    statements: &[StatementBlock],
) -> Result<ProgramStopReason, String> {
    while !state.is_aborted() {
        if !are_all_inputs_pressed(
            state.physical_input.as_ref(),
            trigger_input,
            additional_held_inputs,
        ) {
            state.set_active_block(None);
            state.wait_idle_slice();
            continue;
        }
        let iteration_started_at = Instant::now();
        match state.run_statement_slice(statements)? {
            BlockControlFlow::Continue => enforce_min_loop_yield(iteration_started_at),
            BlockControlFlow::BreakLoop => {
                state.set_active_block(None);
                state.wait_idle_slice();
            }
            BlockControlFlow::StopScript => return Ok(ProgramStopReason::StoppedByBlock),
            BlockControlFlow::Aborted => return Ok(ProgramStopReason::Aborted),
        }
    }
    Ok(ProgramStopReason::Aborted)
}

fn run_on_input_toggle(
    state: &mut InterpreterState<'_>,
    trigger_input: InputTrigger,
    additional_held_inputs: &[InputTrigger],
    statements: &[StatementBlock],
) -> Result<ProgramStopReason, String> {
    let mut is_toggle_active = false;
    let mut was_trigger_pressed = false;

    while !state.is_aborted() {
        let is_trigger_pressed = are_all_inputs_pressed(
            state.physical_input.as_ref(),
            trigger_input,
            additional_held_inputs,
        );
        if is_trigger_pressed && !was_trigger_pressed {
            is_toggle_active = !is_toggle_active;
        }
        was_trigger_pressed = is_trigger_pressed;

        if !is_toggle_active {
            state.set_active_block(None);
            state.wait_idle_slice();
            continue;
        }
        let iteration_started_at = Instant::now();
        match state.run_statement_slice(statements)? {
            BlockControlFlow::Continue => enforce_min_loop_yield(iteration_started_at),
            BlockControlFlow::BreakLoop => {
                is_toggle_active = false;
                state.set_active_block(None);
            }
            BlockControlFlow::StopScript => return Ok(ProgramStopReason::StoppedByBlock),
            BlockControlFlow::Aborted => return Ok(ProgramStopReason::Aborted),
        }
    }
    Ok(ProgramStopReason::Aborted)
}

fn run_on_input_pressed(
    state: &mut InterpreterState<'_>,
    trigger_input: InputTrigger,
    additional_held_inputs: &[InputTrigger],
    statements: &[StatementBlock],
) -> Result<ProgramStopReason, String> {
    let mut was_trigger_pressed = false;

    while !state.is_aborted() {
        let is_trigger_pressed = are_all_inputs_pressed(
            state.physical_input.as_ref(),
            trigger_input,
            additional_held_inputs,
        );
        if !is_trigger_pressed || was_trigger_pressed {
            was_trigger_pressed = is_trigger_pressed;
            state.set_active_block(None);
            state.wait_idle_slice();
            continue;
        }
        was_trigger_pressed = true;
        match state.run_statement_slice(statements)? {
            BlockControlFlow::Continue | BlockControlFlow::BreakLoop => {
                state.set_active_block(None);
            }
            BlockControlFlow::StopScript => return Ok(ProgramStopReason::StoppedByBlock),
            BlockControlFlow::Aborted => return Ok(ProgramStopReason::Aborted),
        }
    }
    Ok(ProgramStopReason::Aborted)
}

fn run_loop_while_active(
    state: &mut InterpreterState<'_>,
    statements: &[StatementBlock],
) -> Result<ProgramStopReason, String> {
    while !state.is_aborted() {
        let iteration_started_at = Instant::now();
        match state.run_statement_slice(statements)? {
            BlockControlFlow::Continue => enforce_min_loop_yield(iteration_started_at),
            BlockControlFlow::BreakLoop | BlockControlFlow::StopScript => {
                return Ok(ProgramStopReason::StoppedByBlock);
            }
            BlockControlFlow::Aborted => return Ok(ProgramStopReason::Aborted),
        }
    }
    Ok(ProgramStopReason::Aborted)
}

fn flow_from_completed(did_complete: bool) -> BlockControlFlow {
    if did_complete {
        BlockControlFlow::Continue
    } else {
        BlockControlFlow::Aborted
    }
}

fn enforce_min_loop_yield(iteration_started_at: Instant) {
    let minimum_duration = Duration::from_micros(MIN_LOOP_YIELD_MICROS);
    let elapsed_duration = iteration_started_at.elapsed();
    if elapsed_duration < minimum_duration {
        std::thread::sleep(minimum_duration.saturating_sub(elapsed_duration));
    }
}

#[cfg(test)]
#[path = "block_interpreter_tests.rs"]
mod tests;
