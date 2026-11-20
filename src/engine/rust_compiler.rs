use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use rand::RngExt;
use serde_json::Value;

use crate::models::{
    CompileResultSnapshot, CompilerDiagnostic, DiagnosticSeverity, ScriptDocument, ValidationError,
    validate_script_for_persistence_and_execution,
};

const SCRIPT_PACKAGE_NAME: &str = "clickforge_user_script";
const MAX_COMPILE_DIAGNOSTICS: usize = 100;
const MAX_ANCESTOR_SEARCH_DEPTH: usize = 6;

const WRAPPER_HEADER_LINES: [&str; 8] = [
    "fn main() {",
    "    if let Err(script_error) = clickforge_sdk::execute_script_entrypoint(user_script::run_script) {",
    "        eprintln!(\"ClickForge script error: {script_error}\");",
    "        std::process::exit(1);",
    "    }",
    "}",
    "",
    "mod user_script {",
];

static COMPILE_CACHE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

/// Returns the default pre-warmed Cargo cache directory under the user's local data directory.
pub fn default_script_build_cache_dir() -> PathBuf {
    if let Some(local_data_directory) = dirs::data_local_dir() {
        return local_data_directory
            .join("clickforge")
            .join("script_build_cache");
    }
    std::env::temp_dir().join("clickforge_script_build_cache")
}

/// Resolves the local filesystem path to `crates/clickforge_sdk` for Cargo path dependencies.
pub fn resolve_clickforge_sdk_path() -> Result<PathBuf, String> {
    let manifest_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("clickforge_sdk");
    if is_valid_sdk_directory(&manifest_candidate) {
        return Ok(canonicalize_or_keep(manifest_candidate));
    }

    if let Ok(current_exe_path) = std::env::current_exe()
        && let Some(found_path) = search_ancestors_for_sdk(&current_exe_path)
    {
        return Ok(canonicalize_or_keep(found_path));
    }

    if let Ok(current_directory) = std::env::current_dir()
        && let Some(found_path) = search_ancestors_for_sdk(&current_directory)
    {
        return Ok(canonicalize_or_keep(found_path));
    }

    Err(
        "Unable to locate crates/clickforge_sdk near CARGO_MANIFEST_DIR, executable, or working directory."
            .to_string(),
    )
}

/// Validates and compiles an Advanced Rust script using the pre-warmed Cargo workspace cache.
pub fn compile_rust_script(script: &ScriptDocument, cache_dir: &Path) -> CompileResultSnapshot {
    let started_at = Instant::now();
    if let Err(validation_error) = validate_script_for_persistence_and_execution(script) {
        return build_validation_failure_snapshot(&script.id, validation_error, started_at);
    }

    let cache_mutex = COMPILE_CACHE_MUTEX.get_or_init(|| Mutex::new(()));
    let _cache_lock = match cache_mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned_guard) => poisoned_guard.into_inner(),
    };

    let sdk_path = match resolve_clickforge_sdk_path() {
        Ok(resolved_path) => resolved_path,
        Err(resolve_error) => {
            return build_single_error_snapshot(&script.id, resolve_error, started_at);
        }
    };

    let project_directory = cache_dir.join("project");
    let binaries_directory = cache_dir.join("binaries");
    if let Err(prepare_error) = prepare_cached_cargo_project(
        &project_directory,
        &binaries_directory,
        &sdk_path,
        &script.rust_source,
    ) {
        return build_single_error_snapshot(&script.id, prepare_error, started_at);
    }

    let manifest_path = project_directory.join("Cargo.toml");
    let cargo_output = match invoke_cargo_build(&manifest_path) {
        Ok(output) => output,
        Err(spawn_error) => {
            return build_single_error_snapshot(&script.id, spawn_error, started_at);
        }
    };

    finalize_compile_snapshot(script, &binaries_directory, &cargo_output, started_at)
}

fn prepare_cached_cargo_project(
    project_directory: &Path,
    binaries_directory: &Path,
    sdk_path: &Path,
    user_rust_source: &str,
) -> Result<(), String> {
    let source_directory = project_directory.join("src");
    fs::create_dir_all(&source_directory).map_err(|io_error| {
        format!(
            "Failed to create script build directory '{}': {io_error}",
            source_directory.display()
        )
    })?;
    fs::create_dir_all(binaries_directory).map_err(|io_error| {
        format!(
            "Failed to create compiled binary cache '{}': {io_error}",
            binaries_directory.display()
        )
    })?;

    let quoted_sdk_path =
        serde_json::to_string(&sdk_path.display().to_string()).map_err(|serde_error| {
            format!("Failed to encode SDK path for Cargo.toml: {serde_error}")
        })?;
    let desired_manifest = format!(
        "[workspace]\n\n[package]\nname = \"{SCRIPT_PACKAGE_NAME}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nclickforge_sdk = {{ path = {quoted_sdk_path} }}\nrand = \"0.10.4\"\n"
    );

    let manifest_path = project_directory.join("Cargo.toml");
    let existing_manifest = fs::read_to_string(&manifest_path).unwrap_or_default();
    if existing_manifest != desired_manifest {
        fs::write(&manifest_path, desired_manifest).map_err(|io_error| {
            format!(
                "Failed to write cached script Cargo.toml '{}': {io_error}",
                manifest_path.display()
            )
        })?;
    }

    let wrapped_source = build_wrapped_main_source(user_rust_source);
    let main_source_path = source_directory.join("main.rs");
    fs::write(&main_source_path, wrapped_source).map_err(|io_error| {
        format!(
            "Failed to write wrapped script source '{}': {io_error}",
            main_source_path.display()
        )
    })?;
    Ok(())
}

fn build_wrapped_main_source(user_rust_source: &str) -> String {
    let header_block = WRAPPER_HEADER_LINES.join("\n");
    format!("{header_block}\n{user_rust_source}\n}}\n")
}

fn invoke_cargo_build(manifest_path: &Path) -> Result<Output, String> {
    let offline_output = Command::new("cargo")
        .arg("build")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--message-format=json")
        .output()
        .map_err(|spawn_error| format!("Failed to run cargo build: {spawn_error}"))?;

    if offline_output.status.success() {
        return Ok(offline_output);
    }

    let stderr_text = String::from_utf8_lossy(&offline_output.stderr);
    let needs_network_resolution = stderr_text.contains("--offline")
        || stderr_text.contains("no matching package named")
        || stderr_text.contains("not cached");
    if !needs_network_resolution {
        return Ok(offline_output);
    }

    Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--message-format=json")
        .output()
        .map_err(|spawn_error| format!("Failed to run cargo build: {spawn_error}"))
}

fn finalize_compile_snapshot(
    script: &ScriptDocument,
    binaries_directory: &Path,
    cargo_output: &Output,
    started_at: Instant,
) -> CompileResultSnapshot {
    let user_line_count =
        u32::try_from(script.rust_source.lines().count().max(1)).unwrap_or(u32::MAX);
    let (mut diagnostics, built_executable) =
        parse_cargo_json_output(&cargo_output.stdout, user_line_count);
    let duration_ms = elapsed_ms_since(started_at);
    let completed_at_unix_ms = ScriptDocument::current_unix_timestamp_ms();

    if !cargo_output.status.success() {
        if diagnostics.is_empty() {
            let stderr_text = String::from_utf8_lossy(&cargo_output.stderr)
                .trim()
                .to_string();
            let message = if stderr_text.is_empty() {
                format!("cargo build failed with status {}", cargo_output.status)
            } else {
                stderr_text
            };
            diagnostics.push(plain_error_diagnostic(message));
        }
        return CompileResultSnapshot {
            script_id: script.id.clone(),
            is_success: false,
            duration_ms,
            binary_path: None,
            diagnostics,
            completed_at_unix_ms,
        };
    }

    let Some(source_executable) = built_executable else {
        diagnostics.push(plain_error_diagnostic(
            "Cargo succeeded without reporting an output executable path.".to_string(),
        ));
        return CompileResultSnapshot {
            script_id: script.id.clone(),
            is_success: false,
            duration_ms,
            binary_path: None,
            diagnostics,
            completed_at_unix_ms,
        };
    };

    match copy_executable_atomically(&source_executable, binaries_directory, &script.id) {
        Ok(final_binary_path) => CompileResultSnapshot {
            script_id: script.id.clone(),
            is_success: true,
            duration_ms,
            binary_path: Some(final_binary_path.display().to_string()),
            diagnostics,
            completed_at_unix_ms,
        },
        Err(copy_error) => {
            diagnostics.push(plain_error_diagnostic(copy_error));
            CompileResultSnapshot {
                script_id: script.id.clone(),
                is_success: false,
                duration_ms,
                binary_path: None,
                diagnostics,
                completed_at_unix_ms,
            }
        }
    }
}

fn parse_cargo_json_output(
    stdout_bytes: &[u8],
    user_line_count: u32,
) -> (Vec<CompilerDiagnostic>, Option<PathBuf>) {
    let stdout_text = String::from_utf8_lossy(stdout_bytes);
    let mut diagnostics = Vec::new();
    let mut built_executable: Option<PathBuf> = None;

    for raw_line in stdout_text.lines() {
        let Ok(json_value) = serde_json::from_str::<Value>(raw_line) else {
            continue;
        };
        let Some(reason) = json_value.get("reason").and_then(Value::as_str) else {
            continue;
        };
        if reason == "compiler-artifact" {
            if let Some(executable_str) = json_value.get("executable").and_then(Value::as_str) {
                built_executable = Some(PathBuf::from(executable_str));
            }
            continue;
        }
        if reason == "compiler-message"
            && diagnostics.len() < MAX_COMPILE_DIAGNOSTICS
            && let Some(diagnostic) = parse_compiler_message(&json_value, user_line_count)
        {
            diagnostics.push(diagnostic);
        }
    }

    (diagnostics, built_executable)
}

fn parse_compiler_message(json_value: &Value, user_line_count: u32) -> Option<CompilerDiagnostic> {
    let message_object = json_value.get("message")?;
    let level_text = message_object.get("level").and_then(Value::as_str)?;
    let severity = match level_text {
        "error" | "error: internal compiler error" => DiagnosticSeverity::Error,
        "warning" => DiagnosticSeverity::Warning,
        "note" | "help" => DiagnosticSeverity::Note,
        _ => return None,
    };

    let message_text = message_object
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Compiler diagnostic")
        .to_string();
    if message_text.starts_with("aborting due to") || message_text.ends_with("warnings emitted") {
        return None;
    }

    let rendered_excerpt = message_object
        .get("rendered")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let primary_span = message_object
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|span_items| {
            span_items
                .iter()
                .find(|span| span.get("is_primary").and_then(Value::as_bool) == Some(true))
                .or_else(|| span_items.first())
        });
    let (file_name, line_number, column_number) =
        extract_mapped_span_location(primary_span, user_line_count);

    Some(CompilerDiagnostic {
        severity,
        file_name,
        line_number,
        column_number,
        message: message_text,
        rendered_excerpt,
    })
}

fn extract_mapped_span_location(
    primary_span: Option<&Value>,
    user_line_count: u32,
) -> (Option<String>, Option<u32>, Option<u32>) {
    let Some(span_object) = primary_span else {
        return (None, None, None);
    };
    let raw_file_name = span_object.get("file_name").and_then(Value::as_str);
    let raw_line_start = span_object
        .get("line_start")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let column_start = span_object
        .get("column_start")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());

    let is_wrapped_main = raw_file_name.is_some_and(|name| name.ends_with("src/main.rs"));
    if !is_wrapped_main {
        return (
            raw_file_name.map(ToString::to_string),
            raw_line_start,
            column_start,
        );
    }

    let header_line_count = WRAPPER_HEADER_LINES.len() as u32;
    let mapped_line = raw_line_start.map(|line_start| {
        if line_start <= header_line_count {
            1
        } else {
            line_start
                .saturating_sub(header_line_count)
                .min(user_line_count.max(1))
        }
    });
    (Some("script.rs".to_string()), mapped_line, column_start)
}

fn copy_executable_atomically(
    source_executable: &Path,
    binaries_directory: &Path,
    script_id: &str,
) -> Result<PathBuf, String> {
    let sanitized_slug = sanitize_script_slug(script_id);
    let final_path = binaries_directory.join(&sanitized_slug);
    let random_suffix: u32 = rand::rng().random();
    let temporary_path = binaries_directory.join(format!(
        ".{sanitized_slug}.{}.{random_suffix:08x}.tmp",
        std::process::id()
    ));

    fs::copy(source_executable, &temporary_path).map_err(|copy_error| {
        format!(
            "Failed to copy compiled script binary to '{}': {copy_error}",
            temporary_path.display()
        )
    })?;
    if let Err(rename_error) = fs::rename(&temporary_path, &final_path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "Failed to promote compiled script binary to '{}': {rename_error}",
            final_path.display()
        ));
    }
    Ok(final_path)
}

fn sanitize_script_slug(script_id: &str) -> String {
    let sanitized: String = script_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "script".to_string()
    } else {
        sanitized
    }
}

fn build_validation_failure_snapshot(
    script_id: &str,
    validation_error: ValidationError,
    started_at: Instant,
) -> CompileResultSnapshot {
    let diagnostics = match validation_error {
        ValidationError::Failed { report, .. } => report
            .issues
            .into_iter()
            .map(|issue| plain_error_diagnostic(issue.message))
            .collect(),
    };
    CompileResultSnapshot {
        script_id: script_id.to_string(),
        is_success: false,
        duration_ms: elapsed_ms_since(started_at),
        binary_path: None,
        diagnostics,
        completed_at_unix_ms: ScriptDocument::current_unix_timestamp_ms(),
    }
}

fn build_single_error_snapshot(
    script_id: &str,
    error_message: String,
    started_at: Instant,
) -> CompileResultSnapshot {
    CompileResultSnapshot {
        script_id: script_id.to_string(),
        is_success: false,
        duration_ms: elapsed_ms_since(started_at),
        binary_path: None,
        diagnostics: vec![plain_error_diagnostic(error_message)],
        completed_at_unix_ms: ScriptDocument::current_unix_timestamp_ms(),
    }
}

fn plain_error_diagnostic(message: String) -> CompilerDiagnostic {
    CompilerDiagnostic {
        severity: DiagnosticSeverity::Error,
        file_name: None,
        line_number: None,
        column_number: None,
        message,
        rendered_excerpt: None,
    }
}

fn elapsed_ms_since(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn is_valid_sdk_directory(candidate_path: &Path) -> bool {
    candidate_path.join("Cargo.toml").is_file()
}

fn search_ancestors_for_sdk(start_path: &Path) -> Option<PathBuf> {
    for ancestor in start_path.ancestors().take(MAX_ANCESTOR_SEARCH_DEPTH) {
        let candidate = ancestor.join("crates").join("clickforge_sdk");
        if is_valid_sdk_directory(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn canonicalize_or_keep(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}
