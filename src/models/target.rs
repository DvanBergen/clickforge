use serde::{Deserialize, Serialize};

const STEAM_APP_ID_PREFIX: &str = "steam_app_";
const XWAYLAND_PROXY_NAMES: [&str; 3] = ["xwayland-satellite", "xwayland", "gamescope"];

/// Strategy used to correlate a live desktop window with a saved game target rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TargetMatchMode {
    #[default]
    PersistentAppAndTitle,
    StrictPid,
}

impl TargetMatchMode {
    /// Returns the human-readable UI label for this matching strategy.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::PersistentAppAndTitle => "App/Exe + Window Title (Persistent)",
            Self::StrictPid => "Strict PID (Current Instance Only)",
        }
    }
}

/// Snapshot of a single running Linux process scanned from `/proc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningProcessEntry {
    pub pid: u32,
    pub process_name: String,
    pub executable_name: String,
    pub command_line: String,
    pub steam_app_id: Option<String>,
    pub is_wine_or_proton: bool,
}

impl RunningProcessEntry {
    /// Returns true if this process is a display proxy whose PID masks XWayland clients.
    pub fn is_xwayland_proxy(&self) -> bool {
        let lower_process = self.process_name.to_ascii_lowercase();
        let lower_executable = self.executable_name.to_ascii_lowercase();
        XWAYLAND_PROXY_NAMES
            .iter()
            .any(|proxy_name| lower_process == *proxy_name || lower_executable == *proxy_name)
    }

    /// Returns true if this process matches the requested executable name case-insensitively.
    pub fn matches_executable_name(&self, target_executable: &str) -> bool {
        let normalized_target = target_executable.trim();
        if normalized_target.is_empty() {
            return false;
        }
        self.executable_name.eq_ignore_ascii_case(normalized_target)
            || self.process_name.eq_ignore_ascii_case(normalized_target)
    }
}

/// Snapshot of a desktop window reported by the active Wayland compositor or X11 server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActiveWindowSnapshot {
    pub window_id: Option<u64>,
    pub title: String,
    pub app_id: String,
    pub pid: Option<u32>,
    pub is_focused: bool,
    pub compositor_name: String,
}

impl ActiveWindowSnapshot {
    /// Extracts the numeric Steam app ID when `app_id` uses the `steam_app_<id>` convention.
    pub fn extracted_steam_app_id(&self) -> Option<&str> {
        let trimmed_app_id = self.app_id.trim();
        if trimmed_app_id.len() <= STEAM_APP_ID_PREFIX.len() {
            return None;
        }
        let (prefix, remainder) = trimmed_app_id.split_at(STEAM_APP_ID_PREFIX.len());
        if !prefix.eq_ignore_ascii_case(STEAM_APP_ID_PREFIX) || remainder.is_empty() {
            return None;
        }
        Some(remainder)
    }

    /// Returns true when the snapshot contains no usable window identity fields.
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.app_id.trim().is_empty() && self.pid.is_none()
    }
}

/// Rule matching a specific target game or desktop application window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GameTargetRule {
    pub display_name: String,
    pub match_mode: TargetMatchMode,
    pub app_id: Option<String>,
    pub window_title_contains: Option<String>,
    pub executable_name: Option<String>,
    pub steam_app_id: Option<String>,
    pub strict_pid: Option<u32>,
}

impl GameTargetRule {
    /// Builds a target rule from a live window, correlating XWayland/Proton `.exe` processes.
    pub fn from_window_and_processes(
        window: &ActiveWindowSnapshot,
        running_processes: &[RunningProcessEntry],
        match_mode: TargetMatchMode,
    ) -> Self {
        let correlated_process = correlate_process_for_window(window, running_processes);
        let steam_app_id = window
            .extracted_steam_app_id()
            .map(ToString::to_string)
            .or_else(|| correlated_process.and_then(|entry| entry.steam_app_id.clone()));
        let executable_name = correlated_process.map(|entry| entry.executable_name.clone());
        let app_id = non_empty_trimmed(&window.app_id);
        let window_title_contains = non_empty_trimmed(&window.title);
        let strict_pid = correlated_process.map(|entry| entry.pid).or(window.pid);
        let display_name = window_title_contains
            .clone()
            .or_else(|| executable_name.clone())
            .or_else(|| app_id.clone())
            .unwrap_or_else(|| "Selected Window".to_string());

        Self {
            display_name,
            match_mode,
            app_id,
            window_title_contains,
            executable_name,
            steam_app_id,
            strict_pid,
        }
    }

    /// Builds a target rule directly from a `/proc` process entry.
    pub fn from_process(process: &RunningProcessEntry, match_mode: TargetMatchMode) -> Self {
        let app_id = process
            .steam_app_id
            .as_ref()
            .map(|steam_id| format!("{STEAM_APP_ID_PREFIX}{steam_id}"));
        let executable_name = non_empty_trimmed(&process.executable_name)
            .or_else(|| non_empty_trimmed(&process.process_name));
        let display_name = executable_name
            .clone()
            .unwrap_or_else(|| format!("PID {}", process.pid));

        Self {
            display_name,
            match_mode,
            app_id,
            window_title_contains: None,
            executable_name,
            steam_app_id: process.steam_app_id.clone(),
            strict_pid: Some(process.pid),
        }
    }

    /// Returns true when this rule defines at least one valid matching criterion for its mode.
    pub fn has_match_criteria(&self) -> bool {
        match self.match_mode {
            TargetMatchMode::StrictPid => self.strict_pid.is_some(),
            TargetMatchMode::PersistentAppAndTitle => {
                non_empty_option(&self.app_id).is_some()
                    || non_empty_option(&self.window_title_contains).is_some()
                    || non_empty_option(&self.executable_name).is_some()
                    || non_empty_option(&self.steam_app_id).is_some()
            }
        }
    }

    /// Evaluates whether a window snapshot matches this rule, handling XWayland/Proton proxies.
    pub fn matches_window(
        &self,
        window: &ActiveWindowSnapshot,
        running_processes: &[RunningProcessEntry],
    ) -> bool {
        if window.is_empty() || !self.has_match_criteria() {
            return false;
        }
        match self.match_mode {
            TargetMatchMode::StrictPid => self.matches_strict_pid(window, running_processes),
            TargetMatchMode::PersistentAppAndTitle => {
                self.matches_persistent_rule(window, running_processes)
            }
        }
    }

    fn matches_strict_pid(
        &self,
        window: &ActiveWindowSnapshot,
        running_processes: &[RunningProcessEntry],
    ) -> bool {
        let Some(target_pid) = self.strict_pid else {
            return false;
        };
        if window.pid == Some(target_pid) {
            return true;
        }
        let is_proxy = is_window_pid_xwayland_proxy(window, running_processes);
        if !is_proxy && window.pid.is_some() {
            return false;
        }
        let Some(target_process) = find_process_by_pid(running_processes, target_pid) else {
            return false;
        };
        if let (Some(process_steam_id), Some(window_steam_id)) = (
            target_process.steam_app_id.as_deref(),
            window.extracted_steam_app_id(),
        ) {
            return process_steam_id.eq_ignore_ascii_case(window_steam_id);
        }
        if let Some(expected_app_id) = non_empty_option(&self.app_id)
            && window.app_id.eq_ignore_ascii_case(expected_app_id)
        {
            return matches_optional_title(self.window_title_contains.as_deref(), &window.title);
        }
        executable_correlates_with_window(&target_process.executable_name, window)
    }

    fn matches_persistent_rule(
        &self,
        window: &ActiveWindowSnapshot,
        running_processes: &[RunningProcessEntry],
    ) -> bool {
        let expected_app_id = non_empty_option(&self.app_id);
        let expected_steam_id = non_empty_option(&self.steam_app_id);
        let expected_title = non_empty_option(&self.window_title_contains);
        let expected_executable = non_empty_option(&self.executable_name);

        if let Some(app_id_rule) = expected_app_id
            && !window.app_id.eq_ignore_ascii_case(app_id_rule)
        {
            return false;
        }
        if !matches_optional_title(expected_title, &window.title) {
            return false;
        }
        if let Some(steam_id_rule) = expected_steam_id
            && !matches_steam_app_id(steam_id_rule, window, running_processes)
        {
            return false;
        }
        let Some(target_executable) = expected_executable else {
            return true;
        };
        let has_window_identity_match =
            expected_app_id.is_some() || expected_steam_id.is_some() || expected_title.is_some();
        matches_executable_criterion(
            target_executable,
            has_window_identity_match,
            window,
            running_processes,
        )
    }
}

/// Execution scope controlling whether a script runs system-wide or only while a game is focused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetScope {
    #[default]
    Global,
    SelectedGame(GameTargetRule),
}

impl TargetScope {
    /// Returns true when the script runs globally regardless of focused window.
    pub fn is_global(&self) -> bool {
        matches!(self, Self::Global)
    }

    /// Returns a concise UI label describing this scope.
    pub fn display_label(&self) -> String {
        match self {
            Self::Global => "Global (Any Window)".to_string(),
            Self::SelectedGame(rule) => {
                if rule.display_name.trim().is_empty() {
                    "Selected Game (Unbound)".to_string()
                } else {
                    rule.display_name.clone()
                }
            }
        }
    }

    /// Returns true when the script is eligible to run for the currently focused window.
    pub fn matches_focused_window(
        &self,
        focused_window: Option<&ActiveWindowSnapshot>,
        running_processes: &[RunningProcessEntry],
    ) -> bool {
        match self {
            Self::Global => true,
            Self::SelectedGame(rule) => focused_window.is_some_and(|window| {
                window.is_focused && rule.matches_window(window, running_processes)
            }),
        }
    }
}

fn non_empty_trimmed(raw_text: &str) -> Option<String> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn non_empty_option(optional_text: &Option<String>) -> Option<&str> {
    optional_text
        .as_deref()
        .map(str::trim)
        .filter(|trimmed| !trimmed.is_empty())
}

fn find_process_by_pid(
    running_processes: &[RunningProcessEntry],
    target_pid: u32,
) -> Option<&RunningProcessEntry> {
    running_processes
        .iter()
        .find(|process_entry| process_entry.pid == target_pid)
}

fn is_window_pid_xwayland_proxy(
    window: &ActiveWindowSnapshot,
    running_processes: &[RunningProcessEntry],
) -> bool {
    let Some(window_pid) = window.pid else {
        return false;
    };
    find_process_by_pid(running_processes, window_pid)
        .is_some_and(RunningProcessEntry::is_xwayland_proxy)
}

fn matches_optional_title(expected_substring: Option<&str>, window_title: &str) -> bool {
    let Some(needle) = expected_substring else {
        return true;
    };
    window_title
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn matches_steam_app_id(
    expected_steam_id: &str,
    window: &ActiveWindowSnapshot,
    running_processes: &[RunningProcessEntry],
) -> bool {
    if let Some(window_steam_id) = window.extracted_steam_app_id() {
        return window_steam_id.eq_ignore_ascii_case(expected_steam_id);
    }
    if let Some(window_pid) = window.pid
        && let Some(direct_process) = find_process_by_pid(running_processes, window_pid)
        && !direct_process.is_xwayland_proxy()
    {
        return direct_process
            .steam_app_id
            .as_deref()
            .is_some_and(|steam_id| steam_id.eq_ignore_ascii_case(expected_steam_id));
    }
    false
}

fn matches_executable_criterion(
    target_executable: &str,
    has_window_identity_match: bool,
    window: &ActiveWindowSnapshot,
    running_processes: &[RunningProcessEntry],
) -> bool {
    if let Some(window_pid) = window.pid
        && let Some(direct_process) = find_process_by_pid(running_processes, window_pid)
        && !direct_process.is_xwayland_proxy()
    {
        return direct_process.matches_executable_name(target_executable);
    }

    let matching_processes: Vec<&RunningProcessEntry> = running_processes
        .iter()
        .filter(|entry| entry.matches_executable_name(target_executable))
        .collect();

    if matching_processes.is_empty() {
        if running_processes.is_empty() {
            return has_window_identity_match
                || executable_correlates_with_window(target_executable, window);
        }
        return false;
    }

    if let Some(window_steam_id) = window.extracted_steam_app_id() {
        let steam_matches_any = matching_processes.iter().any(|entry| {
            entry
                .steam_app_id
                .as_deref()
                .is_some_and(|steam_id| steam_id.eq_ignore_ascii_case(window_steam_id))
        });
        if steam_matches_any {
            return true;
        }
        let all_have_other_steam_ids = matching_processes
            .iter()
            .all(|entry| entry.steam_app_id.is_some());
        if all_have_other_steam_ids {
            return false;
        }
    }

    has_window_identity_match || executable_correlates_with_window(target_executable, window)
}

fn correlate_process_for_window<'slice>(
    window: &ActiveWindowSnapshot,
    running_processes: &'slice [RunningProcessEntry],
) -> Option<&'slice RunningProcessEntry> {
    if let Some(window_pid) = window.pid
        && let Some(direct_process) = find_process_by_pid(running_processes, window_pid)
        && !direct_process.is_xwayland_proxy()
    {
        return Some(direct_process);
    }
    if let Some(window_steam_id) = window.extracted_steam_app_id() {
        if let Some(proton_match) = running_processes.iter().find(|entry| {
            entry.is_wine_or_proton
                && entry
                    .steam_app_id
                    .as_deref()
                    .is_some_and(|steam_id| steam_id.eq_ignore_ascii_case(window_steam_id))
        }) {
            return Some(proton_match);
        }
        if let Some(steam_match) = running_processes.iter().find(|entry| {
            !entry.is_xwayland_proxy()
                && entry
                    .steam_app_id
                    .as_deref()
                    .is_some_and(|steam_id| steam_id.eq_ignore_ascii_case(window_steam_id))
        }) {
            return Some(steam_match);
        }
    }
    running_processes.iter().find(|entry| {
        !entry.is_xwayland_proxy()
            && executable_correlates_with_window(&entry.executable_name, window)
    })
}

fn executable_correlates_with_window(executable_name: &str, window: &ActiveWindowSnapshot) -> bool {
    let normalized_stem = normalize_alphanumeric_stem(executable_name);
    if normalized_stem.len() < 3 {
        return false;
    }
    let normalized_title = normalize_alphanumeric(&window.title);
    let normalized_app_id = normalize_alphanumeric(&window.app_id);
    normalized_title.contains(&normalized_stem) || normalized_app_id.contains(&normalized_stem)
}

fn normalize_alphanumeric_stem(executable_name: &str) -> String {
    let trimmed = executable_name.trim();
    let without_exe = trimmed
        .strip_suffix(".exe")
        .or_else(|| trimmed.strip_suffix(".EXE"))
        .unwrap_or(trimmed);
    normalize_alphanumeric(without_exe)
}

fn normalize_alphanumeric(raw_text: &str) -> String {
    raw_text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_proton_processes() -> Vec<RunningProcessEntry> {
        vec![
            RunningProcessEntry {
                pid: 1200,
                process_name: "xwayland-satellite".to_string(),
                executable_name: "xwayland-satellite".to_string(),
                command_line: "/usr/bin/xwayland-satellite".to_string(),
                steam_app_id: None,
                is_wine_or_proton: false,
            },
            RunningProcessEntry {
                pid: 8842,
                process_name: "HereComesTheHor".to_string(),
                executable_name: "HereComesTheHorde.exe".to_string(),
                command_line: "Z:\\games\\HereComesTheHorde.exe".to_string(),
                steam_app_id: Some("2560650".to_string()),
                is_wine_or_proton: true,
            },
            RunningProcessEntry {
                pid: 3333,
                process_name: "firefox".to_string(),
                executable_name: "firefox".to_string(),
                command_line: "/usr/lib/firefox/firefox".to_string(),
                steam_app_id: None,
                is_wine_or_proton: false,
            },
        ]
    }

    #[test]
    fn persistent_rule_matches_xwayland_proton_game_under_niri_and_rejects_browser() {
        let processes = sample_proton_processes();
        let game_window = ActiveWindowSnapshot {
            window_id: Some(77),
            title: "Here Comes the Horde".to_string(),
            app_id: "steam_app_2560650".to_string(),
            pid: Some(1200),
            is_focused: true,
            compositor_name: "niri".to_string(),
        };
        let rule = GameTargetRule::from_window_and_processes(
            &game_window,
            &processes,
            TargetMatchMode::PersistentAppAndTitle,
        );
        assert_eq!(
            rule.executable_name.as_deref(),
            Some("HereComesTheHorde.exe")
        );
        assert_eq!(rule.steam_app_id.as_deref(), Some("2560650"));
        assert_eq!(rule.strict_pid, Some(8842));
        assert!(rule.matches_window(&game_window, &processes));

        let browser_window = ActiveWindowSnapshot {
            window_id: Some(88),
            title: "Here Comes the Horde Wiki - Firefox".to_string(),
            app_id: "firefox".to_string(),
            pid: Some(3333),
            is_focused: true,
            compositor_name: "niri".to_string(),
        };
        assert!(!rule.matches_window(&browser_window, &processes));

        let steam_friends_window = ActiveWindowSnapshot {
            window_id: Some(89),
            title: "Friends List".to_string(),
            app_id: "steam".to_string(),
            pid: Some(1200),
            is_focused: true,
            compositor_name: "niri".to_string(),
        };
        assert!(!rule.matches_window(&steam_friends_window, &processes));
    }

    #[test]
    fn strict_pid_matches_live_proton_pid_and_fails_when_pid_changes() {
        let processes = sample_proton_processes();
        let game_window = ActiveWindowSnapshot {
            window_id: Some(77),
            title: "Here Comes the Horde".to_string(),
            app_id: "steam_app_2560650".to_string(),
            pid: Some(1200),
            is_focused: true,
            compositor_name: "niri".to_string(),
        };
        let strict_rule = GameTargetRule::from_window_and_processes(
            &game_window,
            &processes,
            TargetMatchMode::StrictPid,
        );
        assert!(strict_rule.matches_window(&game_window, &processes));

        let restarted_processes = vec![
            processes[0].clone(),
            RunningProcessEntry {
                pid: 9999,
                ..processes[1].clone()
            },
        ];
        assert!(!strict_rule.matches_window(&game_window, &restarted_processes));
    }

    #[test]
    fn target_scope_enforces_focus_gating_for_selected_game() {
        let processes = sample_proton_processes();
        let mut game_window = ActiveWindowSnapshot {
            window_id: Some(77),
            title: "Here Comes the Horde".to_string(),
            app_id: "steam_app_2560650".to_string(),
            pid: Some(1200),
            is_focused: true,
            compositor_name: "niri".to_string(),
        };
        let rule = GameTargetRule::from_window_and_processes(
            &game_window,
            &processes,
            TargetMatchMode::PersistentAppAndTitle,
        );
        let scope = TargetScope::SelectedGame(rule);
        assert!(scope.matches_focused_window(Some(&game_window), &processes));

        game_window.is_focused = false;
        assert!(!scope.matches_focused_window(Some(&game_window), &processes));
        assert!(TargetScope::Global.matches_focused_window(None, &processes));
    }
}
