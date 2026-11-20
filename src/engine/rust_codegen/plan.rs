use std::collections::{HashMap, HashSet};

use clickforge_sdk::InputTrigger;

use crate::models::{
    ConditionExpr, CustomBlockDefinition, MAX_CUSTOM_BLOCK_CALL_DEPTH, StatementBlock,
    StatementKind, VisualProgram, lookup_custom_block,
};

const MAX_CUSTOM_TILE_SLUG_CHARS: usize = 32;

#[derive(Default)]
pub(super) struct ProgramFeatureFlags {
    pub(super) uses_mouse_button: bool,
    pub(super) uses_key_code: bool,
    pub(super) uses_gaussian_range: bool,
    pub(super) uses_variables: bool,
    pub(super) mutates_variables: bool,
    pub(super) uses_random_chance: bool,
}

pub(super) struct ResolvedCustomTile<'library> {
    pub(super) definition: &'library CustomBlockDefinition,
    pub(super) function_name: String,
    pub(super) uses_variables: bool,
    pub(super) mutates_variables: bool,
}

pub(super) struct CustomCodegenPlan<'library> {
    pub(super) ordered_tiles: Vec<ResolvedCustomTile<'library>>,
    index_by_id: HashMap<&'library str, usize>,
}

impl<'library> CustomCodegenPlan<'library> {
    pub(super) fn build(
        program: &VisualProgram,
        custom_blocks: &'library [CustomBlockDefinition],
    ) -> Self {
        let mut plan = Self {
            ordered_tiles: Vec::new(),
            index_by_id: HashMap::new(),
        };
        let mut used_function_names = HashSet::new();
        let mut active_stack: Vec<&'library str> = Vec::with_capacity(MAX_CUSTOM_BLOCK_CALL_DEPTH);

        plan.collect_from_slice(
            &program.statements,
            custom_blocks,
            &mut used_function_names,
            &mut active_stack,
        );
        plan.propagate_variable_usage();
        plan
    }

    pub(super) fn lookup_tile(
        &self,
        custom_block_id: &str,
    ) -> Option<&ResolvedCustomTile<'library>> {
        self.index_by_id
            .get(custom_block_id)
            .and_then(|tile_index| self.ordered_tiles.get(*tile_index))
    }

    fn collect_from_slice(
        &mut self,
        statements: &[StatementBlock],
        custom_blocks: &'library [CustomBlockDefinition],
        used_function_names: &mut HashSet<String>,
        active_stack: &mut Vec<&'library str>,
    ) {
        for block in statements {
            match &block.kind {
                StatementKind::IfThen { then_blocks, .. } => {
                    self.collect_from_slice(
                        then_blocks,
                        custom_blocks,
                        used_function_names,
                        active_stack,
                    );
                }
                StatementKind::IfThenElse {
                    then_blocks,
                    else_blocks,
                    ..
                } => {
                    self.collect_from_slice(
                        then_blocks,
                        custom_blocks,
                        used_function_names,
                        active_stack,
                    );
                    self.collect_from_slice(
                        else_blocks,
                        custom_blocks,
                        used_function_names,
                        active_stack,
                    );
                }
                StatementKind::WhileCondition { body_blocks, .. }
                | StatementKind::RepeatCount { body_blocks, .. } => {
                    self.collect_from_slice(
                        body_blocks,
                        custom_blocks,
                        used_function_names,
                        active_stack,
                    );
                }
                StatementKind::RunCustomBlock {
                    custom_block_id, ..
                } => self.register_tile_transitively(
                    custom_block_id,
                    custom_blocks,
                    used_function_names,
                    active_stack,
                ),
                _ => {}
            }
        }
    }

    fn register_tile_transitively(
        &mut self,
        custom_block_id: &str,
        custom_blocks: &'library [CustomBlockDefinition],
        used_function_names: &mut HashSet<String>,
        active_stack: &mut Vec<&'library str>,
    ) {
        if active_stack.len() >= MAX_CUSTOM_BLOCK_CALL_DEPTH {
            return;
        }
        let Some(definition) = lookup_custom_block(custom_blocks, custom_block_id) else {
            return;
        };
        if active_stack.contains(&definition.id.as_str())
            || self.index_by_id.contains_key(definition.id.as_str())
        {
            return;
        }

        let mut direct_flags = ProgramFeatureFlags::default();
        inspect_statement_slice_features(&definition.statements, &mut direct_flags);
        let function_name =
            allocate_unique_tile_function_name(&definition.name, used_function_names);
        let tile_index = self.ordered_tiles.len();
        self.index_by_id.insert(definition.id.as_str(), tile_index);
        self.ordered_tiles.push(ResolvedCustomTile {
            definition,
            function_name,
            uses_variables: direct_flags.uses_variables,
            mutates_variables: direct_flags.mutates_variables,
        });

        active_stack.push(definition.id.as_str());
        self.collect_from_slice(
            &definition.statements,
            custom_blocks,
            used_function_names,
            active_stack,
        );
        active_stack.pop();
    }

    fn propagate_variable_usage(&mut self) {
        let max_passes = self.ordered_tiles.len();
        for _pass_index in 0..max_passes {
            let mut did_change = false;
            for tile_index in 0..self.ordered_tiles.len() {
                let referenced_ids = self.ordered_tiles[tile_index]
                    .definition
                    .referenced_custom_block_ids();
                let mut child_uses_variables = false;
                let mut child_mutates_variables = false;
                for referenced_id in referenced_ids {
                    if let Some(callee) = self.lookup_tile(&referenced_id) {
                        child_uses_variables |= callee.uses_variables;
                        child_mutates_variables |= callee.mutates_variables;
                    }
                }
                let current_tile = &mut self.ordered_tiles[tile_index];
                if child_uses_variables && !current_tile.uses_variables {
                    current_tile.uses_variables = true;
                    did_change = true;
                }
                if child_mutates_variables && !current_tile.mutates_variables {
                    current_tile.mutates_variables = true;
                    did_change = true;
                }
            }
            if !did_change {
                break;
            }
        }
    }
}

pub(super) fn inspect_program_features(
    program: &VisualProgram,
    custom_plan: &CustomCodegenPlan<'_>,
) -> ProgramFeatureFlags {
    let mut flags = ProgramFeatureFlags::default();
    for trigger_input in program.trigger.all_trigger_inputs() {
        record_trigger_feature(trigger_input, &mut flags);
    }
    inspect_statement_slice_features(&program.statements, &mut flags);
    for resolved_tile in &custom_plan.ordered_tiles {
        inspect_statement_slice_features(&resolved_tile.definition.statements, &mut flags);
    }
    flags
}

fn inspect_statement_slice_features(
    statements: &[StatementBlock],
    flags: &mut ProgramFeatureFlags,
) {
    for block in statements {
        match &block.kind {
            StatementKind::IfThen {
                condition,
                then_blocks,
            } => {
                inspect_condition_features(condition, flags);
                inspect_statement_slice_features(then_blocks, flags);
            }
            StatementKind::IfThenElse {
                condition,
                then_blocks,
                else_blocks,
            } => {
                inspect_condition_features(condition, flags);
                inspect_statement_slice_features(then_blocks, flags);
                inspect_statement_slice_features(else_blocks, flags);
            }
            StatementKind::WhileCondition {
                condition,
                body_blocks,
            } => {
                inspect_condition_features(condition, flags);
                inspect_statement_slice_features(body_blocks, flags);
            }
            StatementKind::RepeatCount { body_blocks, .. } => {
                inspect_statement_slice_features(body_blocks, flags);
            }
            StatementKind::MouseClick { .. }
            | StatementKind::MouseDown { .. }
            | StatementKind::MouseUp { .. } => flags.uses_mouse_button = true,
            StatementKind::MouseHoldWhilePressed { watch_input, .. } => {
                flags.uses_mouse_button = true;
                record_trigger_feature(*watch_input, flags);
            }
            StatementKind::KeyTap { .. }
            | StatementKind::KeyDown { .. }
            | StatementKind::KeyUp { .. } => flags.uses_key_code = true,
            StatementKind::WaitRandomGaussian { .. } => flags.uses_gaussian_range = true,
            StatementKind::SetVariable { .. } | StatementKind::ChangeVariable { .. } => {
                flags.uses_variables = true;
                flags.mutates_variables = true;
            }
            StatementKind::BreakLoop
            | StatementKind::StopScript
            | StatementKind::DoNothing
            | StatementKind::MouseMoveRelative { .. }
            | StatementKind::MouseScroll { .. }
            | StatementKind::TypeText { .. }
            | StatementKind::WaitFixed { .. }
            | StatementKind::WaitRandomUniform { .. }
            | StatementKind::RunCustomBlock { .. } => {}
        }
    }
}

fn inspect_condition_features(condition: &ConditionExpr, flags: &mut ProgramFeatureFlags) {
    match condition {
        ConditionExpr::IsInputPressed { input } => record_trigger_feature(*input, flags),
        ConditionExpr::RandomChance { .. } => flags.uses_random_chance = true,
        ConditionExpr::CompareVariable { .. } => flags.uses_variables = true,
        ConditionExpr::Not { inner_condition } => {
            inspect_condition_features(inner_condition, flags);
        }
        ConditionExpr::And {
            left_condition,
            right_condition,
        }
        | ConditionExpr::Or {
            left_condition,
            right_condition,
        } => {
            inspect_condition_features(left_condition, flags);
            inspect_condition_features(right_condition, flags);
        }
    }
}

fn record_trigger_feature(trigger: InputTrigger, flags: &mut ProgramFeatureFlags) {
    match trigger {
        InputTrigger::Mouse(_) => flags.uses_mouse_button = true,
        InputTrigger::Key(_) => flags.uses_key_code = true,
    }
}

pub(super) fn slice_contains_wait_action(
    statements: &[StatementBlock],
    custom_plan: &CustomCodegenPlan<'_>,
    call_depth: usize,
) -> bool {
    if call_depth > MAX_CUSTOM_BLOCK_CALL_DEPTH {
        return false;
    }
    statements.iter().any(|block| match &block.kind {
        StatementKind::WaitFixed { .. }
        | StatementKind::WaitRandomUniform { .. }
        | StatementKind::WaitRandomGaussian { .. } => true,
        StatementKind::IfThen { then_blocks, .. } => {
            slice_contains_wait_action(then_blocks, custom_plan, call_depth)
        }
        StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } => {
            slice_contains_wait_action(then_blocks, custom_plan, call_depth)
                || slice_contains_wait_action(else_blocks, custom_plan, call_depth)
        }
        StatementKind::WhileCondition { body_blocks, .. }
        | StatementKind::RepeatCount { body_blocks, .. } => {
            slice_contains_wait_action(body_blocks, custom_plan, call_depth)
        }
        StatementKind::RunCustomBlock {
            custom_block_id, ..
        } => custom_plan
            .lookup_tile(custom_block_id)
            .is_some_and(|tile| {
                slice_contains_wait_action(
                    &tile.definition.statements,
                    custom_plan,
                    call_depth.saturating_add(1),
                )
            }),
        _ => false,
    })
}

pub(super) fn slice_ends_with_terminator(statements: &[StatementBlock]) -> bool {
    matches!(
        statements.last().map(|block| &block.kind),
        Some(StatementKind::BreakLoop | StatementKind::StopScript)
    )
}

fn allocate_unique_tile_function_name(
    tile_name: &str,
    used_function_names: &mut HashSet<String>,
) -> String {
    let base_slug = sanitize_rust_identifier_slug(tile_name);
    let base_name = format!("run_custom_tile_{base_slug}");
    if used_function_names.insert(base_name.clone()) {
        return base_name;
    }
    let mut suffix_number: usize = 2;
    loop {
        let candidate_name = format!("{base_name}_{suffix_number}");
        if used_function_names.insert(candidate_name.clone()) {
            return candidate_name;
        }
        suffix_number = suffix_number.saturating_add(1);
    }
}

fn sanitize_rust_identifier_slug(raw_name: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_underscore = false;

    for character in raw_name.chars() {
        if slug.len() >= MAX_CUSTOM_TILE_SLUG_CHARS {
            break;
        }
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_was_underscore = false;
        } else if !slug.is_empty() && !previous_was_underscore {
            slug.push('_');
            previous_was_underscore = true;
        }
    }

    let trimmed = slug.trim_matches('_');
    if trimmed.is_empty() {
        "tile".to_string()
    } else {
        trimmed.to_string()
    }
}
