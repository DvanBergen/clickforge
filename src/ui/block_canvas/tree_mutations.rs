use crate::models::{BlockId, StatementBlock, StatementKind, VisualProgram};

/// Clones a statement block subtree while allocating fresh monotonic `BlockId`s from `program`.
pub(super) fn clone_block_with_fresh_ids(
    program: &mut VisualProgram,
    source_block: &StatementBlock,
) -> StatementBlock {
    let cloned_kind = match &source_block.kind {
        StatementKind::IfThen {
            condition,
            then_blocks,
        } => StatementKind::IfThen {
            condition: condition.clone(),
            then_blocks: clone_block_slice_with_fresh_ids(program, then_blocks),
        },
        StatementKind::IfThenElse {
            condition,
            then_blocks,
            else_blocks,
        } => StatementKind::IfThenElse {
            condition: condition.clone(),
            then_blocks: clone_block_slice_with_fresh_ids(program, then_blocks),
            else_blocks: clone_block_slice_with_fresh_ids(program, else_blocks),
        },
        StatementKind::WhileCondition {
            condition,
            body_blocks,
        } => StatementKind::WhileCondition {
            condition: condition.clone(),
            body_blocks: clone_block_slice_with_fresh_ids(program, body_blocks),
        },
        StatementKind::RepeatCount {
            repeat_count,
            body_blocks,
        } => StatementKind::RepeatCount {
            repeat_count: *repeat_count,
            body_blocks: clone_block_slice_with_fresh_ids(program, body_blocks),
        },
        leaf_kind => leaf_kind.clone(),
    };
    program.create_block(cloned_kind)
}

/// Clones a slice of statement blocks recursively with fresh `BlockId`s owned by `program`.
pub(super) fn clone_block_slice_with_fresh_ids(
    program: &mut VisualProgram,
    source_blocks: &[StatementBlock],
) -> Vec<StatementBlock> {
    source_blocks
        .iter()
        .map(|block| clone_block_with_fresh_ids(program, block))
        .collect()
}

/// Replaces `target_block_id` anywhere in `statements` with `replacement_blocks` in place.
pub(super) fn replace_block_with_slice_in_tree(
    statements: &mut Vec<StatementBlock>,
    target_block_id: BlockId,
    replacement_blocks: Vec<StatementBlock>,
) -> bool {
    if let Some(position) = statements
        .iter()
        .position(|block| block.id == target_block_id)
    {
        statements.splice(position..=position, replacement_blocks);
        return true;
    }

    for block in statements {
        match &mut block.kind {
            StatementKind::IfThen { then_blocks, .. }
                if contains_block_id_in_slice(then_blocks, target_block_id) =>
            {
                return replace_block_with_slice_in_tree(
                    then_blocks,
                    target_block_id,
                    replacement_blocks,
                );
            }
            StatementKind::IfThenElse { then_blocks, .. }
                if contains_block_id_in_slice(then_blocks, target_block_id) =>
            {
                return replace_block_with_slice_in_tree(
                    then_blocks,
                    target_block_id,
                    replacement_blocks,
                );
            }
            StatementKind::IfThenElse { else_blocks, .. }
                if contains_block_id_in_slice(else_blocks, target_block_id) =>
            {
                return replace_block_with_slice_in_tree(
                    else_blocks,
                    target_block_id,
                    replacement_blocks,
                );
            }
            StatementKind::WhileCondition { body_blocks, .. }
            | StatementKind::RepeatCount { body_blocks, .. }
                if contains_block_id_in_slice(body_blocks, target_block_id) =>
            {
                return replace_block_with_slice_in_tree(
                    body_blocks,
                    target_block_id,
                    replacement_blocks,
                );
            }
            _ => {}
        }
    }
    false
}

fn contains_block_id_in_slice(statements: &[StatementBlock], target_block_id: BlockId) -> bool {
    for block in statements {
        if block.id == target_block_id {
            return true;
        }
        let is_in_children = match &block.kind {
            StatementKind::IfThen { then_blocks, .. } => {
                contains_block_id_in_slice(then_blocks, target_block_id)
            }
            StatementKind::IfThenElse {
                then_blocks,
                else_blocks,
                ..
            } => {
                contains_block_id_in_slice(then_blocks, target_block_id)
                    || contains_block_id_in_slice(else_blocks, target_block_id)
            }
            StatementKind::WhileCondition { body_blocks, .. }
            | StatementKind::RepeatCount { body_blocks, .. } => {
                contains_block_id_in_slice(body_blocks, target_block_id)
            }
            _ => false,
        };
        if is_in_children {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::{InputTrigger, MouseButton};

    use super::replace_block_with_slice_in_tree;
    use crate::models::{BlockId, ConditionExpr, StatementBlock, StatementKind};

    #[test]
    fn replace_block_with_slice_unpacks_after_earlier_container_and_inside_nested_branch() {
        let mut statements = vec![
            StatementBlock {
                id: BlockId(1),
                kind: StatementKind::IfThen {
                    condition: ConditionExpr::IsInputPressed {
                        input: InputTrigger::Mouse(MouseButton::Right),
                    },
                    then_blocks: vec![StatementBlock {
                        id: BlockId(2),
                        kind: StatementKind::DoNothing,
                    }],
                },
            },
            StatementBlock {
                id: BlockId(3),
                kind: StatementKind::IfThenElse {
                    condition: ConditionExpr::RandomChance {
                        chance_percent: 50.0,
                    },
                    then_blocks: vec![StatementBlock {
                        id: BlockId(4),
                        kind: StatementKind::RunCustomBlock {
                            custom_block_id: "tile-1".to_string(),
                            cached_name: "Burst".to_string(),
                        },
                    }],
                    else_blocks: Vec::new(),
                },
            },
        ];

        let unpacked_blocks = vec![
            StatementBlock {
                id: BlockId(10),
                kind: StatementKind::MouseClick {
                    mouse_button: MouseButton::Left,
                    hold_duration_ms: 12,
                },
            },
            StatementBlock {
                id: BlockId(11),
                kind: StatementKind::WaitFixed { delay_ms: 25 },
            },
        ];

        assert!(replace_block_with_slice_in_tree(
            &mut statements,
            BlockId(4),
            unpacked_blocks
        ));
        if let StatementKind::IfThenElse { then_blocks, .. } = &statements[1].kind {
            assert_eq!(then_blocks.len(), 2);
            assert_eq!(then_blocks[0].id, BlockId(10));
            assert_eq!(then_blocks[1].id, BlockId(11));
        } else {
            panic!("expected IfThenElse container");
        }
    }
}
