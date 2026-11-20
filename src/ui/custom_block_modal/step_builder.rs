use egui::{
    Align, Button, Color32, CornerRadius, Frame, Id, Layout, Margin, Popup, PopupCloseBehavior,
    RichText, ScrollArea, Stroke, Theme, Ui, vec2,
};

use crate::models::validation::MAX_BLOCK_NESTING_DEPTH;
use crate::models::{
    BlockCategory, BlockId, CustomBlockDefinition, MAX_CUSTOM_BLOCK_STEPS, StatementBlock,
    StatementKind, lookup_custom_block,
};
use crate::ui::block_field_editors::render_statement_inline_fields;
use crate::ui::block_palette::templates::{PALETTE_TEMPLATES, build_default_statement_kind};
use crate::ui::custom_block_modal::CustomBlockModalState;
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE,
    BlockVisualColors, CORNER_RADIUS_MD_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX,
    FONT_SIZE_SMALL_PX, STATUS_DANGER_INK, SURFACE_ELEVATED, SURFACE_PANEL, TEXT_MUTED,
    TEXT_PRIMARY, block_visual_colors, build_styled_button, category_color,
    custom_block_visual_colors, paint_status_dot, render_badge_pill, rich_mono_bold, rich_semibold,
};

const STEPS_SCROLL_MAX_HEIGHT_PX: f32 = 310.0;
const ADD_STEP_POPUP_WIDTH_PX: f32 = 248.0;
const ADD_STEP_POPUP_MAX_HEIGHT_PX: f32 = 300.0;
const STEP_CONTROL_BUTTON_SIZE_PX: f32 = 22.0;
const STEP_CONTROLS_RESERVE_WIDTH_PX: f32 = 82.0;
const STEP_FIELDS_MIN_WIDTH_PX: f32 = 160.0;
const MODAL_ID_NAMESPACE: &str = "custom_block_modal";

enum StepSliceMutation {
    MoveUp(usize),
    MoveDown(usize),
    Delete(usize),
}

/// Renders the scrollable inline step editor and `+ Add Step` menu inside the Custom Tile modal.
pub(super) fn render_steps_section(
    ui: &mut Ui,
    modal_state: &mut CustomBlockModalState,
    existing_custom_blocks: &[CustomBlockDefinition],
) {
    let mut total_steps = modal_state.draft.total_block_count();
    let can_add_step = total_steps < MAX_CUSTOM_BLOCK_STEPS;
    let draft_id = modal_state.draft.id.clone();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(rich_semibold("Steps", FONT_SIZE_BODY_PX, TEXT_PRIMARY));
        render_badge_pill(
            ui,
            &format!("{total_steps} / {MAX_CUSTOM_BLOCK_STEPS}"),
            SURFACE_ELEVATED,
            TEXT_MUTED,
            BORDER_SUBTLE,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let add_button = build_styled_button(
                rich_semibold(
                    "+ Add Step ▾",
                    FONT_SIZE_SMALL_PX,
                    if can_add_step {
                        ACCENT_PRIMARY
                    } else {
                        TEXT_MUTED
                    },
                ),
                if can_add_step {
                    ACCENT_PRIMARY_TINT
                } else {
                    SURFACE_ELEVATED
                },
                if can_add_step {
                    ACCENT_PRIMARY
                } else {
                    BORDER_SUBTLE
                },
                CORNER_RADIUS_SM_PX,
                vec2(104.0, 26.0),
            );
            let add_response = ui.add_enabled(can_add_step, add_button);
            if !can_add_step {
                add_response.on_disabled_hover_text(format!(
                    "Maximum of {MAX_CUSTOM_BLOCK_STEPS} blocks per custom tile reached."
                ));
            } else if let Some(new_kind) = render_add_step_popup(
                &add_response,
                Id::unique(("custom_modal_add_step_root", draft_id.as_str())),
                &draft_id,
                existing_custom_blocks,
            ) {
                let new_block = modal_state.draft.create_block(new_kind);
                modal_state.draft.statements.push(new_block);
                total_steps = total_steps.saturating_add(1);
            }
        });
    });

    ui.add_space(6.0);
    Frame::NONE
        .fill(BG_CANVAS)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if modal_state.draft.statements.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(14.0);
                    ui.label(rich_semibold(
                        "No steps in this tile yet",
                        FONT_SIZE_BODY_PX,
                        TEXT_PRIMARY,
                    ));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Click + Add Step above to compose clicks, holds, waits, keys, or control flow.",
                        )
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(14.0);
                });
                return;
            }

            ScrollArea::vertical()
                .id_salt("custom_block_modal_steps_scroll")
                .max_height(STEPS_SCROLL_MAX_HEIGHT_PX)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let mut next_block_id = modal_state.draft.next_block_id;
                    render_modal_step_slice(
                        ui,
                        &mut modal_state.draft.statements,
                        &mut next_block_id,
                        &mut total_steps,
                        &draft_id,
                        existing_custom_blocks,
                        0,
                    );
                    modal_state.draft.next_block_id = next_block_id;
                });
        });
}

fn render_modal_step_slice(
    ui: &mut Ui,
    statements: &mut Vec<StatementBlock>,
    next_block_id: &mut u64,
    total_steps: &mut usize,
    draft_id: &str,
    existing_custom_blocks: &[CustomBlockDefinition],
    nesting_depth: usize,
) {
    let step_count = statements.len();
    let mut deferred_mutation: Option<StepSliceMutation> = None;

    for (step_index, block) in statements.iter_mut().enumerate() {
        let visual_colors = resolve_step_card_colors(&block.kind, existing_custom_blocks);
        let can_move_up = step_index > 0;
        let can_move_down = step_index.saturating_add(1) < step_count;

        Frame::NONE
            .fill(visual_colors.fill)
            .stroke(Stroke::new(1.0, visual_colors.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(rich_mono_bold(
                        format!("{}.", step_index.saturating_add(1)),
                        FONT_SIZE_SMALL_PX,
                        visual_colors.text_muted,
                    ));

                    let fields_width_px = (ui.available_width() - STEP_CONTROLS_RESERVE_WIDTH_PX)
                        .max(STEP_FIELDS_MIN_WIDTH_PX);
                    ui.allocate_ui_with_layout(
                        vec2(fields_width_px, 0.0),
                        Layout::left_to_right(Align::Center).with_main_wrap(true),
                        |ui| {
                            let _ = render_statement_inline_fields(
                                ui,
                                MODAL_ID_NAMESPACE,
                                block.id,
                                &mut block.kind,
                                visual_colors,
                                existing_custom_blocks,
                            );
                        },
                    );

                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        if render_step_order_button(ui, "×", STATUS_DANGER_INK, true, "Delete step")
                        {
                            deferred_mutation = Some(StepSliceMutation::Delete(step_index));
                        }
                        if render_step_order_button(
                            ui,
                            "↓",
                            visual_colors.text_primary,
                            can_move_down,
                            "Move step down",
                        ) {
                            deferred_mutation = Some(StepSliceMutation::MoveDown(step_index));
                        }
                        if render_step_order_button(
                            ui,
                            "↑",
                            visual_colors.text_primary,
                            can_move_up,
                            "Move step up",
                        ) {
                            deferred_mutation = Some(StepSliceMutation::MoveUp(step_index));
                        }
                    });
                });

                if nesting_depth < MAX_BLOCK_NESTING_DEPTH {
                    render_modal_nested_branches(
                        ui,
                        block.id,
                        &mut block.kind,
                        next_block_id,
                        total_steps,
                        draft_id,
                        existing_custom_blocks,
                        nesting_depth.saturating_add(1),
                    );
                }
            });

        if step_index.saturating_add(1) < step_count {
            ui.add_space(6.0);
        }
    }

    apply_step_slice_mutation(statements, deferred_mutation);
}

#[allow(clippy::too_many_arguments)]
fn render_modal_nested_branches(
    ui: &mut Ui,
    parent_block_id: BlockId,
    statement_kind: &mut StatementKind,
    next_block_id: &mut u64,
    total_steps: &mut usize,
    draft_id: &str,
    existing_custom_blocks: &[CustomBlockDefinition],
    child_depth: usize,
) {
    match statement_kind {
        StatementKind::IfThen { then_blocks, .. } => {
            render_modal_branch_box(
                ui,
                "Then",
                ("then", parent_block_id.0),
                then_blocks,
                next_block_id,
                total_steps,
                draft_id,
                existing_custom_blocks,
                child_depth,
            );
        }
        StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } => {
            render_modal_branch_box(
                ui,
                "Then",
                ("then", parent_block_id.0),
                then_blocks,
                next_block_id,
                total_steps,
                draft_id,
                existing_custom_blocks,
                child_depth,
            );
            render_modal_branch_box(
                ui,
                "Else",
                ("else", parent_block_id.0),
                else_blocks,
                next_block_id,
                total_steps,
                draft_id,
                existing_custom_blocks,
                child_depth,
            );
        }
        StatementKind::WhileCondition { body_blocks, .. }
        | StatementKind::RepeatCount { body_blocks, .. } => {
            render_modal_branch_box(
                ui,
                "Body",
                ("body", parent_block_id.0),
                body_blocks,
                next_block_id,
                total_steps,
                draft_id,
                existing_custom_blocks,
                child_depth,
            );
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn render_modal_branch_box(
    ui: &mut Ui,
    branch_label: &str,
    branch_salt: (&str, u64),
    branch_blocks: &mut Vec<StatementBlock>,
    next_block_id: &mut u64,
    total_steps: &mut usize,
    draft_id: &str,
    existing_custom_blocks: &[CustomBlockDefinition],
    child_depth: usize,
) {
    ui.add_space(6.0);
    Frame::NONE
        .fill(BG_CANVAS)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(rich_semibold(branch_label, FONT_SIZE_SMALL_PX, TEXT_MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_add_child = *total_steps < MAX_CUSTOM_BLOCK_STEPS;
                    let add_child = build_styled_button(
                        rich_semibold(
                            "+ Step ▾",
                            FONT_SIZE_SMALL_PX,
                            if can_add_child {
                                TEXT_PRIMARY
                            } else {
                                TEXT_MUTED
                            },
                        ),
                        SURFACE_ELEVATED,
                        if can_add_child {
                            BORDER_STRONG
                        } else {
                            BORDER_SUBTLE
                        },
                        CORNER_RADIUS_SM_PX,
                        vec2(64.0, 20.0),
                    );
                    let add_response = ui.add_enabled(can_add_child, add_child);
                    if !can_add_child {
                        add_response.on_disabled_hover_text(format!(
                            "Maximum of {MAX_CUSTOM_BLOCK_STEPS} blocks per custom tile reached."
                        ));
                    } else if let Some(new_kind) = render_add_step_popup(
                        &add_response,
                        Id::unique(("custom_modal_branch_add", draft_id, branch_salt)),
                        draft_id,
                        existing_custom_blocks,
                    ) {
                        let allocated_id = allocate_draft_block_id(next_block_id);
                        branch_blocks.push(StatementBlock {
                            id: allocated_id,
                            kind: new_kind,
                        });
                        *total_steps = total_steps.saturating_add(1);
                    }
                });
            });
            if !branch_blocks.is_empty() {
                ui.add_space(4.0);
                render_modal_step_slice(
                    ui,
                    branch_blocks,
                    next_block_id,
                    total_steps,
                    draft_id,
                    existing_custom_blocks,
                    child_depth,
                );
            }
        });
}

fn render_add_step_popup(
    trigger_response: &egui::Response,
    popup_id: Id,
    draft_id: &str,
    existing_custom_blocks: &[CustomBlockDefinition],
) -> Option<StatementKind> {
    let mut selected_kind: Option<StatementKind> = None;
    let popup_frame = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .shadow(
            trigger_response
                .ctx
                .style_of(Theme::Dark)
                .visuals
                .popup_shadow,
        )
        .inner_margin(Margin::same(8));

    Popup::from_toggle_button_response(trigger_response)
        .id(popup_id)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .width(ADD_STEP_POPUP_WIDTH_PX)
        .frame(popup_frame)
        .show(|ui| {
            ScrollArea::vertical()
                .max_height(ADD_STEP_POPUP_MAX_HEIGHT_PX)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let categories: [(&str, BlockCategory); 4] = [
                        ("Actions", BlockCategory::Action),
                        ("Timing", BlockCategory::Timing),
                        ("Control", BlockCategory::Control),
                        ("Variables", BlockCategory::Variable),
                    ];
                    for (group_index, (group_title, category)) in categories.iter().enumerate() {
                        if group_index > 0 {
                            ui.add_space(6.0);
                        }
                        ui.horizontal(|ui| {
                            paint_status_dot(ui, category_color(*category), 3.5);
                            ui.label(rich_semibold(*group_title, FONT_SIZE_SMALL_PX, TEXT_MUTED));
                        });
                        for template in PALETTE_TEMPLATES
                            .iter()
                            .filter(|entry| entry.category == *category)
                        {
                            let item_button = Button::new(rich_semibold(
                                template.title,
                                FONT_SIZE_SMALL_PX,
                                TEXT_PRIMARY,
                            ))
                            .frame(false)
                            .min_size(vec2(ui.available_width(), 22.0));
                            if ui
                                .add(item_button)
                                .on_hover_text(template.summary)
                                .clicked()
                            {
                                selected_kind = Some(build_default_statement_kind(template.id));
                                ui.close();
                            }
                        }
                    }

                    let callable_tiles: Vec<&CustomBlockDefinition> = existing_custom_blocks
                        .iter()
                        .filter(|tile| tile.id != draft_id)
                        .collect();
                    if !callable_tiles.is_empty() {
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            paint_status_dot(ui, category_color(BlockCategory::Custom), 3.5);
                            ui.label(rich_semibold("My Blocks", FONT_SIZE_SMALL_PX, TEXT_MUTED));
                        });
                        for custom_tile in callable_tiles {
                            let label = format!("★ {}", custom_tile.name);
                            let item_button =
                                Button::new(rich_semibold(label, FONT_SIZE_SMALL_PX, TEXT_PRIMARY))
                                    .frame(false)
                                    .min_size(vec2(ui.available_width(), 22.0));
                            if ui.add(item_button).clicked() {
                                selected_kind = Some(StatementKind::RunCustomBlock {
                                    custom_block_id: custom_tile.id.clone(),
                                    cached_name: custom_tile.name.clone(),
                                });
                                ui.close();
                            }
                        }
                    }
                });
        });

    selected_kind
}

fn render_step_order_button(
    ui: &mut Ui,
    symbol: &str,
    ink_color: Color32,
    is_enabled: bool,
    tooltip: &str,
) -> bool {
    let display_color = if is_enabled { ink_color } else { TEXT_MUTED };
    let button = Button::new(rich_mono_bold(symbol, FONT_SIZE_SMALL_PX, display_color))
        .fill(Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 72))
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .min_size(vec2(
            STEP_CONTROL_BUTTON_SIZE_PX,
            STEP_CONTROL_BUTTON_SIZE_PX,
        ));
    ui.add_enabled(is_enabled, button)
        .on_hover_text(tooltip)
        .clicked()
}

fn resolve_step_card_colors(
    statement_kind: &StatementKind,
    existing_custom_blocks: &[CustomBlockDefinition],
) -> BlockVisualColors {
    if let StatementKind::RunCustomBlock {
        custom_block_id, ..
    } = statement_kind
        && let Some(definition) = lookup_custom_block(existing_custom_blocks, custom_block_id)
    {
        return custom_block_visual_colors(definition.color);
    }
    block_visual_colors(statement_kind.category())
}

fn apply_step_slice_mutation(
    statements: &mut Vec<StatementBlock>,
    mutation: Option<StepSliceMutation>,
) {
    let Some(action) = mutation else {
        return;
    };
    match action {
        StepSliceMutation::MoveUp(index) if index > 0 && index < statements.len() => {
            statements.swap(index, index.saturating_sub(1));
        }
        StepSliceMutation::MoveDown(index) if index.saturating_add(1) < statements.len() => {
            statements.swap(index, index.saturating_add(1));
        }
        StepSliceMutation::Delete(index) if index < statements.len() => {
            statements.remove(index);
        }
        _ => {}
    }
}

fn allocate_draft_block_id(next_block_id: &mut u64) -> BlockId {
    let current_id = (*next_block_id).max(1);
    *next_block_id = current_id.saturating_add(1);
    BlockId(current_id)
}
