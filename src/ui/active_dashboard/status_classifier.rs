use egui::Color32;

use crate::models::{
    ActiveWindowSnapshot, CoordinatorSnapshot, ScriptDocument, ScriptExecutionPhase, ScriptMode,
    ScriptRuntimeStatus, TargetScope, TriggerBlock, ValidationReport,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE, STATUS_DANGER,
    STATUS_DANGER_INK, STATUS_DANGER_SOLID, STATUS_DANGER_TINT, STATUS_INFO_INK, STATUS_RUNNING,
    STATUS_RUNNING_INK, STATUS_RUNNING_TINT, SURFACE_PANEL, TEXT_MUTED, truncate_display_text,
};

pub(crate) const DEFAULT_UINPUT_OFFLINE_HINT: &str = "Unable to open /dev/uinput. Run 'sudo modprobe uinput' or 'sudo scripts/install_udev_rules.sh', then restart ClickForge.";
pub(crate) const MAX_FOCUSED_TITLE_CHARS: usize = 42;
const MAX_ERROR_DETAIL_CHARS: usize = 160;

type StatusPalette = (Color32, Color32, Color32, Color32);

const PALETTE_RUNNING: StatusPalette = (
    STATUS_RUNNING_TINT,
    STATUS_RUNNING_INK,
    STATUS_RUNNING,
    STATUS_RUNNING,
);
const PALETTE_WARNING: StatusPalette = (
    ACCENT_PRIMARY_TINT,
    ACCENT_PRIMARY,
    ACCENT_PRIMARY,
    ACCENT_PRIMARY,
);
const PALETTE_DANGER: StatusPalette = (
    STATUS_DANGER_TINT,
    STATUS_DANGER_INK,
    STATUS_DANGER,
    STATUS_DANGER_SOLID,
);
const PALETTE_INFO: StatusPalette = (
    SURFACE_PANEL,
    STATUS_INFO_INK,
    BORDER_STRONG,
    STATUS_INFO_INK,
);
const PALETTE_OFF: StatusPalette = (BG_CANVAS, TEXT_MUTED, BORDER_SUBTLE, BORDER_SUBTLE);

/// Resolved presentation descriptor for a single script's live execution state.
#[derive(Debug, Clone)]
pub(crate) struct ScriptLiveStatusInfo {
    pub card_label: &'static str,
    pub compact_label: &'static str,
    pub fill_color: Color32,
    pub ink_color: Color32,
    pub border_color: Color32,
    pub bar_color: Color32,
    pub explanation: String,
    pub is_prominent: bool,
}

impl ScriptLiveStatusInfo {
    fn from_palette(
        card_label: &'static str,
        compact_label: &'static str,
        (fill_color, ink_color, border_color, bar_color): StatusPalette,
        explanation: String,
        is_prominent: bool,
    ) -> Self {
        Self {
            card_label,
            compact_label,
            fill_color,
            ink_color,
            border_color,
            bar_color,
            explanation,
            is_prominent,
        }
    }
}

/// Top-level summary displayed inside the Active tab's global engine status hero card.
#[derive(Debug, Clone)]
pub(crate) struct GlobalEngineSummary {
    pub headline: &'static str,
    pub subline: String,
    pub bar_color: Color32,
    pub headline_color: Color32,
    pub border_color: Color32,
    pub enabled_count: usize,
}

/// Classifies a script's live runtime status into labels, palette tokens, and a plain-English reason.
pub(crate) fn classify_script_live_status(
    script: &ScriptDocument,
    validation_report: &ValidationReport,
    snapshot: &CoordinatorSnapshot,
) -> ScriptLiveStatusInfo {
    let runtime_status = snapshot.status_for_script(&script.id);
    let phase = runtime_status
        .map(|status| status.phase)
        .unwrap_or(ScriptExecutionPhase::Disabled);

    if (snapshot.is_emergency_stopped && script.is_enabled)
        || phase == ScriptExecutionPhase::StoppedByKillSwitch
    {
        return ScriptLiveStatusInfo::from_palette(
            "EMERGENCY STOPPED",
            "STOPPED",
            PALETTE_DANGER,
            "Halted by Kill-Switch or STOP ALL. Click Resume Armed Scripts to re-arm.".to_string(),
            true,
        );
    }

    if !validation_report.is_valid() {
        let first_issue = validation_report
            .issues
            .first()
            .map(|issue| issue.message.clone())
            .unwrap_or_else(|| "Complete script setup in Editor before enabling.".to_string());
        return ScriptLiveStatusInfo::from_palette(
            "SETUP NEEDED",
            "SETUP",
            PALETTE_WARNING,
            first_issue,
            false,
        );
    }

    if phase == ScriptExecutionPhase::Compiling {
        return ScriptLiveStatusInfo::from_palette(
            "COMPILING",
            "BUILD",
            PALETTE_INFO,
            "Compiling Advanced Rust script in background build cache...".to_string(),
            true,
        );
    }

    if phase == ScriptExecutionPhase::Error {
        let card_label = if script.mode == ScriptMode::AdvancedRust {
            "COMPILE ERROR"
        } else {
            "ERROR"
        };
        let detail = runtime_status
            .and_then(|status| status.last_error.as_deref())
            .map(|error_text| truncate_display_text(error_text, MAX_ERROR_DETAIL_CHARS))
            .unwrap_or_else(|| {
                "Script stopped with an error. Inspect Telemetry logs for details.".to_string()
            });
        return ScriptLiveStatusInfo::from_palette(
            card_label,
            "ERROR",
            PALETTE_DANGER,
            detail,
            true,
        );
    }

    if !script.is_enabled {
        return ScriptLiveStatusInfo::from_palette(
            "OFF",
            "OFF",
            PALETTE_OFF,
            "Turn ON to arm this script.".to_string(),
            false,
        );
    }

    classify_enabled_script_status(script, runtime_status, phase, snapshot)
}

fn classify_enabled_script_status(
    script: &ScriptDocument,
    runtime_status: Option<&ScriptRuntimeStatus>,
    phase: ScriptExecutionPhase,
    snapshot: &CoordinatorSnapshot,
) -> ScriptLiveStatusInfo {
    match phase {
        ScriptExecutionPhase::Running => {
            let actions_count = runtime_status
                .map(|status| status.total_actions_emitted)
                .unwrap_or(0);
            let explanation = match runtime_status.and_then(|status| status.last_sampled_delay_ms) {
                Some(delay_ms) => format!(
                    "Actively executing ({actions_count} actions sent, last delay {delay_ms} ms)"
                ),
                None => format!("Actively executing ({actions_count} actions sent)"),
            };
            ScriptLiveStatusInfo::from_palette(
                "RUNNING",
                "RUNNING",
                PALETTE_RUNNING,
                explanation,
                true,
            )
        }
        ScriptExecutionPhase::Armed => ScriptLiveStatusInfo::from_palette(
            "ARMED & READY",
            "ARMED",
            PALETTE_RUNNING,
            build_armed_explanation(
                script,
                runtime_status,
                snapshot.input_status.is_uinput_ready,
            ),
            true,
        ),
        ScriptExecutionPhase::WaitingForFocus => ScriptLiveStatusInfo::from_palette(
            "WAITING FOR GAME",
            "WAIT GAME",
            PALETTE_WARNING,
            format!(
                "Paused until '{}' is the focused window.",
                resolve_target_display_name(script)
            ),
            true,
        ),
        ScriptExecutionPhase::Disabled => {
            if let Some(arm_key) = script.arm_toggle_key
                && runtime_status.is_some_and(|status| !status.is_armed_by_toggle)
            {
                return ScriptLiveStatusInfo::from_palette(
                    "ARM TOGGLE OFF",
                    "ARM OFF",
                    PALETTE_WARNING,
                    format!("Press {} to arm this script.", arm_key.display_label()),
                    true,
                );
            }
            if matches!(script.target_scope, TargetScope::SelectedGame(_)) {
                return ScriptLiveStatusInfo::from_palette(
                    "WAITING FOR GAME",
                    "WAIT GAME",
                    PALETTE_WARNING,
                    format!(
                        "Paused until '{}' is the focused window.",
                        resolve_target_display_name(script)
                    ),
                    true,
                );
            }
            ScriptLiveStatusInfo::from_palette(
                "ARMED & READY",
                "ARMED",
                PALETTE_RUNNING,
                build_armed_explanation(
                    script,
                    runtime_status,
                    snapshot.input_status.is_uinput_ready,
                ),
                true,
            )
        }
        ScriptExecutionPhase::Compiling
        | ScriptExecutionPhase::StoppedByKillSwitch
        | ScriptExecutionPhase::Error => ScriptLiveStatusInfo::from_palette(
            "OFF",
            "OFF",
            PALETTE_OFF,
            "Turn ON to arm this script.".to_string(),
            false,
        ),
    }
}

/// Computes the unmistakable top-level engine state displayed in the Active hero card.
pub(crate) fn classify_global_engine_state(
    scripts: &[ScriptDocument],
    snapshot: &CoordinatorSnapshot,
) -> GlobalEngineSummary {
    let enabled_scripts: Vec<&ScriptDocument> =
        scripts.iter().filter(|script| script.is_enabled).collect();
    let enabled_count = enabled_scripts.len();

    if snapshot.is_emergency_stopped {
        return GlobalEngineSummary {
            headline: "EMERGENCY STOPPED",
            subline: format!(
                "All scripts halted and virtual inputs released (stop count: {}). Click Resume Armed Scripts to re-arm {} enabled script(s).",
                snapshot.emergency_stop_count, enabled_count
            ),
            bar_color: STATUS_DANGER_SOLID,
            headline_color: STATUS_DANGER_INK,
            border_color: STATUS_DANGER,
            enabled_count,
        };
    }

    if enabled_count == 0 {
        return GlobalEngineSummary {
            headline: "ALL SCRIPTS OFF",
            subline: "Zero scripts are enabled. Turn ON a script below to arm it.".to_string(),
            bar_color: BORDER_STRONG,
            headline_color: TEXT_MUTED,
            border_color: BORDER_SUBTLE,
            enabled_count,
        };
    }

    let counts = tally_enabled_script_phases(&enabled_scripts, snapshot);
    let focused_label = resolve_focused_window_label(snapshot.focused_window.as_ref());

    if counts.running_count > 0 {
        let suffix = plural_suffix(counts.running_count);
        let subline = match focused_label {
            Some(window_name) => format!(
                "{} script{suffix} actively executing ({} actions sent). Focused window: {window_name}.",
                counts.running_count, counts.total_running_actions
            ),
            None => format!(
                "{} script{suffix} actively executing ({} actions sent).",
                counts.running_count, counts.total_running_actions
            ),
        };
        return GlobalEngineSummary {
            headline: "CLICKING LIVE",
            subline,
            bar_color: STATUS_RUNNING,
            headline_color: STATUS_RUNNING_INK,
            border_color: STATUS_RUNNING,
            enabled_count,
        };
    }

    if counts.armed_count > 0 {
        let suffix = plural_suffix(counts.armed_count);
        let subline = match focused_label {
            Some(window_name) => format!(
                "Focused window: {window_name} (matched {} armed script{suffix}).",
                counts.armed_count
            ),
            None => format!(
                "{} global script{suffix} armed and ready for your trigger input.",
                counts.armed_count
            ),
        };
        return GlobalEngineSummary {
            headline: "ARMED & READY",
            subline,
            bar_color: STATUS_RUNNING,
            headline_color: STATUS_RUNNING_INK,
            border_color: BORDER_STRONG,
            enabled_count,
        };
    }

    if counts.waiting_count > 0 {
        let subline = match focused_label {
            Some(window_name) => format!(
                "Focused window '{window_name}' does not match any enabled game target. Alt-Tab into your game to activate."
            ),
            None => "No game target is focused. Alt-Tab into your game to activate.".to_string(),
        };
        return GlobalEngineSummary {
            headline: "WAITING FOR TARGET GAME",
            subline,
            bar_color: ACCENT_PRIMARY,
            headline_color: ACCENT_PRIMARY,
            border_color: BORDER_STRONG,
            enabled_count,
        };
    }

    if counts.arm_toggle_off_count > 0 {
        return GlobalEngineSummary {
            headline: "ARM TOGGLE OFF",
            subline: "Enabled scripts are disarmed by their Arm Toggle key. Press the bound Arm key to arm."
                .to_string(),
            bar_color: ACCENT_PRIMARY,
            headline_color: ACCENT_PRIMARY,
            border_color: BORDER_STRONG,
            enabled_count,
        };
    }

    if counts.compiling_count > 0 {
        return GlobalEngineSummary {
            headline: "COMPILING SCRIPT",
            subline: "Building Advanced Rust binary in background cache.".to_string(),
            bar_color: STATUS_INFO_INK,
            headline_color: STATUS_INFO_INK,
            border_color: BORDER_STRONG,
            enabled_count,
        };
    }

    if counts.error_count > 0 {
        return GlobalEngineSummary {
            headline: "SCRIPT ERROR",
            subline: "One or more enabled scripts stopped with an error. Inspect details below."
                .to_string(),
            bar_color: STATUS_DANGER,
            headline_color: STATUS_DANGER_INK,
            border_color: STATUS_DANGER,
            enabled_count,
        };
    }

    GlobalEngineSummary {
        headline: "ALL SCRIPTS OFF",
        subline: "No scripts are currently armed.".to_string(),
        bar_color: BORDER_STRONG,
        headline_color: TEXT_MUTED,
        border_color: BORDER_SUBTLE,
        enabled_count,
    }
}

#[derive(Default)]
struct EnabledPhaseTally {
    running_count: usize,
    armed_count: usize,
    waiting_count: usize,
    compiling_count: usize,
    error_count: usize,
    arm_toggle_off_count: usize,
    total_running_actions: u64,
}

fn tally_enabled_script_phases(
    enabled_scripts: &[&ScriptDocument],
    snapshot: &CoordinatorSnapshot,
) -> EnabledPhaseTally {
    let mut tally = EnabledPhaseTally::default();
    for script in enabled_scripts {
        let status = snapshot.status_for_script(&script.id);
        match status.map(|entry| entry.phase) {
            Some(ScriptExecutionPhase::Running) => {
                tally.running_count = tally.running_count.saturating_add(1);
                tally.total_running_actions = tally
                    .total_running_actions
                    .saturating_add(status.map(|entry| entry.total_actions_emitted).unwrap_or(0));
            }
            Some(ScriptExecutionPhase::Armed) => {
                tally.armed_count = tally.armed_count.saturating_add(1);
            }
            Some(ScriptExecutionPhase::WaitingForFocus) => {
                tally.waiting_count = tally.waiting_count.saturating_add(1);
            }
            Some(ScriptExecutionPhase::Compiling) => {
                tally.compiling_count = tally.compiling_count.saturating_add(1);
            }
            Some(ScriptExecutionPhase::Error) => {
                tally.error_count = tally.error_count.saturating_add(1);
            }
            Some(ScriptExecutionPhase::Disabled)
                if script.arm_toggle_key.is_some()
                    && status.is_some_and(|entry| !entry.is_armed_by_toggle) =>
            {
                tally.arm_toggle_off_count = tally.arm_toggle_off_count.saturating_add(1);
            }
            _ if matches!(script.target_scope, TargetScope::SelectedGame(_)) => {
                tally.waiting_count = tally.waiting_count.saturating_add(1);
            }
            _ => {
                tally.armed_count = tally.armed_count.saturating_add(1);
            }
        }
    }
    tally
}

fn build_armed_explanation(
    script: &ScriptDocument,
    runtime_status: Option<&ScriptRuntimeStatus>,
    is_uinput_ready: bool,
) -> String {
    let trigger_instruction = match script.mode {
        ScriptMode::VisualBlocks => describe_trigger_action(&script.visual_program.trigger),
        ScriptMode::AdvancedRust => "Advanced Rust worker active and awaiting input".to_string(),
    };

    let intercept_note = if !is_uinput_ready {
        " (uinput offline)"
    } else if !script.intercept_trigger_input {
        ""
    } else if runtime_status.is_some_and(|status| status.is_intercept_grab_active) {
        " (Intercept ON)"
    } else {
        " (Intercept armed)"
    };

    format!("Ready: {trigger_instruction}{intercept_note}")
}

fn describe_trigger_action(trigger: &TriggerBlock) -> String {
    let chord_label = trigger.chord_display_label().unwrap_or_default();
    match trigger {
        TriggerBlock::WhileInputHeld { .. } => format!("Hold {chord_label} to fire"),
        TriggerBlock::OnInputToggle { .. } => format!("Press {chord_label} to toggle firing"),
        TriggerBlock::OnInputPressed { .. } => format!("Press {chord_label} to run once"),
        TriggerBlock::LoopWhileActive => "Looping continuously while focused".to_string(),
    }
}

fn resolve_target_display_name(script: &ScriptDocument) -> &str {
    match &script.target_scope {
        TargetScope::SelectedGame(rule) if !rule.display_name.trim().is_empty() => {
            rule.display_name.trim()
        }
        TargetScope::SelectedGame(_) => "target game",
        TargetScope::Global => "Global",
    }
}

fn resolve_focused_window_label(focused_window: Option<&ActiveWindowSnapshot>) -> Option<String> {
    let window = focused_window.filter(|entry| !entry.is_empty())?;
    let raw_label = if !window.title.trim().is_empty() {
        window.title.trim()
    } else {
        window.app_id.trim()
    };
    if raw_label.is_empty() {
        return None;
    }
    Some(truncate_display_text(raw_label, MAX_FOCUSED_TITLE_CHARS))
}

fn plural_suffix(item_count: usize) -> &'static str {
    if item_count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::KeyCode;

    use super::*;
    use crate::models::{GameTargetRule, TargetMatchMode, inspect_script_validation};

    fn make_valid_enabled_script() -> ScriptDocument {
        let mut script = ScriptDocument::new_building_upgrade_template();
        script.is_enabled = true;
        script
    }

    fn make_snapshot_with_status(
        status: ScriptRuntimeStatus,
        is_emergency_stopped: bool,
        is_uinput_ready: bool,
    ) -> CoordinatorSnapshot {
        let mut snapshot = CoordinatorSnapshot {
            is_emergency_stopped,
            script_statuses: vec![status],
            ..CoordinatorSnapshot::default()
        };
        snapshot.input_status.is_uinput_ready = is_uinput_ready;
        snapshot
    }

    #[test]
    fn classifies_running_armed_waiting_arm_toggle_off_and_emergency_stop() {
        let mut script = make_valid_enabled_script();
        let validation = inspect_script_validation(&script);

        let mut running_status = ScriptRuntimeStatus::disabled(&script.id);
        running_status.phase = ScriptExecutionPhase::Running;
        running_status.is_focus_matched = true;
        running_status.is_armed_by_toggle = true;
        running_status.total_actions_emitted = 14;
        running_status.last_sampled_delay_ms = Some(58);
        let running_snapshot = make_snapshot_with_status(running_status, false, true);
        let running_info = classify_script_live_status(&script, &validation, &running_snapshot);
        assert_eq!(running_info.card_label, "RUNNING");
        assert_eq!(running_info.compact_label, "RUNNING");
        assert!(running_info.explanation.contains("14 actions sent"));
        assert!(running_info.explanation.contains("58 ms"));

        let mut armed_status = ScriptRuntimeStatus::disabled(&script.id);
        armed_status.phase = ScriptExecutionPhase::Armed;
        armed_status.is_focus_matched = true;
        armed_status.is_armed_by_toggle = true;
        armed_status.is_intercept_grab_active = true;
        let armed_snapshot = make_snapshot_with_status(armed_status, false, true);
        let armed_info = classify_script_live_status(&script, &validation, &armed_snapshot);
        assert_eq!(armed_info.card_label, "ARMED & READY");
        assert_eq!(armed_info.compact_label, "ARMED");
        assert!(armed_info.explanation.contains("Intercept ON"));

        script.visual_program.trigger = TriggerBlock::WhileInputHeld {
            trigger_input: clickforge_sdk::InputTrigger::Mouse(clickforge_sdk::MouseButton::Left),
            additional_held_inputs: vec![clickforge_sdk::InputTrigger::Key(KeyCode::LeftShift)],
        };
        let chord_armed_info = classify_script_live_status(&script, &validation, &armed_snapshot);
        assert!(
            chord_armed_info
                .explanation
                .contains("Hold Left Mouse + Left Shift to fire")
        );

        let mut disarmed_status = ScriptRuntimeStatus::disabled(&script.id);
        disarmed_status.phase = ScriptExecutionPhase::Disabled;
        disarmed_status.is_focus_matched = true;
        disarmed_status.is_armed_by_toggle = false;
        let disarmed_snapshot = make_snapshot_with_status(disarmed_status, false, true);
        let disarmed_info = classify_script_live_status(&script, &validation, &disarmed_snapshot);
        assert_eq!(disarmed_info.card_label, "ARM TOGGLE OFF");
        assert_eq!(disarmed_info.compact_label, "ARM OFF");
        assert!(disarmed_info.explanation.contains("F6"));

        script.target_scope = TargetScope::SelectedGame(GameTargetRule {
            display_name: "Here Comes the Horde".to_string(),
            match_mode: TargetMatchMode::PersistentAppAndTitle,
            window_title_contains: Some("Here Comes the Horde".to_string()),
            ..GameTargetRule::default()
        });
        let game_validation = inspect_script_validation(&script);
        let mut waiting_status = ScriptRuntimeStatus::disabled(&script.id);
        waiting_status.phase = ScriptExecutionPhase::WaitingForFocus;
        let waiting_snapshot = make_snapshot_with_status(waiting_status, false, true);
        let waiting_info =
            classify_script_live_status(&script, &game_validation, &waiting_snapshot);
        assert_eq!(waiting_info.card_label, "WAITING FOR GAME");
        assert_eq!(waiting_info.compact_label, "WAIT GAME");
        assert!(waiting_info.explanation.contains("Here Comes the Horde"));

        let stopped_snapshot =
            make_snapshot_with_status(ScriptRuntimeStatus::disabled(&script.id), true, true);
        let stopped_info =
            classify_script_live_status(&script, &game_validation, &stopped_snapshot);
        assert_eq!(stopped_info.card_label, "EMERGENCY STOPPED");
        assert_eq!(stopped_info.compact_label, "STOPPED");
    }

    #[test]
    fn classifies_global_hero_headline_priorities() {
        let mut script = make_valid_enabled_script();
        script.is_enabled = false;
        let empty_snapshot = CoordinatorSnapshot::default();
        assert_eq!(
            classify_global_engine_state(&[script.clone()], &empty_snapshot).headline,
            "ALL SCRIPTS OFF"
        );

        script.is_enabled = true;
        let mut waiting_status = ScriptRuntimeStatus::disabled(&script.id);
        waiting_status.phase = ScriptExecutionPhase::WaitingForFocus;
        let waiting_snapshot = make_snapshot_with_status(waiting_status, false, true);
        assert_eq!(
            classify_global_engine_state(&[script.clone()], &waiting_snapshot).headline,
            "WAITING FOR TARGET GAME"
        );

        let mut armed_status = ScriptRuntimeStatus::disabled(&script.id);
        armed_status.phase = ScriptExecutionPhase::Armed;
        let armed_snapshot = make_snapshot_with_status(armed_status, false, true);
        assert_eq!(
            classify_global_engine_state(&[script.clone()], &armed_snapshot).headline,
            "ARMED & READY"
        );

        let mut running_status = ScriptRuntimeStatus::disabled(&script.id);
        running_status.phase = ScriptExecutionPhase::Running;
        let running_snapshot = make_snapshot_with_status(running_status, false, true);
        assert_eq!(
            classify_global_engine_state(&[script.clone()], &running_snapshot).headline,
            "CLICKING LIVE"
        );

        let stopped_snapshot =
            make_snapshot_with_status(ScriptRuntimeStatus::disabled(&script.id), true, true);
        assert_eq!(
            classify_global_engine_state(&[script], &stopped_snapshot).headline,
            "EMERGENCY STOPPED"
        );
    }

    #[test]
    fn setup_needed_overrides_disabled_when_kill_switch_is_missing() {
        let mut script =
            ScriptDocument::new_blank("Draft", TargetScope::Global, ScriptMode::VisualBlocks);
        script.kill_switch_key = None;
        let validation = inspect_script_validation(&script);
        let snapshot = CoordinatorSnapshot::default();
        let status_info = classify_script_live_status(&script, &validation, &snapshot);
        assert_eq!(status_info.card_label, "SETUP NEEDED");
        assert_eq!(status_info.compact_label, "SETUP");

        script.kill_switch_key = Some(KeyCode::F8);
        let valid_report = inspect_script_validation(&script);
        let off_info = classify_script_live_status(&script, &valid_report, &snapshot);
        assert_eq!(off_info.card_label, "OFF");
    }
}
