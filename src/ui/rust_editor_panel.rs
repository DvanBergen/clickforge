use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{
    Align, Button, Color32, CornerRadius, FontId, Frame, Galley, Layout, Margin, RichText,
    ScrollArea, Stroke, TextBuffer, TextEdit, TextFormat, Ui,
};

use crate::models::{
    CompileResultSnapshot, CompilerDiagnostic, DiagnosticSeverity, ScriptDocument,
    ScriptExecutionPhase, ValidationReport,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, CATEGORY_ACTION,
    CORNER_RADIUS_SM_PX, FONT_SIZE_MONOSPACE_PX, FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_DANGER,
    STATUS_DANGER_TINT, STATUS_RUNNING, STATUS_RUNNING_TINT, SURFACE_CODE, SURFACE_ELEVATED,
    SURFACE_PANEL, SYNTAX_COMMENT, SYNTAX_KEYWORD, SYNTAX_NUMBER, SYNTAX_STRING, SYNTAX_TYPE,
    TEXT_MUTED, TEXT_PRIMARY, render_badge_pill,
};

const DIAGNOSTICS_SECTION_HEIGHT_PX: f32 = 148.0;
const MIN_EDITOR_HEIGHT_PX: f32 = 220.0;
const LINE_NUMBER_GUTTER_WIDTH_PX: f32 = 40.0;

const RUST_KEYWORDS: [&str; 25] = [
    "as", "break", "const", "continue", "crate", "else", "enum", "false", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "mut", "pub", "ref", "return", "self", "struct",
    "true", "use",
];

const RUST_EXTRA_KEYWORDS: [&str; 2] = ["while", "where"];

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

const SNIPPET_WHILE_HELD: &str = "\n    if script_context.is_mouse_pressed(MouseButton::Left)? {\n        script_context.mouse_click(MouseButton::Left, 12)?;\n    }\n";
const SNIPPET_GAUSSIAN_WAIT: &str = "\n    let jitter_range_ms = GaussianRangeMs::new(10.0, 20.0, 15.0, 2.5)?;\n    script_context.wait_base_plus_gaussian_ms(45, jitter_range_ms)?;\n";
const SNIPPET_MOUSE_CLICK: &str = "\n    script_context.mouse_click(MouseButton::Left, 12)?;\n";
const SNIPPET_TYPE_TEXT: &str = "\n    script_context.type_text(\"hello\", 35, 10)?;\n";

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

    render_editor_toolbar(
        ui,
        script,
        is_valid,
        is_compiling,
        validation_report,
        emitted_actions,
    );
    ui.add_space(6.0);

    let editor_height_px =
        (ui.available_height() - DIAGNOSTICS_SECTION_HEIGHT_PX - 12.0).max(MIN_EDITOR_HEIGHT_PX);

    render_code_editor_area(ui, script, editor_height_px, emitted_actions);
    ui.add_space(6.0);
    render_compiler_diagnostics_pane(ui, is_compiling, compile_result);
}

fn render_editor_toolbar(
    ui: &mut Ui,
    script: &mut ScriptDocument,
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
                ui.label(
                    RichText::new("SDK Snippets:")
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                );
                if render_snippet_button(ui, "While Held") {
                    insert_snippet_into_source(&mut script.rust_source, SNIPPET_WHILE_HELD);
                    emitted_actions.push(RustEditorAction::MarkDirty);
                }
                if render_snippet_button(ui, "Gaussian Wait") {
                    insert_snippet_into_source(&mut script.rust_source, SNIPPET_GAUSSIAN_WAIT);
                    emitted_actions.push(RustEditorAction::MarkDirty);
                }
                if render_snippet_button(ui, "Mouse Click") {
                    insert_snippet_into_source(&mut script.rust_source, SNIPPET_MOUSE_CLICK);
                    emitted_actions.push(RustEditorAction::MarkDirty);
                }
                if render_snippet_button(ui, "Type Text") {
                    insert_snippet_into_source(&mut script.rust_source, SNIPPET_TYPE_TEXT);
                    emitted_actions.push(RustEditorAction::MarkDirty);
                }

                ui.separator();

                if ui
                    .add(
                        Button::new(RichText::new("Starter Template").size(FONT_SIZE_SMALL_PX))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
                    )
                    .on_hover_text("Replace editor buffer with default clickforge_sdk starter code")
                    .clicked()
                {
                    script.rust_source = ScriptDocument::default_rust_starter_source();
                    emitted_actions.push(RustEditorAction::MarkDirty);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let compile_label = if is_compiling {
                        "Compiling..."
                    } else {
                        "Compile & Check"
                    };
                    let compile_button = Button::new(
                        RichText::new(compile_label)
                            .size(FONT_SIZE_SMALL_PX)
                            .strong()
                            .color(INK_ON_ACCENT),
                    )
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
                });
            });
        });
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
    if let Some(return_pos) = rust_source.rfind("Ok(())") {
        rust_source.insert_str(return_pos, snippet_text);
        return;
    }
    rust_source.push_str(snippet_text);
}

fn render_code_editor_area(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    editor_height_px: f32,
    emitted_actions: &mut Vec<RustEditorAction>,
) {
    Frame::NONE
        .fill(SURFACE_CODE)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt(("rust_code_editor_scroll", script.id.as_str()))
                .max_height(editor_height_px)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let line_count = script.rust_source.lines().count().max(1);
                    ui.horizontal_top(|ui| {
                        render_line_number_gutter(ui, line_count);
                        ui.separator();

                        let mut layouter =
                            |editor_ui: &Ui, source_buffer: &dyn TextBuffer, wrap_width_px: f32| {
                                layout_highlighted_rust_source(
                                    editor_ui,
                                    source_buffer.as_str(),
                                    wrap_width_px,
                                )
                            };

                        let editor_response = ui.add(
                            TextEdit::multiline(&mut script.rust_source)
                                .id_salt(("rust_source_input", script.id.as_str()))
                                .font(FontId::monospace(FONT_SIZE_MONOSPACE_PX))
                                .code_editor()
                                .desired_rows(20)
                                .desired_width(ui.available_width())
                                .frame(Frame::NONE)
                                .layouter(&mut layouter),
                        );
                        if editor_response.changed() {
                            emitted_actions.push(RustEditorAction::MarkDirty);
                        }
                    });
                });
        });
}

fn render_line_number_gutter(ui: &mut Ui, line_count: usize) {
    let mut line_numbers_text = String::with_capacity(line_count.saturating_mul(5));
    for line_number in 1..=line_count {
        if line_number > 1 {
            line_numbers_text.push('\n');
        }
        line_numbers_text.push_str(&format!("{line_number:>4}"));
    }

    ui.allocate_ui_with_layout(
        egui::vec2(LINE_NUMBER_GUTTER_WIDTH_PX, ui.available_height()),
        Layout::top_down(Align::Max),
        |ui| {
            ui.label(
                RichText::new(line_numbers_text)
                    .font(FontId::monospace(FONT_SIZE_MONOSPACE_PX))
                    .color(TEXT_MUTED),
            );
        },
    );
}

fn render_compiler_diagnostics_pane(
    ui: &mut Ui,
    is_compiling: bool,
    compile_result: Option<&CompileResultSnapshot>,
) {
    Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Compiler Diagnostics")
                        .size(FONT_SIZE_SMALL_PX)
                        .strong()
                        .color(TEXT_MUTED),
                );
                if is_compiling {
                    render_badge_pill(
                        ui,
                        "Building with cargo...",
                        SURFACE_ELEVATED,
                        CATEGORY_ACTION,
                        CATEGORY_ACTION,
                    );
                } else if let Some(result) = compile_result {
                    if result.is_success {
                        render_badge_pill(
                            ui,
                            &format!("Compiled in {} ms", result.duration_ms),
                            STATUS_RUNNING_TINT,
                            STATUS_RUNNING,
                            STATUS_RUNNING,
                        );
                    } else {
                        render_badge_pill(
                            ui,
                            &format!(
                                "Failed ({} diagnostic(s), {} ms)",
                                result.diagnostics.len(),
                                result.duration_ms
                            ),
                            STATUS_DANGER_TINT,
                            STATUS_DANGER,
                            STATUS_DANGER,
                        );
                    }
                } else {
                    ui.label(
                        RichText::new("Click Compile & Check to verify this Rust script")
                            .size(FONT_SIZE_SMALL_PX)
                            .color(TEXT_MUTED),
                    );
                }
            });

            ui.add_space(4.0);

            ScrollArea::vertical()
                .id_salt("rust_compiler_diagnostics_scroll")
                .max_height(DIAGNOSTICS_SECTION_HEIGHT_PX - 36.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let Some(result) = compile_result else {
                        return;
                    };
                    if result.diagnostics.is_empty() {
                        if result.is_success {
                            ui.label(
                                RichText::new("Zero warnings and zero errors. Binary ready for isolated execution.")
                                    .size(FONT_SIZE_SMALL_PX)
                                    .color(STATUS_RUNNING),
                            );
                        }
                        return;
                    }
                    for diagnostic in &result.diagnostics {
                        render_single_diagnostic_row(ui, diagnostic);
                        ui.add_space(4.0);
                    }
                });
        });
}

fn render_single_diagnostic_row(ui: &mut Ui, diagnostic: &CompilerDiagnostic) {
    let (severity_label, badge_fill, badge_ink, badge_border) = match diagnostic.severity {
        DiagnosticSeverity::Error => ("ERROR", STATUS_DANGER_TINT, STATUS_DANGER, STATUS_DANGER),
        DiagnosticSeverity::Warning => {
            ("WARN", ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY)
        }
        DiagnosticSeverity::Note => ("NOTE", SURFACE_ELEVATED, TEXT_MUTED, BORDER_SUBTLE),
    };

    ui.horizontal_wrapped(|ui| {
        render_badge_pill(ui, severity_label, badge_fill, badge_ink, badge_border);
        if let Some(line_number) = diagnostic.line_number {
            let location_text = match diagnostic.column_number {
                Some(column_number) => format!("L{line_number}:C{column_number}"),
                None => format!("L{line_number}"),
            };
            ui.label(
                RichText::new(location_text)
                    .size(FONT_SIZE_SMALL_PX)
                    .monospace()
                    .color(ACCENT_PRIMARY),
            );
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
            ui.label(
                RichText::new(trimmed_excerpt)
                    .size(FONT_SIZE_SMALL_PX)
                    .monospace()
                    .color(TEXT_MUTED),
            );
        }
    }
}

fn layout_highlighted_rust_source(ui: &Ui, source_text: &str, wrap_width_px: f32) -> Arc<Galley> {
    let mut layout_job = LayoutJob::default();
    layout_job.wrap.max_width = wrap_width_px;

    let mono_font = FontId::monospace(FONT_SIZE_MONOSPACE_PX);
    let source_bytes = source_text.as_bytes();
    let total_bytes = source_bytes.len();
    let mut cursor_byte: usize = 0;

    while cursor_byte < total_bytes {
        let current_slice = &source_text[cursor_byte..];
        let Some(first_char) = current_slice.chars().next() else {
            break;
        };

        if current_slice.starts_with("//") {
            let comment_end = find_line_end_offset(current_slice);
            append_highlighted_segment(
                &mut layout_job,
                &current_slice[..comment_end],
                &mono_font,
                SYNTAX_COMMENT,
            );
            cursor_byte = cursor_byte.saturating_add(comment_end);
            continue;
        }

        if first_char == '"' {
            let string_end = find_quoted_string_end_offset(current_slice);
            append_highlighted_segment(
                &mut layout_job,
                &current_slice[..string_end],
                &mono_font,
                SYNTAX_STRING,
            );
            cursor_byte = cursor_byte.saturating_add(string_end);
            continue;
        }

        if first_char.is_ascii_alphabetic() || first_char == '_' {
            let ident_end = find_identifier_end_offset(current_slice);
            let token_text = &current_slice[..ident_end];
            let token_color = classify_identifier_color(token_text);
            append_highlighted_segment(&mut layout_job, token_text, &mono_font, token_color);
            cursor_byte = cursor_byte.saturating_add(ident_end);
            continue;
        }

        if first_char.is_ascii_digit() {
            let number_end = find_number_end_offset(current_slice);
            append_highlighted_segment(
                &mut layout_job,
                &current_slice[..number_end],
                &mono_font,
                SYNTAX_NUMBER,
            );
            cursor_byte = cursor_byte.saturating_add(number_end);
            continue;
        }

        let char_len_bytes = first_char.len_utf8();
        append_highlighted_segment(
            &mut layout_job,
            &current_slice[..char_len_bytes],
            &mono_font,
            TEXT_PRIMARY,
        );
        cursor_byte = cursor_byte.saturating_add(char_len_bytes);
    }

    ui.painter().layout_job(layout_job)
}

fn append_highlighted_segment(
    layout_job: &mut LayoutJob,
    segment_text: &str,
    font_id: &FontId,
    text_color: Color32,
) {
    layout_job.append(
        segment_text,
        0.0,
        TextFormat {
            font_id: font_id.clone(),
            color: text_color,
            ..Default::default()
        },
    );
}

fn find_line_end_offset(remaining_slice: &str) -> usize {
    remaining_slice.find('\n').unwrap_or(remaining_slice.len())
}

fn find_quoted_string_end_offset(remaining_slice: &str) -> usize {
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

fn find_identifier_end_offset(remaining_slice: &str) -> usize {
    for (byte_offset, character) in remaining_slice.char_indices() {
        if !(character.is_ascii_alphanumeric() || character == '_') {
            return byte_offset;
        }
    }
    remaining_slice.len()
}

fn find_number_end_offset(remaining_slice: &str) -> usize {
    for (byte_offset, character) in remaining_slice.char_indices() {
        if !(character.is_ascii_digit() || character == '.' || character == '_') {
            return byte_offset;
        }
    }
    remaining_slice.len()
}

fn classify_identifier_color(identifier: &str) -> Color32 {
    if RUST_KEYWORDS.contains(&identifier) || RUST_EXTRA_KEYWORDS.contains(&identifier) {
        return SYNTAX_KEYWORD;
    }
    if RUST_SDK_TYPES.contains(&identifier) {
        return SYNTAX_TYPE;
    }
    TEXT_PRIMARY
}
