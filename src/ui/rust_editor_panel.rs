use std::sync::Arc;
use std::time::Duration;

use egui::text::LayoutJob;
use egui::{
    Align, Button, Color32, CornerRadius, FontId, Frame, Galley, Id, Label, Layout, Margin,
    RichText, ScrollArea, Sense, Stroke, TextBuffer, TextEdit, TextFormat, Ui, pos2, vec2,
};

use crate::models::{
    CompileResultSnapshot, CompilerDiagnostic, DiagnosticSeverity, ScriptDocument,
    ScriptExecutionPhase, ValidationReport,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_SM_PX,
    FONT_SIZE_MONOSPACE_PX, FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_DANGER_INK,
    STATUS_DANGER_SOLID, STATUS_DANGER_TINT, STATUS_INFO_INK, STATUS_RUNNING_INK, SURFACE_CODE,
    SURFACE_ELEVATED, SURFACE_PANEL, SYNTAX_COMMENT, SYNTAX_KEYWORD, SYNTAX_NUMBER, SYNTAX_STRING,
    SYNTAX_TYPE, TEXT_MUTED, TEXT_PRIMARY, paint_status_dot, render_badge_pill, rich_mono,
    rich_semibold,
};

const DIAGNOSTICS_EXPANDED_HEIGHT_PX: f32 = 148.0;
const DIAGNOSTICS_EXPANDED_HEADER_OVERHEAD_PX: f32 = 48.0;
const DIAGNOSTICS_STRIP_HEIGHT_PX: f32 = 32.0;
const EDITOR_VERTICAL_CHROME_OVERHEAD_PX: f32 = 34.0;
const MIN_EDITOR_HEIGHT_PX: f32 = 200.0;
const MIN_EDITOR_WIDTH_PX: f32 = 320.0;
const MIN_MONO_GLYPH_ADVANCE_PX: f32 = 7.5;
const EDITOR_HORIZONTAL_TAIL_PADDING_PX: f32 = 40.0;
const GUTTER_DIGIT_WIDTH_PX: f32 = 8.5;
const GUTTER_HORIZONTAL_PADDING_PX: f32 = 12.0;
const MIN_GUTTER_WIDTH_PX: f32 = 42.0;
const EDITOR_CHROME_RESERVED_WIDTH_PX: f32 = 34.0;
const STARTER_CONFIRM_TIMEOUT_SECONDS: f64 = 4.0;
const STARTER_CONFIRM_REPAINT_INTERVAL_MS: u64 = 250;
const STATUS_DOT_RADIUS_PX: f32 = 4.0;
const CHECKMARK_ICON_SIZE_PX: f32 = 12.0;
const DESIRED_EDITOR_ROWS_COUNT: usize = 20;

const RUST_KEYWORDS: [&str; 27] = [
    "as", "break", "const", "continue", "crate", "else", "enum", "false", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "mut", "pub", "ref", "return", "self", "struct",
    "true", "use", "where", "while",
];

const RUST_SDK_TYPES: [&str; 18] = [
    "ScriptContext",
    "ScriptResult",
    "GaussianRangeMs",
    "MouseButton",
    "KeyCode",
    "InputTrigger",
    "Ok",
    "Err",
    "Some",
    "None",
    "Option",
    "Result",
    "String",
    "Vec",
    "u32",
    "u64",
    "i32",
    "f64",
];

const SNIPPET_CATALOG: [(&str, &str); 4] = [
    (
        "While Held",
        "\n    if script_context.is_mouse_pressed(MouseButton::Left)? {\n        script_context.mouse_click(MouseButton::Left, 12)?;\n    }\n",
    ),
    (
        "Gaussian Wait",
        "\n    let jitter_range_ms = GaussianRangeMs::new(10.0, 20.0, 15.0, 2.5)?;\n    script_context.wait_base_plus_gaussian_ms(45, jitter_range_ms)?;\n",
    ),
    (
        "Mouse Click",
        "\n    script_context.mouse_click(MouseButton::Left, 12)?;\n",
    ),
    (
        "Type Text",
        "\n    script_context.type_text(\"hello\", 35, 10)?;\n",
    ),
];

/// Actions emitted by the Advanced Rust editor panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustEditorAction {
    MarkDirty,
    CompileActiveScript,
}

/// Renders the syntax-highlighted Advanced Rust code editor, SDK snippets, and diagnostics.
pub fn render_rust_editor_panel(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    validation_report: &ValidationReport,
    runtime_phase: ScriptExecutionPhase,
    compile_result: Option<&CompileResultSnapshot>,
    emitted_actions: &mut Vec<RustEditorAction>,
) {
    let is_valid = validation_report.is_valid();
    let is_compiling = runtime_phase == ScriptExecutionPhase::Compiling;
    let starter_confirm_id = Id::unique(("rust_starter_template_confirm_at", script.id.as_str()));

    render_editor_toolbar(
        ui,
        script,
        starter_confirm_id,
        is_valid,
        is_compiling,
        validation_report,
        emitted_actions,
    );
    ui.add_space(6.0);

    let should_expand_diagnostics =
        !is_compiling && compile_result.is_some_and(|result| !result.diagnostics.is_empty());
    let diagnostics_height_px = if should_expand_diagnostics {
        DIAGNOSTICS_EXPANDED_HEIGHT_PX
    } else {
        DIAGNOSTICS_STRIP_HEIGHT_PX
    };
    let editor_height_px =
        (ui.available_height() - diagnostics_height_px - EDITOR_VERTICAL_CHROME_OVERHEAD_PX)
            .max(MIN_EDITOR_HEIGHT_PX);

    let line_count = script.rust_source.split('\n').count().max(1);
    if render_code_editor_area(ui, script, line_count, editor_height_px) {
        clear_starter_confirm_state(ui.ctx(), starter_confirm_id);
        emitted_actions.push(RustEditorAction::MarkDirty);
    }

    ui.add_space(6.0);
    if should_expand_diagnostics {
        if let Some(result) = compile_result {
            render_expanded_diagnostics_pane(ui, result, line_count);
        }
    } else {
        render_compact_diagnostics_strip(ui, is_compiling, compile_result, line_count);
    }
}

fn is_starter_confirm_armed(
    egui_context: &egui::Context,
    confirm_id: Id,
    now_seconds: f64,
) -> bool {
    let Some(armed_at_seconds) = egui_context.data(|data| data.get_temp::<f64>(confirm_id)) else {
        return false;
    };
    let elapsed_seconds = now_seconds - armed_at_seconds;
    if (0.0..STARTER_CONFIRM_TIMEOUT_SECONDS).contains(&elapsed_seconds) {
        egui_context
            .request_repaint_after(Duration::from_millis(STARTER_CONFIRM_REPAINT_INTERVAL_MS));
        return true;
    }
    clear_starter_confirm_state(egui_context, confirm_id);
    false
}

fn clear_starter_confirm_state(egui_context: &egui::Context, confirm_id: Id) {
    egui_context.data_mut(|data| data.remove::<f64>(confirm_id));
}

fn render_editor_toolbar(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    starter_confirm_id: Id,
    is_valid: bool,
    is_compiling: bool,
    validation_report: &ValidationReport,
    emitted_actions: &mut Vec<RustEditorAction>,
) {
    Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(rich_semibold(
                    "SDK Snippets:",
                    FONT_SIZE_SMALL_PX,
                    TEXT_MUTED,
                ));
                for (snippet_label, snippet_code) in SNIPPET_CATALOG {
                    if render_snippet_button(ui, snippet_label) {
                        insert_snippet_into_source(&mut script.rust_source, snippet_code);
                        clear_starter_confirm_state(ui.ctx(), starter_confirm_id);
                        emitted_actions.push(RustEditorAction::MarkDirty);
                    }
                }

                ui.separator();
                render_starter_template_controls(ui, script, starter_confirm_id, emitted_actions);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    render_compile_action_button(
                        ui,
                        is_valid,
                        is_compiling,
                        validation_report,
                        emitted_actions,
                    );
                });
            });
        });
}

fn render_starter_template_controls(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    starter_confirm_id: Id,
    emitted_actions: &mut Vec<RustEditorAction>,
) {
    let now_seconds = ui.input(|input_state| input_state.time);
    if !is_starter_confirm_armed(ui.ctx(), starter_confirm_id, now_seconds) {
        let starter_button =
            Button::new(RichText::new("Starter Template").size(FONT_SIZE_SMALL_PX))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE));
        if ui
            .add(starter_button)
            .on_hover_text("Replace editor buffer with default clickforge_sdk starter code")
            .clicked()
        {
            ui.ctx()
                .data_mut(|data| data.insert_temp(starter_confirm_id, now_seconds));
            ui.ctx()
                .request_repaint_after(Duration::from_millis(STARTER_CONFIRM_REPAINT_INTERVAL_MS));
        }
        return;
    }

    let confirm_button = Button::new(rich_semibold(
        "Confirm Overwrite?",
        FONT_SIZE_SMALL_PX,
        Color32::WHITE,
    ))
    .fill(STATUS_DANGER_SOLID)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));
    if ui
        .add(confirm_button)
        .on_hover_text("Replaces all code in this editor with the starter template")
        .clicked()
    {
        script.rust_source = ScriptDocument::default_rust_starter_source();
        clear_starter_confirm_state(ui.ctx(), starter_confirm_id);
        emitted_actions.push(RustEditorAction::MarkDirty);
    }

    let cancel_button = Button::new(
        RichText::new("Cancel")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, BORDER_SUBTLE));
    if ui.add(cancel_button).clicked() {
        clear_starter_confirm_state(ui.ctx(), starter_confirm_id);
    }
}

fn render_compile_action_button(
    ui: &mut Ui,
    is_valid: bool,
    is_compiling: bool,
    validation_report: &ValidationReport,
    emitted_actions: &mut Vec<RustEditorAction>,
) {
    let compile_label = if is_compiling {
        "Compiling..."
    } else {
        "Compile & Check"
    };
    let compile_button = Button::new(rich_semibold(
        compile_label,
        FONT_SIZE_SMALL_PX,
        INK_ON_ACCENT,
    ))
    .fill(ACCENT_PRIMARY)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));

    let response = ui.add_enabled(is_valid && !is_compiling, compile_button);
    if response.clicked() {
        emitted_actions.push(RustEditorAction::CompileActiveScript);
    }
    if !is_valid {
        let first_reason = validation_report
            .issues
            .first()
            .map(|issue| issue.message.as_str())
            .unwrap_or("Fix validation errors first.");
        response.on_disabled_hover_text(format!("Compile blocked: {first_reason}"));
    }
}

fn render_snippet_button(ui: &mut Ui, label_text: &str) -> bool {
    let snippet_button = Button::new(
        RichText::new(format!("+ {label_text}"))
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_PRIMARY),
    )
    .fill(SURFACE_ELEVATED)
    .stroke(Stroke::new(1.0, BORDER_STRONG))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));
    ui.add(snippet_button).clicked()
}

fn insert_snippet_into_source(rust_source: &mut String, snippet_text: &str) {
    if let Some(return_offset_bytes) = rust_source.rfind("Ok(())") {
        rust_source.insert_str(return_offset_bytes, snippet_text);
        return;
    }
    rust_source.push_str(snippet_text);
}

fn render_code_editor_area(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    line_count: usize,
    editor_height_px: f32,
) -> bool {
    let mut did_change = false;
    let mono_font = FontId::monospace(FONT_SIZE_MONOSPACE_PX);
    let line_height_px =
        ui.fonts_mut(|fonts| fonts.row_height(&mono_font)) + ui.spacing().extra_text_line_spacing;
    let digit_width = line_count.to_string().len().max(4);
    let gutter_width_px = ((digit_width as f32) * GUTTER_DIGIT_WIDTH_PX
        + GUTTER_HORIZONTAL_PADDING_PX)
        .max(MIN_GUTTER_WIDTH_PX);
    let viewport_width_px =
        (ui.available_width() - gutter_width_px - EDITOR_CHROME_RESERVED_WIDTH_PX)
            .max(MIN_EDITOR_WIDTH_PX);
    let longest_line_chars = script
        .rust_source
        .split('\n')
        .map(|line_text| line_text.chars().count())
        .max()
        .unwrap_or(0);
    let glyph_advance_px = ui
        .fonts_mut(|fonts| fonts.glyph_width(&mono_font, 'M'))
        .max(MIN_MONO_GLYPH_ADVANCE_PX);
    let editor_target_width_px = viewport_width_px
        .max((longest_line_chars as f32) * glyph_advance_px + EDITOR_HORIZONTAL_TAIL_PADDING_PX);

    Frame::NONE
        .fill(SURFACE_CODE)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ScrollArea::both()
                .id_salt(("rust_code_editor_scroll", script.id.as_str()))
                .max_height(editor_height_px)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        render_line_number_gutter(
                            ui,
                            line_count,
                            digit_width,
                            gutter_width_px,
                            &mono_font,
                            line_height_px,
                        );
                        ui.separator();

                        let mut layouter =
                            |editor_ui: &Ui,
                             source_buffer: &dyn TextBuffer,
                             _wrap_width_px: f32| {
                                layout_highlighted_rust_source(
                                    editor_ui,
                                    source_buffer.as_str(),
                                    &mono_font,
                                    line_height_px,
                                )
                            };

                        let editor_response = ui.add(
                            TextEdit::multiline(&mut script.rust_source)
                                .id_salt(("rust_source_input", script.id.as_str()))
                                .font(mono_font.clone())
                                .code_editor()
                                .desired_rows(DESIRED_EDITOR_ROWS_COUNT)
                                .desired_width(editor_target_width_px)
                                .frame(Frame::NONE)
                                .layouter(&mut layouter),
                        );
                        if editor_response.changed() {
                            did_change = true;
                        }
                    });
                });
        });

    did_change
}

fn render_line_number_gutter(
    ui: &mut Ui,
    line_count: usize,
    digit_width: usize,
    gutter_width_px: f32,
    mono_font: &FontId,
    line_height_px: f32,
) {
    let mut line_numbers_text = String::with_capacity(line_count.saturating_mul(digit_width + 1));
    for line_number in 1..=line_count {
        if line_number > 1 {
            line_numbers_text.push('\n');
        }
        line_numbers_text.push_str(&format!("{line_number:>width$}", width = digit_width));
    }

    let mut gutter_job = LayoutJob::default();
    gutter_job.wrap.max_width = f32::INFINITY;
    gutter_job.append(
        &line_numbers_text,
        0.0,
        TextFormat {
            font_id: mono_font.clone(),
            color: TEXT_MUTED,
            line_height: Some(line_height_px),
            ..Default::default()
        },
    );

    let min_gutter_height_px = (line_count as f32) * line_height_px;
    ui.allocate_ui_with_layout(
        vec2(gutter_width_px, min_gutter_height_px),
        Layout::top_down(Align::Max),
        |ui| {
            ui.add(Label::new(gutter_job).selectable(false));
        },
    );
}

fn render_diagnostics_container(ui: &mut Ui, render_contents: impl FnOnce(&mut Ui)) {
    Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            render_contents(ui);
        });
}

fn render_compact_diagnostics_strip(
    ui: &mut Ui,
    is_compiling: bool,
    compile_result: Option<&CompileResultSnapshot>,
    line_count: usize,
) {
    render_diagnostics_container(ui, |ui| {
        ui.set_min_height(DIAGNOSTICS_STRIP_HEIGHT_PX - 12.0);
        ui.horizontal(|ui| {
            if is_compiling {
                paint_status_dot(ui, STATUS_INFO_INK, STATUS_DOT_RADIUS_PX);
                ui.label(rich_semibold(
                    "Compiling...",
                    FONT_SIZE_SMALL_PX,
                    STATUS_INFO_INK,
                ));
                ui.label(
                    RichText::new("Running cargo check in isolated workspace")
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                );
            } else if let Some(result) = compile_result {
                if result.is_success {
                    paint_clean_checkmark_icon(ui, STATUS_RUNNING_INK);
                    ui.label(rich_semibold(
                        format!("Compiled cleanly in {} ms", result.duration_ms),
                        FONT_SIZE_SMALL_PX,
                        STATUS_RUNNING_INK,
                    ));
                    ui.label(
                        RichText::new("Zero warnings, zero errors")
                            .size(FONT_SIZE_SMALL_PX)
                            .color(TEXT_MUTED),
                    );
                } else {
                    paint_status_dot(ui, STATUS_DANGER_INK, STATUS_DOT_RADIUS_PX);
                    ui.label(rich_semibold(
                        format!("Compilation failed in {} ms", result.duration_ms),
                        FONT_SIZE_SMALL_PX,
                        STATUS_DANGER_INK,
                    ));
                }
            } else {
                paint_status_dot(ui, TEXT_MUTED, STATUS_DOT_RADIUS_PX);
                ui.label(rich_semibold(
                    "Ready to compile",
                    FONT_SIZE_SMALL_PX,
                    TEXT_PRIMARY,
                ));
                ui.label(
                    RichText::new("Click Compile & Check to verify this Rust script")
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                );
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(rich_mono(
                    format!("{line_count} lines"),
                    FONT_SIZE_SMALL_PX,
                    TEXT_MUTED,
                ));
            });
        });
    });
}

fn paint_clean_checkmark_icon(ui: &mut Ui, stroke_color: Color32) {
    let (icon_rect, _) = ui.allocate_exact_size(
        vec2(CHECKMARK_ICON_SIZE_PX, CHECKMARK_ICON_SIZE_PX),
        Sense::hover(),
    );
    let stroke = Stroke::new(1.8, stroke_color);
    let left_point = pos2(
        icon_rect.min.x + icon_rect.width() * 0.16,
        icon_rect.min.y + icon_rect.height() * 0.54,
    );
    let middle_point = pos2(
        icon_rect.min.x + icon_rect.width() * 0.42,
        icon_rect.min.y + icon_rect.height() * 0.80,
    );
    let right_point = pos2(
        icon_rect.min.x + icon_rect.width() * 0.86,
        icon_rect.min.y + icon_rect.height() * 0.24,
    );
    let painter = ui.painter();
    painter.line_segment([left_point, middle_point], stroke);
    painter.line_segment([middle_point, right_point], stroke);
}

fn render_expanded_diagnostics_pane(
    ui: &mut Ui,
    compile_result: &CompileResultSnapshot,
    line_count: usize,
) {
    let error_count = compile_result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count();
    let warning_count = compile_result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .count();

    render_diagnostics_container(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(rich_semibold(
                "Compiler Diagnostics",
                FONT_SIZE_SMALL_PX,
                TEXT_PRIMARY,
            ));
            if error_count > 0 {
                render_badge_pill(
                    ui,
                    &format!("{error_count} error(s)"),
                    STATUS_DANGER_TINT,
                    STATUS_DANGER_INK,
                    STATUS_DANGER_INK,
                );
            }
            if warning_count > 0 {
                render_badge_pill(
                    ui,
                    &format!("{warning_count} warning(s)"),
                    ACCENT_PRIMARY_TINT,
                    ACCENT_PRIMARY,
                    ACCENT_PRIMARY,
                );
            }
            ui.label(rich_mono(
                format!("{} ms", compile_result.duration_ms),
                FONT_SIZE_SMALL_PX,
                TEXT_MUTED,
            ));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(rich_mono(
                    format!("{line_count} lines"),
                    FONT_SIZE_SMALL_PX,
                    TEXT_MUTED,
                ));
            });
        });

        ui.add_space(4.0);
        ScrollArea::vertical()
            .id_salt("rust_compiler_diagnostics_scroll")
            .max_height(
                (DIAGNOSTICS_EXPANDED_HEIGHT_PX - DIAGNOSTICS_EXPANDED_HEADER_OVERHEAD_PX)
                    .max(60.0),
            )
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for diagnostic in &compile_result.diagnostics {
                    render_single_diagnostic_row(ui, diagnostic);
                    ui.add_space(4.0);
                }
            });
    });
}

fn render_single_diagnostic_row(ui: &mut Ui, diagnostic: &CompilerDiagnostic) {
    let (severity_label, badge_fill, badge_ink, badge_border) = match diagnostic.severity {
        DiagnosticSeverity::Error => (
            "ERROR",
            STATUS_DANGER_TINT,
            STATUS_DANGER_INK,
            STATUS_DANGER_INK,
        ),
        DiagnosticSeverity::Warning => {
            ("WARN", ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY)
        }
        DiagnosticSeverity::Note => ("NOTE", SURFACE_ELEVATED, STATUS_INFO_INK, BORDER_SUBTLE),
    };

    ui.horizontal_wrapped(|ui| {
        render_badge_pill(ui, severity_label, badge_fill, badge_ink, badge_border);
        if let Some(line_number) = diagnostic.line_number {
            let location_text = match diagnostic.column_number {
                Some(column_number) => format!("L{line_number}:C{column_number}"),
                None => format!("L{line_number}"),
            };
            ui.label(rich_mono(location_text, FONT_SIZE_SMALL_PX, ACCENT_PRIMARY));
        }
        ui.label(
            RichText::new(&diagnostic.message)
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_PRIMARY),
        );
    });

    if let Some(excerpt) = &diagnostic.rendered_excerpt {
        let trimmed_excerpt = excerpt.trim();
        if !trimmed_excerpt.is_empty() {
            ui.label(rich_mono(trimmed_excerpt, FONT_SIZE_SMALL_PX, TEXT_MUTED));
        }
    }
}

fn layout_highlighted_rust_source(
    ui: &Ui,
    source_text: &str,
    mono_font: &FontId,
    line_height_px: f32,
) -> Arc<Galley> {
    let mut layout_job = LayoutJob::default();
    layout_job.wrap.max_width = f32::INFINITY;
    layout_job.keep_trailing_whitespace = true;

    let mut append_segment = |segment_text: &str, text_color: Color32| {
        layout_job.append(
            segment_text,
            0.0,
            TextFormat {
                font_id: mono_font.clone(),
                color: text_color,
                line_height: Some(line_height_px),
                ..Default::default()
            },
        );
    };

    let total_bytes = source_text.len();
    let mut cursor_byte: usize = 0;

    while cursor_byte < total_bytes {
        let current_slice = &source_text[cursor_byte..];
        let Some(first_character) = current_slice.chars().next() else {
            break;
        };

        let (token_len_bytes, token_color) = if current_slice.starts_with("//") {
            (find_line_end_offset_bytes(current_slice), SYNTAX_COMMENT)
        } else if first_character == '"' {
            (
                find_quoted_string_end_offset_bytes(current_slice),
                SYNTAX_STRING,
            )
        } else if first_character.is_ascii_alphabetic() || first_character == '_' {
            let ident_end_bytes = find_identifier_end_offset_bytes(current_slice);
            (
                ident_end_bytes,
                classify_identifier_color(&current_slice[..ident_end_bytes]),
            )
        } else if first_character.is_ascii_digit() {
            (find_number_end_offset_bytes(current_slice), SYNTAX_NUMBER)
        } else {
            (first_character.len_utf8(), TEXT_PRIMARY)
        };

        append_segment(&current_slice[..token_len_bytes], token_color);
        cursor_byte = cursor_byte.saturating_add(token_len_bytes);
    }

    ui.painter().layout_job(layout_job)
}

fn find_line_end_offset_bytes(remaining_slice: &str) -> usize {
    remaining_slice.find('\n').unwrap_or(remaining_slice.len())
}

fn find_quoted_string_end_offset_bytes(remaining_slice: &str) -> usize {
    let mut is_escaped = false;
    for (byte_offset, character) in remaining_slice.char_indices().skip(1) {
        if is_escaped {
            is_escaped = false;
            continue;
        }
        if character == '\\' {
            is_escaped = true;
            continue;
        }
        if character == '"' {
            return byte_offset.saturating_add(1);
        }
        if character == '\n' {
            return byte_offset;
        }
    }
    remaining_slice.len()
}

fn find_identifier_end_offset_bytes(remaining_slice: &str) -> usize {
    for (byte_offset, character) in remaining_slice.char_indices() {
        if !(character.is_ascii_alphanumeric() || character == '_') {
            return byte_offset;
        }
    }
    remaining_slice.len()
}

fn find_number_end_offset_bytes(remaining_slice: &str) -> usize {
    for (byte_offset, character) in remaining_slice.char_indices() {
        if !(character.is_ascii_digit() || character == '.' || character == '_') {
            return byte_offset;
        }
    }
    remaining_slice.len()
}

fn classify_identifier_color(identifier: &str) -> Color32 {
    if RUST_KEYWORDS.contains(&identifier) {
        return SYNTAX_KEYWORD;
    }
    if RUST_SDK_TYPES.contains(&identifier) {
        return SYNTAX_TYPE;
    }
    TEXT_PRIMARY
}
