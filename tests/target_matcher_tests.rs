use clickforge::linux::{MAX_PROCESS_SCAN_ENTRIES, scan_running_processes};
use clickforge::models::{
    ActiveWindowSnapshot, GameTargetRule, RunningProcessEntry, TargetMatchMode, TargetScope,
};

fn build_niri_xwayland_proton_processes() -> Vec<RunningProcessEntry> {
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
            command_line: "Z:\\games\\SteamLibrary\\HereComesTheHorde.exe".to_string(),
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
fn persistent_matching_handles_xwayland_proton_steam_app_id_under_niri() {
    let running_processes = build_niri_xwayland_proton_processes();
    let mut game_window = ActiveWindowSnapshot {
        window_id: Some(77),
        title: "Here Comes the Horde".to_string(),
        app_id: "steam_app_2560650".to_string(),
        pid: Some(1200),
        is_focused: true,
        compositor_name: "niri".to_string(),
    };

    assert_eq!(game_window.extracted_steam_app_id(), Some("2560650"));

    let target_rule = GameTargetRule::from_window_and_processes(
        &game_window,
        &running_processes,
        TargetMatchMode::PersistentAppAndTitle,
    );
    assert_eq!(
        target_rule.executable_name.as_deref(),
        Some("HereComesTheHorde.exe")
    );
    assert_eq!(target_rule.steam_app_id.as_deref(), Some("2560650"));
    assert_eq!(target_rule.strict_pid, Some(8842));
    assert!(target_rule.matches_window(&game_window, &running_processes));

    let target_scope = TargetScope::SelectedGame(target_rule.clone());
    assert!(target_scope.matches_focused_window(Some(&game_window), &running_processes));

    // Focus loss (Alt-Tab) immediately disables execution eligibility.
    game_window.is_focused = false;
    assert!(!target_scope.matches_focused_window(Some(&game_window), &running_processes));

    // Browser window with overlapping title text must not match.
    let browser_window = ActiveWindowSnapshot {
        window_id: Some(88),
        title: "Here Comes the Horde Wiki - Firefox".to_string(),
        app_id: "firefox".to_string(),
        pid: Some(3333),
        is_focused: true,
        compositor_name: "niri".to_string(),
    };
    assert!(!target_rule.matches_window(&browser_window, &running_processes));

    // Unrelated Steam client window under xwayland-satellite must not match.
    let steam_friends_window = ActiveWindowSnapshot {
        window_id: Some(89),
        title: "Friends List".to_string(),
        app_id: "steam".to_string(),
        pid: Some(1200),
        is_focused: true,
        compositor_name: "niri".to_string(),
    };
    assert!(!target_rule.matches_window(&steam_friends_window, &running_processes));
}

#[test]
fn persistent_matching_handles_steam_app_default_with_proton_exe_in_proc() {
    let running_processes = build_niri_xwayland_proton_processes();
    let default_app_game_window = ActiveWindowSnapshot {
        window_id: Some(91),
        title: "Here Comes the Horde".to_string(),
        app_id: "steam_app_default".to_string(),
        pid: Some(1200),
        is_focused: true,
        compositor_name: "niri".to_string(),
    };

    assert_eq!(default_app_game_window.extracted_steam_app_id(), None);

    let rule_from_window = GameTargetRule::from_window_and_processes(
        &default_app_game_window,
        &running_processes,
        TargetMatchMode::PersistentAppAndTitle,
    );
    assert_eq!(
        rule_from_window.executable_name.as_deref(),
        Some("HereComesTheHorde.exe")
    );
    assert_eq!(rule_from_window.steam_app_id.as_deref(), Some("2560650"));
    assert_eq!(rule_from_window.strict_pid, Some(8842));
    assert!(rule_from_window.matches_window(&default_app_game_window, &running_processes));

    let different_default_game_window = ActiveWindowSnapshot {
        window_id: Some(92),
        title: "Unrelated Indie Game".to_string(),
        app_id: "steam_app_default".to_string(),
        pid: Some(1200),
        is_focused: true,
        compositor_name: "niri".to_string(),
    };
    assert!(!rule_from_window.matches_window(&different_default_game_window, &running_processes));

    // Rule created from the Processes tab matches both steam_app_2560650 and steam_app_default.
    let proton_process = &running_processes[1];
    let rule_from_process =
        GameTargetRule::from_process(proton_process, TargetMatchMode::PersistentAppAndTitle);
    assert!(rule_from_process.matches_window(&default_app_game_window, &running_processes));

    let numeric_app_game_window = ActiveWindowSnapshot {
        app_id: "steam_app_2560650".to_string(),
        ..default_app_game_window.clone()
    };
    assert!(rule_from_process.matches_window(&numeric_app_game_window, &running_processes));
}

#[test]
fn strict_pid_mode_tracks_exact_proton_instance_and_rejects_restarted_pid() {
    let running_processes = build_niri_xwayland_proton_processes();

    for app_id in ["steam_app_2560650", "steam_app_default"] {
        let game_window = ActiveWindowSnapshot {
            window_id: Some(77),
            title: "Here Comes the Horde".to_string(),
            app_id: app_id.to_string(),
            pid: Some(1200),
            is_focused: true,
            compositor_name: "niri".to_string(),
        };

        let strict_rule = GameTargetRule::from_window_and_processes(
            &game_window,
            &running_processes,
            TargetMatchMode::StrictPid,
        );
        assert_eq!(strict_rule.strict_pid, Some(8842));
        assert!(strict_rule.matches_window(&game_window, &running_processes));

        let restarted_processes = vec![
            running_processes[0].clone(),
            RunningProcessEntry {
                pid: 9999,
                ..running_processes[1].clone()
            },
        ];
        assert!(!strict_rule.matches_window(&game_window, &restarted_processes));
    }
}

#[test]
fn live_proc_scanner_enumerates_host_processes_including_current_test_binary() {
    let current_pid = std::process::id();
    let host_processes = scan_running_processes();

    assert!(!host_processes.is_empty());
    assert!(host_processes.len() <= MAX_PROCESS_SCAN_ENTRIES);
    assert!(
        host_processes.iter().any(|entry| entry.pid == current_pid),
        "live /proc scanner did not find current test PID {current_pid}"
    );
}
