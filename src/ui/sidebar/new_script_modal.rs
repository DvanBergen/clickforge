use clickforge_sdk::KeyCode;
use egui::{
    Align, Color32, CornerRadius, Frame, Id, Key, Layout, Margin, Modal, RichText, Stroke,
    TextEdit, Theme, Ui, vec2,
};

use crate::models::ScriptMode;
use crate::ui::sidebar::{BADGE_ACCENT, BADGE_HOVER_ACTION, SidebarAction};
use crate::ui::theme::{
    ACCENT_PRIMARY, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_MD_PX, CORNER_RADIUS_PILL_PX,
    CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_HEADING_PX, FONT_SIZE_SMALL_PX,
    INK_ON_ACCENT, STATUS_RUNNING_INK, SURFACE_ELEVATED, SURFACE_HOVER, SURFACE_PANEL, TEXT_MUTED,
    TEXT_PRIMARY, build_styled_button, render_badge_pill, rich_bold, rich_mono_bold, rich_semibold,
};

const NEW_SCRIPT_MODAL_WIDTH_PX: f32 = 468.0;

const KILL_SWITCH_QUICK_PICKS: [(Option<KeyCode>, &str); 5] = [
    (Some(KeyCode::F8), "F8"),
    (Some(KeyCode::Pause), "Pause"),
    (Some(KeyCode::ScrollLock), "ScrollLock"),
    (Some(KeyCode::F9), "F9"),
    (None, "Bind Later"),
];

const TEMPLATE_CHOICES: [(NewScriptTemplateChoice, &str); 3] = [
    (NewScriptTemplateChoice::VisualBlocks, "Visual Blocks"),
    (NewScriptTemplateChoice::AdvancedRust, "Advanced Rust"),
    (
        NewScriptTemplateChoice::BuildingUpgradeTemplate,
        "Building Upgrade Template",
    ),
];

/// Authoring template option selected inside the New Script modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NewScriptTemplateChoice {
    #[default]
    VisualBlocks,
    AdvancedRust,
    BuildingUpgradeTemplate,
}

/// Transient form state for the New Script creation modal.
#[derive(Debug, Clone, Default)]
pub struct NewScriptModalState {
    pub is_open: bool,
    pub script_name: String,
    pub is_selected_game_scope: bool,
    pub template_choice: NewScriptTemplateChoice,
    pub selected_kill_switch_key: Option<KeyCode>,
    should_focus_name_input: bool,
}

impl NewScriptModalState {
    /// Resets form fields with no auto-filled Kill-Switch and opens the modal dialog.
    pub fn open_with_defaults(&mut self, existing_script_count: usize) {
        self.script_name = format!("New Script {}", existing_script_count.saturating_add(1));
        self.is_selected_game_scope = true;
        self.template_choice = NewScriptTemplateChoice::VisualBlocks;
        self.selected_kill_switch_key = None;
        self.should_focus_name_input = true;
        self.is_open = true;
    }
}

/// Renders the modal dialog for creating a new script with explicit Kill-Switch selection.
pub fn render_new_script_modal(
    egui_context: &egui::Context,
    modal_state: &mut NewScriptModalState,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    if !modal_state.is_open {
        return;
    }

    let modal_frame = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .shadow(egui_context.style_of(Theme::Dark).visuals.window_shadow)
        .inner_margin(Margin::same(18));

    let modal_response = Modal::new(Id::unique("new_script_modal"))
        .frame(modal_frame)
        .show(egui_context, |ui| {
            ui.set_width(NEW_SCRIPT_MODAL_WIDTH_PX);
            ui.label(rich_bold(
                "Create New Script",
                FONT_SIZE_HEADING_PX,
                TEXT_PRIMARY,
            ));
            render_modal_field_label(ui, "Script Name");
            let name_response = ui.add(
                TextEdit::singleline(&mut modal_state.script_name)
                    .desired_width(f32::INFINITY)
                    .hint_text("Enter script name"),
            );
            if modal_state.should_focus_name_input {
                name_response.request_focus();
                modal_state.should_focus_name_input = false;
            }

            render_modal_field_label(ui, "Execution Scope");
            ui.horizontal(|ui| {
                for (is_game, label_text) in
                    [(true, "Selected Game Only"), (false, "Global (Any Window)")]
                {
                    if render_choice_chip(
                        ui,
                        label_text,
                        modal_state.is_selected_game_scope == is_game,
                    ) {
                        modal_state.is_selected_game_scope = is_game;
                    }
                }
            });

            render_modal_field_label(ui, "Starting Mode");
            ui.horizontal(|ui| {
                for (option, label_text) in TEMPLATE_CHOICES {
                    if render_choice_chip(ui, label_text, modal_state.template_choice == option) {
                        modal_state.template_choice = option;
                    }
                }
            });

            ui.add_space(12.0);
            render_kill_switch_quick_picker(ui, &mut modal_state.selected_kill_switch_key);

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);
            render_new_script_footer_buttons(ui, modal_state, emitted_actions);
        });

    if modal_response.should_close() {
        modal_state.is_open = false;
    }
}

/// Maps `NewScriptTemplateChoice` to its underlying `ScriptMode`.
pub fn script_mode_for_template(template_choice: NewScriptTemplateChoice) -> ScriptMode {
    match template_choice {
        NewScriptTemplateChoice::VisualBlocks
        | NewScriptTemplateChoice::BuildingUpgradeTemplate => ScriptMode::VisualBlocks,
        NewScriptTemplateChoice::AdvancedRust => ScriptMode::AdvancedRust,
    }
}

fn render_modal_field_label(ui: &mut Ui, label_text: &str) {
    ui.add_space(10.0);
    ui.label(rich_semibold(label_text, FONT_SIZE_SMALL_PX, TEXT_MUTED));
    ui.add_space(3.0);
}

fn render_kill_switch_quick_picker(ui: &mut Ui, selected_kill_switch_key: &mut Option<KeyCode>) {
    ui.horizontal(|ui| {
        ui.label(rich_semibold(
            "Hard Kill-Switch Key",
            FONT_SIZE_SMALL_PX,
            TEXT_MUTED,
        ));
        render_badge_pill(
            ui,
            "Required before Save / Enable",
            SURFACE_ELEVATED,
            TEXT_MUTED,
            BORDER_SUBTLE,
        );
    });
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (candidate_key, chip_label) in KILL_SWITCH_QUICK_PICKS {
            let is_selected = *selected_kill_switch_key == candidate_key;
            let (fill, ink, border) = match (is_selected, candidate_key.is_some()) {
                (true, true) => BADGE_ACCENT,
                (true, false) => BADGE_HOVER_ACTION,
                (false, _) => (SURFACE_ELEVATED, TEXT_MUTED, BORDER_SUBTLE),
            };
            let label = if candidate_key.is_some() {
                rich_mono_bold(chip_label, FONT_SIZE_SMALL_PX, ink)
            } else {
                rich_semibold(chip_label, FONT_SIZE_SMALL_PX, ink)
            };
            if ui
                .add(build_styled_button(
                    label,
                    fill,
                    border,
                    CORNER_RADIUS_PILL_PX,
                    vec2(0.0, 26.0),
                ))
                .clicked()
            {
                *selected_kill_switch_key = candidate_key;
            }
        }
    });

    ui.add_space(4.0);
    let (helper_text, helper_color) = match selected_kill_switch_key {
        Some(chosen_key) => (
            format!(
                "Pressing {} immediately stops all scripts and releases every input.",
                chosen_key.display_label()
            ),
            STATUS_RUNNING_INK,
        ),
        None => (
            "No Kill-Switch selected. Save, Compile, and Enable stay blocked until one is bound."
                .to_string(),
            TEXT_MUTED,
        ),
    };
    ui.label(
        RichText::new(helper_text)
            .size(FONT_SIZE_SMALL_PX)
            .color(helper_color),
    );
}

fn render_choice_chip(ui: &mut Ui, label_text: &str, is_selected: bool) -> bool {
    let (fill, border, label) = if is_selected {
        (
            SURFACE_HOVER,
            ACCENT_PRIMARY,
            rich_semibold(label_text, FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
        )
    } else {
        (
            SURFACE_ELEVATED,
            BORDER_SUBTLE,
            RichText::new(label_text)
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        )
    };
    ui.add(build_styled_button(
        label,
        fill,
        border,
        CORNER_RADIUS_SM_PX,
        vec2(0.0, 26.0),
    ))
    .clicked()
}

fn render_new_script_footer_buttons(
    ui: &mut Ui,
    modal_state: &mut NewScriptModalState,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let trimmed_name = modal_state.script_name.trim().to_string();
    let can_create = !trimmed_name.is_empty();
    let mut should_submit = can_create && ui.ctx().input(|input| input.key_pressed(Key::Enter));

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let (fill, ink) = if can_create {
            (ACCENT_PRIMARY, INK_ON_ACCENT)
        } else {
            (SURFACE_ELEVATED, TEXT_MUTED)
        };
        let create = build_styled_button(
            rich_semibold("Create Script", FONT_SIZE_BODY_PX, ink),
            fill,
            Color32::TRANSPARENT,
            CORNER_RADIUS_SM_PX,
            vec2(110.0, 30.0),
        );
        should_submit |= ui.add_enabled(can_create, create).clicked();
        let cancel = build_styled_button(
            rich_semibold("Cancel", FONT_SIZE_BODY_PX, TEXT_PRIMARY),
            SURFACE_ELEVATED,
            BORDER_SUBTLE,
            CORNER_RADIUS_SM_PX,
            vec2(74.0, 30.0),
        );
        if ui.add(cancel).clicked() {
            modal_state.is_open = false;
        }
    });

    if should_submit {
        emitted_actions.push(SidebarAction::CreateScript {
            name: trimmed_name,
            is_selected_game_scope: modal_state.is_selected_game_scope,
            template_choice: modal_state.template_choice,
            kill_switch_key: modal_state.selected_kill_switch_key,
        });
        modal_state.is_open = false;
    }
}
