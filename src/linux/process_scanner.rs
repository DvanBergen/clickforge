use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use crate::models::target::RunningProcessEntry;

/// Hard upper bound on `/proc` entries scanned per pass to keep memory and latency bounded.
pub const MAX_PROCESS_SCAN_ENTRIES: usize = 4096;

const PROC_ROOT_PATH: &str = "/proc";
const MAX_COMM_READ_BYTES: u64 = 256;
const MAX_STAT_READ_BYTES: u64 = 1024;
const MAX_CMDLINE_READ_BYTES: u64 = 16_384;
const MAX_ENVIRON_READ_BYTES: u64 = 131_072;
const MAX_STORED_COMMAND_LINE_CHARS: usize = 512;
const MAX_PARENT_INHERITANCE_DEPTH: usize = 8;

const STEAM_APP_ID_ENV_KEYS: [&str; 3] = ["SteamAppId", "STEAM_COMPAT_APP_ID", "SteamGameId"];

const WINE_RUNTIME_INDICATORS: [&str; 7] = [
    "wine-preloader",
    "wine64-preloader",
    "wineserver",
    "wine64",
    "wine",
    "proton",
    "pressure-vessel",
];

const WINE_BACKGROUND_EXECUTABLES: [&str; 16] = [
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "svchost.exe",
    "rpcss.exe",
    "explorer.exe",
    "conhost.exe",
    "tabtip.exe",
    "steam.exe",
    "start.exe",
    "wineboot.exe",
    "winemenubuilder.exe",
    "rundll32.exe",
    "csrss.exe",
    "crss.exe",
    "msedgewebview2.exe",
];

#[derive(Debug, Clone)]
struct ScannedProcessRecord {
    entry: RunningProcessEntry,
    parent_pid: Option<u32>,
}

/// Scans `/proc` natively and returns up to `MAX_PROCESS_SCAN_ENTRIES` running Linux processes.
pub fn scan_running_processes() -> Vec<RunningProcessEntry> {
    scan_running_processes_from_root(Path::new(PROC_ROOT_PATH), MAX_PROCESS_SCAN_ENTRIES)
}

/// Scans a `/proc`-compatible root directory with an explicit entry cap for testing and production.
pub fn scan_running_processes_from_root(
    proc_root: &Path,
    max_entries: usize,
) -> Vec<RunningProcessEntry> {
    let Ok(directory_entries) = fs::read_dir(proc_root) else {
        return Vec::new();
    };

    let bounded_limit = max_entries.min(MAX_PROCESS_SCAN_ENTRIES);
    let mut records = Vec::with_capacity(bounded_limit.min(256));

    for entry_result in directory_entries {
        if records.len() >= bounded_limit {
            break;
        }
        let Ok(directory_entry) = entry_result else {
            continue;
        };
        let Some(process_pid) = parse_pid_from_file_name(&directory_entry.file_name()) else {
            continue;
        };
        if let Some(record) = read_single_process_record(&directory_entry.path(), process_pid) {
            records.push(record);
        }
    }

    propagate_inherited_steam_app_ids(&mut records);
    sort_process_records_for_correlation(&mut records);
    records.into_iter().map(|record| record.entry).collect()
}

/// Parses the parent PID field (field 4) from a Linux `/proc/<pid>/stat` line.
pub fn parse_parent_pid_from_stat(stat_contents: &str) -> Option<u32> {
    let closing_paren_index = stat_contents.rfind(')')?;
    let trailing_fields = stat_contents.get(closing_paren_index.saturating_add(1)..)?;
    let mut field_iterator = trailing_fields.split_whitespace();
    let _process_state = field_iterator.next()?;
    let parent_pid_text = field_iterator.next()?;
    parent_pid_text.parse::<u32>().ok()
}

/// Extracts a Windows `.exe` basename (case-insensitive) from parsed command-line tokens.
pub fn extract_windows_exe_basename(command_tokens: &[String]) -> Option<String> {
    for raw_token in command_tokens {
        if let Some(exe_name) = extract_exe_basename_from_token(raw_token) {
            return Some(exe_name);
        }
    }
    None
}

/// Extracts the Steam App ID from NUL-delimited `/proc/<pid>/environ` bytes.
pub fn extract_steam_app_id_from_environ(environ_bytes: &[u8]) -> Option<String> {
    for target_key in STEAM_APP_ID_ENV_KEYS {
        if let Some(found_value) = find_environ_key_value(environ_bytes, target_key) {
            return Some(found_value);
        }
    }
    None
}

fn read_single_process_record(
    process_directory: &Path,
    process_pid: u32,
) -> Option<ScannedProcessRecord> {
    let comm_bytes = read_bounded_file(&process_directory.join("comm"), MAX_COMM_READ_BYTES)?;
    let process_name = String::from_utf8_lossy(&comm_bytes).trim().to_string();
    if process_name.is_empty() {
        return None;
    }

    let stat_bytes = read_bounded_file(&process_directory.join("stat"), MAX_STAT_READ_BYTES);
    let parent_pid = stat_bytes
        .as_deref()
        .map(|bytes| String::from_utf8_lossy(bytes))
        .and_then(|stat_text| parse_parent_pid_from_stat(&stat_text));

    let cmdline_bytes =
        read_bounded_file(&process_directory.join("cmdline"), MAX_CMDLINE_READ_BYTES)
            .unwrap_or_default();
    let command_tokens = parse_cmdline_tokens(&cmdline_bytes);
    let command_line = format_command_line_preview(&command_tokens);

    let environ_bytes =
        read_bounded_file(&process_directory.join("environ"), MAX_ENVIRON_READ_BYTES)
            .unwrap_or_default();
    let steam_app_id = extract_steam_app_id_from_environ(&environ_bytes);

    let extracted_windows_exe = extract_windows_exe_basename(&command_tokens);
    let is_wine_or_proton = detect_wine_or_proton(
        &process_name,
        &command_line,
        extracted_windows_exe.is_some(),
    );
    let executable_name = resolve_executable_name(
        &process_name,
        &command_tokens,
        extracted_windows_exe.as_deref(),
    );

    Some(ScannedProcessRecord {
        entry: RunningProcessEntry {
            pid: process_pid,
            process_name,
            executable_name,
            command_line,
            steam_app_id,
            is_wine_or_proton,
        },
        parent_pid,
    })
}

fn parse_pid_from_file_name(file_name: &std::ffi::OsStr) -> Option<u32> {
    let text = file_name.to_str()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok()
}

fn read_bounded_file(file_path: &Path, max_read_bytes: u64) -> Option<Vec<u8>> {
    let file = File::open(file_path).ok()?;
    let mut bounded_reader = file.take(max_read_bytes);
    let mut byte_buffer = Vec::new();
    bounded_reader.read_to_end(&mut byte_buffer).ok()?;
    Some(byte_buffer)
}

fn parse_cmdline_tokens(cmdline_bytes: &[u8]) -> Vec<String> {
    cmdline_bytes
        .split(|byte| *byte == 0)
        .filter(|slice| !slice.is_empty())
        .map(|slice| String::from_utf8_lossy(slice).trim().to_string())
        .filter(|token| !token.is_empty())
        .collect()
}

fn format_command_line_preview(command_tokens: &[String]) -> String {
    let joined = command_tokens.join(" ");
    if joined.chars().count() <= MAX_STORED_COMMAND_LINE_CHARS {
        return joined;
    }
    joined.chars().take(MAX_STORED_COMMAND_LINE_CHARS).collect()
}

fn extract_exe_basename_from_token(raw_token: &str) -> Option<String> {
    let direct_candidate = normalize_candidate_path(raw_token);
    if ends_with_exe_ignore_case(direct_candidate) {
        return extract_path_basename(direct_candidate);
    }

    // Some Wine processes pack a full Windows command line with arguments into argv[0].
    let lower_token = raw_token.to_ascii_lowercase();
    let exe_match_index = lower_token.find(".exe")?;
    let exe_end_index = exe_match_index.checked_add(4)?;
    let trailing_char = raw_token.get(exe_end_index..)?.chars().next();
    let has_valid_boundary = trailing_char
        .is_none_or(|character| character.is_whitespace() || matches!(character, '"' | '\''));
    if !has_valid_boundary {
        return None;
    }

    let prefix_slice = raw_token.get(..exe_end_index)?;
    let cleaned_prefix = normalize_candidate_path(prefix_slice);
    extract_path_basename(cleaned_prefix)
}

fn normalize_candidate_path(raw_text: &str) -> &str {
    raw_text.trim().trim_matches(['"', '\''])
}

fn ends_with_exe_ignore_case(candidate_text: &str) -> bool {
    let Some(suffix_start) = candidate_text.len().checked_sub(4) else {
        return false;
    };
    candidate_text
        .get(suffix_start..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
}

fn extract_path_basename(path_text: &str) -> Option<String> {
    let basename = path_text.rsplit(['/', '\\']).next().map(str::trim)?;
    if basename.is_empty() {
        return None;
    }
    Some(basename.to_string())
}

fn find_environ_key_value(environ_bytes: &[u8], target_key: &str) -> Option<String> {
    for raw_entry in environ_bytes.split(|byte| *byte == 0) {
        if raw_entry.is_empty() {
            continue;
        }
        let entry_text = String::from_utf8_lossy(raw_entry);
        let Some((key_part, value_part)) = entry_text.split_once('=') else {
            continue;
        };
        if key_part != target_key {
            continue;
        }
        let trimmed_value = value_part.trim();
        if is_valid_steam_app_id(trimmed_value) {
            return Some(trimmed_value.to_string());
        }
    }
    None
}

fn is_valid_steam_app_id(candidate_id: &str) -> bool {
    !candidate_id.is_empty()
        && candidate_id != "0"
        && candidate_id.bytes().all(|byte| byte.is_ascii_digit())
}

fn detect_wine_or_proton(process_name: &str, command_line: &str, has_windows_exe: bool) -> bool {
    if has_windows_exe {
        return true;
    }
    let lower_process = process_name.to_ascii_lowercase();
    let lower_command = command_line.to_ascii_lowercase();
    WINE_RUNTIME_INDICATORS
        .iter()
        .any(|indicator| lower_process == *indicator || lower_command.contains(indicator))
}

fn resolve_executable_name(
    process_name: &str,
    command_tokens: &[String],
    extracted_windows_exe: Option<&str>,
) -> String {
    if let Some(windows_exe) = extracted_windows_exe {
        return windows_exe.to_string();
    }
    if let Some(first_token) = command_tokens.first()
        && let Some(basename) = extract_path_basename(normalize_candidate_path(first_token))
    {
        return basename;
    }
    process_name.to_string()
}

fn propagate_inherited_steam_app_ids(records: &mut [ScannedProcessRecord]) {
    let mut parent_by_pid: HashMap<u32, u32> = HashMap::with_capacity(records.len());
    let mut steam_id_by_pid: HashMap<u32, String> = HashMap::new();

    for record in records.iter() {
        if let Some(parent_pid) = record.parent_pid {
            parent_by_pid.insert(record.entry.pid, parent_pid);
        }
        if let Some(steam_app_id) = &record.entry.steam_app_id {
            steam_id_by_pid.insert(record.entry.pid, steam_app_id.clone());
        }
    }

    for record in records.iter_mut() {
        if record.entry.steam_app_id.is_some() {
            continue;
        }
        if let Some(inherited_id) =
            find_ancestor_steam_app_id(record.parent_pid, &parent_by_pid, &steam_id_by_pid)
        {
            record.entry.steam_app_id = Some(inherited_id);
        }
    }
}

fn find_ancestor_steam_app_id(
    initial_parent_pid: Option<u32>,
    parent_by_pid: &HashMap<u32, u32>,
    steam_id_by_pid: &HashMap<u32, String>,
) -> Option<String> {
    let mut current_pid = initial_parent_pid?;
    for _depth_step in 0..MAX_PARENT_INHERITANCE_DEPTH {
        if let Some(found_steam_id) = steam_id_by_pid.get(&current_pid) {
            return Some(found_steam_id.clone());
        }
        let next_parent_pid = *parent_by_pid.get(&current_pid)?;
        if next_parent_pid == 0 || next_parent_pid == current_pid {
            return None;
        }
        current_pid = next_parent_pid;
    }
    None
}

fn sort_process_records_for_correlation(records: &mut [ScannedProcessRecord]) {
    records.sort_by(|left_record, right_record| {
        let left_rank = process_correlation_priority(&left_record.entry);
        let right_rank = process_correlation_priority(&right_record.entry);
        left_rank
            .cmp(&right_rank)
            .then_with(|| left_record.entry.pid.cmp(&right_record.entry.pid))
    });
}

fn process_correlation_priority(entry: &RunningProcessEntry) -> u8 {
    let is_wine_helper = is_wine_background_executable(&entry.executable_name);
    if entry.is_wine_or_proton && entry.steam_app_id.is_some() && !is_wine_helper {
        return 0;
    }
    if entry.is_wine_or_proton && !is_wine_helper {
        return 1;
    }
    if entry.steam_app_id.is_some() && !is_wine_helper {
        return 2;
    }
    if !is_wine_helper {
        return 3;
    }
    4
}

fn is_wine_background_executable(executable_name: &str) -> bool {
    WINE_BACKGROUND_EXECUTABLES
        .iter()
        .any(|helper_name| executable_name.eq_ignore_ascii_case(helper_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_temp_proc_root(test_label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root_path = std::env::temp_dir().join(format!(
            "clickforge_proc_test_{test_label}_{}_{nanos}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&root_path);
        root_path
    }

    #[test]
    fn extracts_wine_proton_exe_names_and_steam_app_ids() {
        let tokens = vec![
            "Z:\\mnt\\games\\SteamLibrary\\HereComesTheHorde.EXE".to_string(),
            "--fullscreen".to_string(),
        ];
        assert_eq!(
            extract_windows_exe_basename(&tokens).as_deref(),
            Some("HereComesTheHorde.EXE")
        );

        let packed_tokens =
            vec!["C:\\Program Files\\Studio\\GameClient.exe -dx11 -windowed".to_string()];
        assert_eq!(
            extract_windows_exe_basename(&packed_tokens).as_deref(),
            Some("GameClient.exe")
        );

        let environ_bytes = b"HOME=/tmp/clickforge-test-home\0SteamAppId=2560650\0DISPLAY=:0\0";
        assert_eq!(
            extract_steam_app_id_from_environ(environ_bytes).as_deref(),
            Some("2560650")
        );

        let zero_environ = b"SteamAppId=0\0STEAM_COMPAT_APP_ID=1245620\0";
        assert_eq!(
            extract_steam_app_id_from_environ(zero_environ).as_deref(),
            Some("1245620")
        );
    }

    #[test]
    fn parses_parent_pid_even_with_spaces_and_parentheses_in_comm() {
        let stat_line = "4201 (Web Content (Isolated)) S 3900 3900 3900 0 -1 4194304";
        assert_eq!(parse_parent_pid_from_stat(stat_line), Some(3900));
    }

    #[test]
    fn synthetic_proc_scan_inherits_steam_app_id_and_prioritizes_game_exe() {
        let proc_root = create_temp_proc_root("synthetic");

        let helper_directory = proc_root.join("100");
        let game_directory = proc_root.join("200");
        let _ = fs::create_dir_all(&helper_directory);
        let _ = fs::create_dir_all(&game_directory);

        let _ = fs::write(helper_directory.join("comm"), "services.exe\n");
        let _ = fs::write(helper_directory.join("stat"), "100 (services.exe) S 1 1 1");
        let _ = fs::write(
            helper_directory.join("cmdline"),
            b"C:\\windows\\system32\\services.exe\0",
        );
        let _ = fs::write(helper_directory.join("environ"), b"SteamAppId=2560650\0");

        let _ = fs::write(game_directory.join("comm"), "HereComesTheHor\n");
        let _ = fs::write(
            game_directory.join("stat"),
            "200 (HereComesTheHor) S 100 100 100",
        );
        let _ = fs::write(
            game_directory.join("cmdline"),
            b"Z:\\games\\HereComesTheHorde.exe\0--play\0",
        );
        let _ = fs::write(game_directory.join("environ"), b"PATH=/usr/bin\0");

        let scanned = scan_running_processes_from_root(&proc_root, 16);
        let _ = fs::remove_dir_all(&proc_root);

        assert_eq!(scanned.len(), 2);
        assert_eq!(scanned[0].executable_name, "HereComesTheHorde.exe");
        assert_eq!(scanned[0].steam_app_id.as_deref(), Some("2560650"));
        assert!(scanned[0].is_wine_or_proton);
        assert_eq!(scanned[1].executable_name, "services.exe");
    }

    #[test]
    fn live_proc_scan_finds_current_test_process() {
        let current_pid = std::process::id();
        let processes = scan_running_processes();
        assert!(!processes.is_empty());
        assert!(processes.len() <= MAX_PROCESS_SCAN_ENTRIES);
        assert!(processes.iter().any(|entry| entry.pid == current_pid));
    }
}
