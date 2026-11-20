use clickforge_sdk::{KeyCode, MouseButton};

use crate::models::blocks::{BlockId, BlockSlotTarget, StatementBlock, StatementKind};

pub(crate) fn child_slices(kind: &StatementKind) -> (&[StatementBlock], &[StatementBlock]) {
    match kind {
        StatementKind::IfThen { then_blocks, .. } => (then_blocks, &[]),
        StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } => (then_blocks, else_blocks),
        StatementKind::WhileCondition { body_blocks, .. }
        | StatementKind::RepeatCount { body_blocks, .. } => (body_blocks, &[]),
        _ => (&[], &[]),
    }
}

pub(crate) fn child_slices_mut(
    kind: &mut StatementKind,
) -> (
    Option<&mut Vec<StatementBlock>>,
    Option<&mut Vec<StatementBlock>>,
) {
    match kind {
        StatementKind::IfThen { then_blocks, .. } => (Some(then_blocks), None),
        StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } => (Some(then_blocks), Some(else_blocks)),
        StatementKind::WhileCondition { body_blocks, .. }
        | StatementKind::RepeatCount { body_blocks, .. } => (Some(body_blocks), None),
        _ => (None, None),
    }
}

pub(crate) fn slot_parent_block_id(target_slot: BlockSlotTarget) -> Option<BlockId> {
    match target_slot {
        BlockSlotTarget::Root { .. } => None,
        BlockSlotTarget::ThenBranch {
            parent_block_id, ..
        }
        | BlockSlotTarget::ElseBranch {
            parent_block_id, ..
        }
        | BlockSlotTarget::LoopBody {
            parent_block_id, ..
        } => Some(parent_block_id),
    }
}

pub(crate) fn adjust_slot_for_same_parent_move(
    root_statements: &[StatementBlock],
    source_block_id: BlockId,
    target_slot: BlockSlotTarget,
) -> BlockSlotTarget {
    let Some(target_slice) = resolve_target_slice(root_statements, target_slot) else {
        return target_slot;
    };
    let Some(current_index) = target_slice
        .iter()
        .position(|block| block.id == source_block_id)
    else {
        return target_slot;
    };
    decrement_slot_if_after(target_slot, current_index)
}

fn decrement_slot_if_after(target_slot: BlockSlotTarget, removed_index: usize) -> BlockSlotTarget {
    match target_slot {
        BlockSlotTarget::Root { insert_index } if removed_index < insert_index => {
            BlockSlotTarget::Root {
                insert_index: insert_index.saturating_sub(1),
            }
        }
        BlockSlotTarget::ThenBranch {
            parent_block_id,
            insert_index,
        } if removed_index < insert_index => BlockSlotTarget::ThenBranch {
            parent_block_id,
            insert_index: insert_index.saturating_sub(1),
        },
        BlockSlotTarget::ElseBranch {
            parent_block_id,
            insert_index,
        } if removed_index < insert_index => BlockSlotTarget::ElseBranch {
            parent_block_id,
            insert_index: insert_index.saturating_sub(1),
        },
        BlockSlotTarget::LoopBody {
            parent_block_id,
            insert_index,
        } if removed_index < insert_index => BlockSlotTarget::LoopBody {
            parent_block_id,
            insert_index: insert_index.saturating_sub(1),
        },
        _ => target_slot,
    }
}

pub(crate) fn resolve_target_slice(
    root_statements: &[StatementBlock],
    target_slot: BlockSlotTarget,
) -> Option<&[StatementBlock]> {
    match target_slot {
        BlockSlotTarget::Root { .. } => Some(root_statements),
        BlockSlotTarget::ThenBranch {
            parent_block_id, ..
        } => {
            let parent_block = find_in_slice(root_statements, parent_block_id)?;
            match &parent_block.kind {
                StatementKind::IfThen { then_blocks, .. }
                | StatementKind::IfThenElse { then_blocks, .. } => Some(then_blocks),
                _ => None,
            }
        }
        BlockSlotTarget::ElseBranch {
            parent_block_id, ..
        } => {
            let parent_block = find_in_slice(root_statements, parent_block_id)?;
            match &parent_block.kind {
                StatementKind::IfThenElse { else_blocks, .. } => Some(else_blocks),
                _ => None,
            }
        }
        BlockSlotTarget::LoopBody {
            parent_block_id, ..
        } => {
            let parent_block = find_in_slice(root_statements, parent_block_id)?;
            match &parent_block.kind {
                StatementKind::WhileCondition { body_blocks, .. }
                | StatementKind::RepeatCount { body_blocks, .. } => Some(body_blocks),
                _ => None,
            }
        }
    }
}

pub(crate) fn resolve_target_slice_mut(
    root_statements: &mut Vec<StatementBlock>,
    target_slot: BlockSlotTarget,
) -> Option<(&mut Vec<StatementBlock>, usize)> {
    match target_slot {
        BlockSlotTarget::Root { insert_index } => Some((root_statements, insert_index)),
        BlockSlotTarget::ThenBranch {
            parent_block_id,
            insert_index,
        } => {
            let parent_block = find_in_slice_mut(root_statements, parent_block_id)?;
            match &mut parent_block.kind {
                StatementKind::IfThen { then_blocks, .. }
                | StatementKind::IfThenElse { then_blocks, .. } => {
                    Some((then_blocks, insert_index))
                }
                _ => None,
            }
        }
        BlockSlotTarget::ElseBranch {
            parent_block_id,
            insert_index,
        } => {
            let parent_block = find_in_slice_mut(root_statements, parent_block_id)?;
            match &mut parent_block.kind {
                StatementKind::IfThenElse { else_blocks, .. } => Some((else_blocks, insert_index)),
                _ => None,
            }
        }
        BlockSlotTarget::LoopBody {
            parent_block_id,
            insert_index,
        } => {
            let parent_block = find_in_slice_mut(root_statements, parent_block_id)?;
            match &mut parent_block.kind {
                StatementKind::WhileCondition { body_blocks, .. }
                | StatementKind::RepeatCount { body_blocks, .. } => {
                    Some((body_blocks, insert_index))
                }
                _ => None,
            }
        }
    }
}

pub(crate) fn block_subtree_contains_id(block: &StatementBlock, target_block_id: BlockId) -> bool {
    if block.id == target_block_id {
        return true;
    }
    let (primary_children, secondary_children) = child_slices(&block.kind);
    find_in_slice(primary_children, target_block_id).is_some()
        || find_in_slice(secondary_children, target_block_id).is_some()
}

pub(crate) fn find_in_slice(
    statements: &[StatementBlock],
    target_block_id: BlockId,
) -> Option<&StatementBlock> {
    for block in statements {
        if block.id == target_block_id {
            return Some(block);
        }
        let (primary_children, secondary_children) = child_slices(&block.kind);
        if let Some(found) = find_in_slice(primary_children, target_block_id) {
            return Some(found);
        }
        if let Some(found) = find_in_slice(secondary_children, target_block_id) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn find_in_slice_mut(
    statements: &mut [StatementBlock],
    target_block_id: BlockId,
) -> Option<&mut StatementBlock> {
    for block in statements {
        if block.id == target_block_id {
            return Some(block);
        }
        let (primary_children, secondary_children) = child_slices_mut(&mut block.kind);
        if let Some(children) = primary_children
            && let Some(found) = find_in_slice_mut(children, target_block_id)
        {
            return Some(found);
        }
        if let Some(children) = secondary_children
            && let Some(found) = find_in_slice_mut(children, target_block_id)
        {
            return Some(found);
        }
    }
    None
}

pub(crate) fn remove_from_slice(
    statements: &mut Vec<StatementBlock>,
    target_block_id: BlockId,
) -> Option<StatementBlock> {
    if let Some(position) = statements
        .iter()
        .position(|block| block.id == target_block_id)
    {
        return Some(statements.remove(position));
    }
    for block in statements {
        let (primary_children, secondary_children) = child_slices_mut(&mut block.kind);
        if let Some(children) = primary_children
            && let Some(removed) = remove_from_slice(children, target_block_id)
        {
            return Some(removed);
        }
        if let Some(children) = secondary_children
            && let Some(removed) = remove_from_slice(children, target_block_id)
        {
            return Some(removed);
        }
    }
    None
}

pub(crate) fn count_blocks_in_slice(statements: &[StatementBlock]) -> usize {
    let mut total_count: usize = 0;
    for block in statements {
        let (primary_children, secondary_children) = child_slices(&block.kind);
        total_count = total_count
            .saturating_add(1)
            .saturating_add(count_blocks_in_slice(primary_children))
            .saturating_add(count_blocks_in_slice(secondary_children));
    }
    total_count
}

pub(crate) fn max_depth_in_slice(statements: &[StatementBlock], current_depth: usize) -> usize {
    let mut deepest = current_depth;
    for block in statements {
        let (primary_children, secondary_children) = child_slices(&block.kind);
        if primary_children.is_empty() && secondary_children.is_empty() {
            continue;
        }
        let next_depth = current_depth.saturating_add(1);
        deepest = deepest
            .max(max_depth_in_slice(primary_children, next_depth))
            .max(max_depth_in_slice(secondary_children, next_depth));
    }
    deepest
}

pub(crate) fn collect_emitted_keys_in_slice(
    statements: &[StatementBlock],
    collected_keys: &mut Vec<KeyCode>,
) {
    for block in statements {
        collect_emitted_keys_for_kind(&block.kind, collected_keys);
        let (primary_children, secondary_children) = child_slices(&block.kind);
        collect_emitted_keys_in_slice(primary_children, collected_keys);
        collect_emitted_keys_in_slice(secondary_children, collected_keys);
    }
}

fn collect_emitted_keys_for_kind(kind: &StatementKind, collected_keys: &mut Vec<KeyCode>) {
    match kind {
        StatementKind::KeyTap { key_code, .. }
        | StatementKind::KeyDown { key_code }
        | StatementKind::KeyUp { key_code } => push_unique_key(collected_keys, *key_code),
        StatementKind::TypeText { text, .. } => collect_type_text_keys(text, collected_keys),
        _ => {}
    }
}

fn collect_type_text_keys(text: &str, collected_keys: &mut Vec<KeyCode>) {
    for character in text.chars() {
        let Some((key_code, requires_shift)) = KeyCode::ascii_char_to_key(character) else {
            continue;
        };
        push_unique_key(collected_keys, key_code);
        if requires_shift {
            push_unique_key(collected_keys, KeyCode::LeftShift);
        }
    }
}

pub(crate) fn collect_condition_keys_in_slice(
    statements: &[StatementBlock],
    collected_keys: &mut Vec<KeyCode>,
) {
    for block in statements {
        if let StatementKind::IfThen { condition, .. }
        | StatementKind::IfThenElse { condition, .. }
        | StatementKind::WhileCondition { condition, .. } = &block.kind
        {
            condition.collect_referenced_keys(collected_keys);
        }
        let (primary_children, secondary_children) = child_slices(&block.kind);
        collect_condition_keys_in_slice(primary_children, collected_keys);
        collect_condition_keys_in_slice(secondary_children, collected_keys);
    }
}

pub(crate) fn collect_emitted_buttons_in_slice(
    statements: &[StatementBlock],
    collected_buttons: &mut Vec<MouseButton>,
) {
    for block in statements {
        if let StatementKind::MouseClick { mouse_button, .. }
        | StatementKind::MouseDown { mouse_button }
        | StatementKind::MouseUp { mouse_button } = &block.kind
            && !collected_buttons.contains(mouse_button)
        {
            collected_buttons.push(*mouse_button);
        }
        let (primary_children, secondary_children) = child_slices(&block.kind);
        collect_emitted_buttons_in_slice(primary_children, collected_buttons);
        collect_emitted_buttons_in_slice(secondary_children, collected_buttons);
    }
}

pub(crate) fn push_unique_key(collected_keys: &mut Vec<KeyCode>, key_code: KeyCode) {
    if !collected_keys.contains(&key_code) {
        collected_keys.push(key_code);
    }
}
