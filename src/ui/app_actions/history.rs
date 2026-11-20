use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::models::VisualProgram;
use crate::ui::app::ClickForgeWorkbenchApp;

const MAX_UNDO_SNAPSHOTS_PER_SCRIPT: usize = 40;
const MAX_TRACKED_HISTORY_SCRIPTS: usize = 64;
const UNDO_COALESCE_WINDOW_MS: u64 = 450;

/// Bounded per-script undo/redo snapshot stack for Visual Block editing.
#[derive(Debug, Default)]
pub(crate) struct VisualProgramHistory {
    undo_stack: VecDeque<VisualProgram>,
    redo_stack: VecDeque<VisualProgram>,
    last_push_at: Option<Instant>,
    last_block_count: usize,
}

impl VisualProgramHistory {
    pub(crate) fn record_snapshot(
        &mut self,
        previous_program: VisualProgram,
        current_block_count: usize,
        is_pointer_down: bool,
    ) {
        let should_coalesce_drag = is_pointer_down
            && self.last_block_count == current_block_count
            && self.last_push_at.is_some_and(|pushed_at| {
                pushed_at.elapsed() < Duration::from_millis(UNDO_COALESCE_WINDOW_MS)
            });
        if should_coalesce_drag {
            return;
        }
        if self
            .undo_stack
            .back()
            .is_some_and(|latest| *latest == previous_program)
        {
            return;
        }
        while self.undo_stack.len() >= MAX_UNDO_SNAPSHOTS_PER_SCRIPT {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(previous_program);
        self.redo_stack.clear();
        self.last_push_at = Some(Instant::now());
        self.last_block_count = current_block_count;
    }

    pub(crate) fn undo(&mut self, current_program: VisualProgram) -> Option<VisualProgram> {
        let previous_program = self.undo_stack.pop_back()?;
        while self.redo_stack.len() >= MAX_UNDO_SNAPSHOTS_PER_SCRIPT {
            self.redo_stack.pop_front();
        }
        self.redo_stack.push_back(current_program);
        self.last_push_at = None;
        Some(previous_program)
    }

    pub(crate) fn redo(&mut self, current_program: VisualProgram) -> Option<VisualProgram> {
        let next_program = self.redo_stack.pop_back()?;
        while self.undo_stack.len() >= MAX_UNDO_SNAPSHOTS_PER_SCRIPT {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(current_program);
        self.last_push_at = None;
        Some(next_program)
    }
}

impl ClickForgeWorkbenchApp {
    pub(crate) fn record_visual_program_undo_snapshot(
        &mut self,
        script_id: &str,
        previous_program: VisualProgram,
        current_block_count: usize,
        is_pointer_down: bool,
    ) {
        if self.visual_history_by_script_id.len() >= MAX_TRACKED_HISTORY_SCRIPTS
            && !self.visual_history_by_script_id.contains_key(script_id)
        {
            let active_ids: HashSet<&str> = self
                .scripts
                .iter()
                .map(|script| script.id.as_str())
                .collect();
            self.visual_history_by_script_id
                .retain(|tracked_id, _| active_ids.contains(tracked_id.as_str()));
        }

        self.visual_history_by_script_id
            .entry(script_id.to_string())
            .or_default()
            .record_snapshot(previous_program, current_block_count, is_pointer_down);
    }

    pub(crate) fn undo_active_visual_edit(&mut self) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        let script_id = self.scripts[active_index].id.clone();
        let current_program = self.scripts[active_index].visual_program.clone();
        let Some(restored_program) = self
            .visual_history_by_script_id
            .get_mut(&script_id)
            .and_then(|history| history.undo(current_program))
        else {
            return;
        };
        self.scripts[active_index].visual_program = restored_program;
        self.mark_script_dirty(&script_id);
        self.show_toast("Undid visual block edit.", false);
    }

    pub(crate) fn redo_active_visual_edit(&mut self) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        let script_id = self.scripts[active_index].id.clone();
        let current_program = self.scripts[active_index].visual_program.clone();
        let Some(restored_program) = self
            .visual_history_by_script_id
            .get_mut(&script_id)
            .and_then(|history| history.redo(current_program))
        else {
            return;
        };
        self.scripts[active_index].visual_program = restored_program;
        self.mark_script_dirty(&script_id);
        self.show_toast("Redid visual block edit.", false);
    }
}
