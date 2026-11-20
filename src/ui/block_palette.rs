use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use egui::{
    Align, Button, Color32, CornerRadius, Frame, Layout, Margin, Pos2, Rect, RichText, ScrollArea,
    Sense, Stroke, Ui, pos2, vec2,
};

use crate::models::{
    BlockCategory, BlockId, BlockSlotTarget, ConditionExpr, StatementKind, TriggerBlock,
};
use crate::ui::theme::{
    BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX,
    SURFACE_ELEVATED, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY, category_color,
};

/// Default palette dock width in logical pixels.
pub const PALETTE_DEFAULT_WIDTH_PX: f32 = 228.0;

/// Minimum palette dock width in logical pixels.
pub const PALETTE_MIN_WIDTH_PX: f32 = 196.0;

/// Maximum palette dock width in logical pixels.
pub const PALETTE_MAX_WIDTH_PX: f32 = 300.0;

const PALETTE_STRIPE_WIDTH_PX: f32 = 4.0;
const PALETTE_DROP_SNAP_TOLERANCE_Y_PX: f32 = 30.0;
const PALETTE_DROP_MARGIN_X_PX: f32 = 28.0;

/// Payload carried during a drag-and-drop gesture across the palette and block canvas.
#[derive(Debug, Clone)]
pub enum BlockDragPayload {
    ReplaceTrigger(TriggerBlock),
    NewStatement(StatementKind),
    MoveExistingBlock(BlockId),
}

/// Active pointer drag state with visual preview metadata.
#[derive(Debug, Clone)]
pub struct ActiveBlockDrag {
    pub payload: BlockDragPayload,
    pub preview_label: String,
    pub accent_color: Color32,
}

/// Closest valid drop slot computed during a canvas layout frame.
#[derive(Debug, Clone, Copy)]
pub struct HoveredDropSlot {
    pub target_slot: BlockSlotTarget,
    pub indicator_line_y_px: f32,
    pub indicator_min_x_px: f32,
    pub indicator_max_x_px: f32,
    pub distance_y_px: f32,
}

/// Shared drag-and-drop state synchronized between the palette and block canvas.
#[derive(Debug, Default)]
pub struct BlockDragState {
    pub active_drag: Option<ActiveBlockDrag>,
    pub hovered_slot: Option<HoveredDropSlot>,
    pub is_hovering_trigger_hat: bool,
}

impl BlockDragState {
    /// Clears per-frame hover candidates before painting the block canvas.
    pub fn reset_frame_targets(&mut self) {
        self.hovered_slot = None;
        self.is_hovering_trigger_hat = false;
    }

    /// Starts dragging a new or existing block if no drag is active yet.
    pub fn start_drag(
        &mut self,
        payload: BlockDragPayload,
        preview_label: impl Into<String>,
        accent_color: Color32,
    ) {
        self.active_drag = Some(ActiveBlockDrag {
            payload,
            preview_label: preview_label.into(),
            accent_color,
        });
    }

    /// Returns true while dragging a statement block from the palette or canvas.
    pub fn is_dragging_statement(&self) -> bool {
        matches!(
            self.active_drag.as_ref().map(|drag| &drag.payload),
            Some(BlockDragPayload::NewStatement(_) | BlockDragPayload::MoveExistingBlock(_))
        )
    }

    /// Returns true while dragging a replacement trigger hat block.
    pub fn is_dragging_trigger(&self) -> bool {
        matches!(
            self.active_drag.as_ref().map(|drag| &drag.payload),
            Some(BlockDragPayload::ReplaceTrigger(_))
        )
    }

    /// Returns the ID of the existing canvas block currently being dragged, if any.
    pub fn dragged_existing_block_id(&self) -> Option<BlockId> {
        match self.active_drag.as_ref().map(|drag| &drag.payload) {
            Some(BlockDragPayload::MoveExistingBlock(block_id)) => Some(*block_id),
            _ => None,
        }
    }

    /// Evaluates a candidate insertion slot against the pointer position and keeps the closest match.
    pub fn offer_slot_candidate(
        &mut self,
        pointer_pos: Pos2,
        target_slot: BlockSlotTarget,
        line_y_px: f32,
        min_x_px: f32,
        max_x_px: f32,
    ) {
        if !self.is_dragging_statement() {
            return;
        }
        let is_within_x = pointer_pos.x >= min_x_px - PALETTE_DROP_MARGIN_X_PX
            && pointer_pos.x <= max_x_px + PALETTE_DROP_MARGIN_X_PX;
        if !is_within_x {
            return;
        }
        let distance_y_px = (pointer_pos.y - line_y_px).abs();
        if distance_y_px > PALETTE_DROP_SNAP_TOLERANCE_Y_PX {
            return;
        }

        let should_replace = self
            .hovered_slot
            .is_none_or(|existing| distance_y_px < existing.distance_y_px);
        if should_replace {
            self.hovered_slot = Some(HoveredDropSlot {
                target_slot,
                indicator_line_y_px: line_y_px,
                indicator_min_x_px: min_x_px,
                indicator_max_x_px: max_x_px,
                distance_y_px,
            });
        }
    }
}

/// Immediate click-to-insert actions emitted by the block palette.
#[derive(Debug, Clone)]
pub enum PaletteAction {
    ApplyTrigger(TriggerBlock),
    AppendStatement(StatementKind),
}

struct TriggerTemplateItem {
    title: &'static str,
    summary: &'static str,
    build_trigger: fn() -> TriggerBlock,
}

struct StatementTemplateItem {
    title: &'static str,
    summary: &'static str,
    category: BlockCategory,
    build_kind: fn() -> StatementKind,
}

const TRIGGER_TEMPLATES: [TriggerTemplateItem; 4] = [
    TriggerTemplateItem {
        title: "While Input Held",
        summary: "Repeats body while mouse button or key is held down",
        build_trigger: || TriggerBlock::WhileInputHeld {
            trigger_input: InputTrigger::Mouse(MouseButton::Left),
        },
    },
    TriggerTemplateItem {
        title: "Toggle on Input",
        summary: "Starts or stops the loop each time the trigger is pressed",
        build_trigger: || TriggerBlock::OnInputToggle {
            trigger_input: InputTrigger::Key(KeyCode::F5),
        },
    },
    TriggerTemplateItem {
        title: "Once on Press",
        summary: "Executes the sequence once per trigger press",
        build_trigger: || TriggerBlock::OnInputPressed {
            trigger_input: InputTrigger::Mouse(MouseButton::Side),
        },
    },
    TriggerTemplateItem {
        title: "Loop While Active",
        summary: "Loops continuously while target window is focused and script is armed",
        build_trigger: || TriggerBlock::LoopWhileActive,
    },
];

const CONTROL_TEMPLATES: [StatementTemplateItem; 6] = [
    StatementTemplateItem {
        title: "If",
        summary: "Runs nested blocks when condition is true",
        category: BlockCategory::Control,
        build_kind: || StatementKind::IfThen {
            condition: ConditionExpr::IsInputPressed {
                input: InputTrigger::Mouse(MouseButton::Left),
            },
            then_blocks: Vec::new(),
        },
    },
    StatementTemplateItem {
        title: "If / Else",
        summary: "Branches between Then and Else child block stacks",
        category: BlockCategory::Control,
        build_kind: || StatementKind::IfThenElse {
            condition: ConditionExpr::RandomChance {
                chance_percent: 50.0,
            },
            then_blocks: Vec::new(),
            else_blocks: Vec::new(),
        },
    },
    StatementTemplateItem {
        title: "While",
        summary: "Repeats nested blocks while condition stays true",
        category: BlockCategory::Control,
        build_kind: || StatementKind::WhileCondition {
            condition: ConditionExpr::IsInputPressed {
                input: InputTrigger::Mouse(MouseButton::Left),
            },
            body_blocks: Vec::new(),
        },
    },
    StatementTemplateItem {
        title: "Repeat N Times",
        summary: "Repeats nested blocks a fixed number of iterations",
        category: BlockCategory::Control,
        build_kind: || StatementKind::RepeatCount {
            repeat_count: 5,
            body_blocks: Vec::new(),
        },
    },
    StatementTemplateItem {
        title: "Break Loop",
        summary: "Exits the innermost Repeat or While loop",
        category: BlockCategory::Control,
        build_kind: || StatementKind::BreakLoop,
    },
    StatementTemplateItem {
        title: "Stop Script",
        summary: "Stops the current script pass immediately",
        category: BlockCategory::Control,
        build_kind: || StatementKind::StopScript,
    },
];

const ACTION_TEMPLATES: [StatementTemplateItem; 9] = [
    StatementTemplateItem {
        title: "Mouse Click",
        summary: "Presses and releases a mouse button with hold duration",
        category: BlockCategory::Action,
        build_kind: || StatementKind::MouseClick {
            mouse_button: MouseButton::Left,
            hold_duration_ms: 12,
        },
    },
    StatementTemplateItem {
        title: "Mouse Down",
        summary: "Holds down a mouse button until released",
        category: BlockCategory::Action,
        build_kind: || StatementKind::MouseDown {
            mouse_button: MouseButton::Left,
        },
    },
    StatementTemplateItem {
        title: "Mouse Up",
        summary: "Releases a held mouse button",
        category: BlockCategory::Action,
        build_kind: || StatementKind::MouseUp {
            mouse_button: MouseButton::Left,
        },
    },
    StatementTemplateItem {
        title: "Mouse Move",
        summary: "Moves the pointer by a relative pixel offset",
        category: BlockCategory::Action,
        build_kind: || StatementKind::MouseMoveRelative {
            delta_x_px: 0,
            delta_y_px: 0,
        },
    },
    StatementTemplateItem {
        title: "Mouse Scroll",
        summary: "Scrolls mouse wheel vertically (positive = up, negative = down)",
        category: BlockCategory::Action,
        build_kind: || StatementKind::MouseScroll { vertical_steps: 1 },
    },
    StatementTemplateItem {
        title: "Key Tap",
        summary: "Presses and releases a keyboard key with hold duration",
        category: BlockCategory::Action,
        build_kind: || StatementKind::KeyTap {
            key_code: KeyCode::E,
            hold_duration_ms: 18,
        },
    },
    StatementTemplateItem {
        title: "Key Down",
        summary: "Holds down a keyboard key until released",
        category: BlockCategory::Action,
        build_kind: || StatementKind::KeyDown {
            key_code: KeyCode::LeftShift,
        },
    },
    StatementTemplateItem {
        title: "Key Up",
        summary: "Releases a held keyboard key",
        category: BlockCategory::Action,
        build_kind: || StatementKind::KeyUp {
            key_code: KeyCode::LeftShift,
        },
    },
    StatementTemplateItem {
        title: "Type Text",
        summary: "Types ASCII text with per-keystroke delay and jitter",
        category: BlockCategory::Action,
        build_kind: || StatementKind::TypeText {
            text: "gg".to_string(),
            keystroke_delay_ms: 35,
            jitter_ms: 10,
        },
    },
];

const TIMING_TEMPLATES: [StatementTemplateItem; 3] = [
    StatementTemplateItem {
        title: "Wait Fixed",
        summary: "Pauses for a constant duration in milliseconds",
        category: BlockCategory::Timing,
        build_kind: || StatementKind::WaitFixed { delay_ms: 50 },
    },
    StatementTemplateItem {
        title: "Wait Uniform",
        summary: "Pauses for a random uniform delay between min and max ms",
        category: BlockCategory::Timing,
        build_kind: || StatementKind::WaitRandomUniform {
            min_delay_ms: 40,
            max_delay_ms: 80,
        },
    },
    StatementTemplateItem {
        title: "Wait Gaussian",
        summary: "Pauses for base ms plus a clamped Gaussian jitter offset",
        category: BlockCategory::Timing,
        build_kind: || StatementKind::WaitRandomGaussian {
            base_delay_ms: 45,
            min_offset_ms: 10.0,
            max_offset_ms: 20.0,
            mean_offset_ms: 15.0,
            std_dev_ms: 2.5,
        },
    },
];

const VARIABLE_TEMPLATES: [StatementTemplateItem; 2] = [
    StatementTemplateItem {
        title: "Set Variable",
        summary: "Assigns a numeric value to a local script variable",
        category: BlockCategory::Variable,
        build_kind: || StatementKind::SetVariable {
            variable_name: "counter".to_string(),
            value: 0.0,
        },
    },
    StatementTemplateItem {
        title: "Change Variable",
        summary: "Adds a delta to a local script variable",
        category: BlockCategory::Variable,
        build_kind: || StatementKind::ChangeVariable {
            variable_name: "counter".to_string(),
            delta_value: 1.0,
        },
    },
];

/// Renders the categorized Scratch-style block palette with drag-to-canvas and click-to-add.
pub fn render_block_palette(
    ui: &mut Ui,
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) {
    ui.label(
        RichText::new("Block Palette")
            .size(FONT_SIZE_BODY_PX)
            .strong()
            .color(TEXT_PRIMARY),
    );
    ui.label(
        RichText::new("Drag onto canvas or click +")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    ui.add_space(6.0);

    ScrollArea::vertical()
        .id_salt("block_palette_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            render_trigger_palette_section(ui, drag_state, emitted_actions);
            ui.add_space(8.0);
            render_statement_palette_section(
                ui,
                "Control",
                BlockCategory::Control,
                &CONTROL_TEMPLATES,
                drag_state,
                emitted_actions,
            );
            ui.add_space(8.0);
            render_statement_palette_section(
                ui,
                "Actions",
                BlockCategory::Action,
                &ACTION_TEMPLATES,
                drag_state,
                emitted_actions,
            );
            ui.add_space(8.0);
            render_statement_palette_section(
                ui,
                "Timing",
                BlockCategory::Timing,
                &TIMING_TEMPLATES,
                drag_state,
                emitted_actions,
            );
            ui.add_space(8.0);
            render_statement_palette_section(
                ui,
                "Variables",
                BlockCategory::Variable,
                &VARIABLE_TEMPLATES,
                drag_state,
                emitted_actions,
            );
        });
}

fn render_trigger_palette_section(
    ui: &mut Ui,
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) {
    let accent_color = category_color(BlockCategory::Trigger);
    render_palette_category_heading(ui, "Triggers", accent_color);

    for template in &TRIGGER_TEMPLATES {
        render_palette_card(
            ui,
            template.title,
            template.summary,
            accent_color,
            "Set",
            || {
                emitted_actions.push(PaletteAction::ApplyTrigger((template.build_trigger)()));
            },
            || {
                drag_state.start_drag(
                    BlockDragPayload::ReplaceTrigger((template.build_trigger)()),
                    template.title,
                    accent_color,
                );
            },
        );
    }
}

fn render_statement_palette_section(
    ui: &mut Ui,
    section_title: &str,
    category: BlockCategory,
    templates: &[StatementTemplateItem],
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) {
    let accent_color = category_color(category);
    render_palette_category_heading(ui, section_title, accent_color);

    for template in templates {
        render_palette_card(
            ui,
            template.title,
            template.summary,
            category_color(template.category),
            "+",
            || {
                emitted_actions.push(PaletteAction::AppendStatement((template.build_kind)()));
            },
            || {
                drag_state.start_drag(
                    BlockDragPayload::NewStatement((template.build_kind)()),
                    template.title,
                    accent_color,
                );
            },
        );
    }
}

fn render_palette_category_heading(ui: &mut Ui, title: &str, accent_color: Color32) {
    ui.horizontal(|ui| {
        let (bar_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
        ui.painter().rect_filled(bar_rect, 2.0, accent_color);
        ui.label(
            RichText::new(title)
                .size(FONT_SIZE_SMALL_PX)
                .strong()
                .color(TEXT_MUTED),
        );
    });
    ui.add_space(3.0);
}

fn render_palette_card(
    ui: &mut Ui,
    title: &str,
    summary: &str,
    accent_color: Color32,
    quick_action_label: &str,
    on_quick_insert: impl FnOnce(),
    on_start_drag: impl FnOnce(),
) {
    let card_frame = Frame::NONE
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin {
            left: 10,
            right: 6,
            top: 4,
            bottom: 4,
        });

    let mut did_click_quick_button = false;

    let frame_response = card_frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(title)
                    .size(FONT_SIZE_SMALL_PX)
                    .strong()
                    .color(TEXT_PRIMARY),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let insert_button = Button::new(
                    RichText::new(quick_action_label)
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_PRIMARY),
                )
                .fill(SURFACE_HOVER)
                .stroke(Stroke::new(1.0, BORDER_STRONG))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));
                if ui.add(insert_button).clicked() {
                    did_click_quick_button = true;
                }
            });
        });
    });

    let card_rect = frame_response.response.rect;
    let stripe_rect = Rect::from_min_max(
        card_rect.min,
        pos2(card_rect.min.x + PALETTE_STRIPE_WIDTH_PX, card_rect.max.y),
    );
    ui.painter().rect_filled(
        stripe_rect,
        CornerRadius {
            nw: CORNER_RADIUS_SM_PX,
            sw: CORNER_RADIUS_SM_PX,
            ne: 0,
            se: 0,
        },
        accent_color,
    );

    if did_click_quick_button {
        on_quick_insert();
    } else {
        let drag_response = frame_response
            .response
            .interact(Sense::click_and_drag())
            .on_hover_text(summary);
        if drag_response.drag_started() {
            on_start_drag();
        }
    }

    ui.add_space(4.0);
}
