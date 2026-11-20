use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clickforge::models::{
    ConditionExpr, GameTargetRule, ScriptDocument, ScriptMode, StatementKind, TargetScope,
    TriggerBlock, ValidationIssueCode, inspect_script_validation,
    validate_script_for_persistence_and_execution,
};
use clickforge::storage::{MAX_SCRIPT_BACKUPS, ScriptRepository, StorageError};
use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};

struct TempDirectoryGuard {
    directory_path: PathBuf,
}

impl TempDirectoryGuard {
    fn create(test_label: &str) -> Result<Self, std::io::Error> {
        let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
        let random_suffix: u32 = rand::random();
        let process_id = std::process::id();
        let directory_path = std::env::temp_dir().join(format!(
            "clickforge-validation-it-{test_label}-{process_id}-{timestamp_ms}-{random_suffix:08x}"
        ));
        fs::create_dir_all(&directory_path)?;
        Ok(Self { directory_path })
    }
}

impl Drop for TempDirectoryGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory_path);
    }
}

#[test]
fn missing_kill_switch_is_rejected_by_validation_and_repository_save() -> Result<(), Box<dyn Error>>
{
    let temp_guard = TempDirectoryGuard::create("missing-kill-switch")?;
    let repository = ScriptRepository::new_with_base_dir(temp_guard.directory_path.clone())?;

    for mode in [ScriptMode::VisualBlocks, ScriptMode::AdvancedRust] {
        let unsafe_script = ScriptDocument::new_blank("Unsafe Script", TargetScope::Global, mode);
        let report = inspect_script_validation(&unsafe_script);
        assert!(report.has_issue(ValidationIssueCode::MissingKillSwitchKey));
        assert!(validate_script_for_persistence_and_execution(&unsafe_script).is_err());

        let save_outcome = repository.save_script(&unsafe_script);
        assert!(matches!(
            save_outcome,
            Err(StorageError::ValidationRejected(_))
        ));
    }

    assert!(repository.load_all_scripts()?.is_empty());
    Ok(())
}

#[test]
fn kill_switch_collisions_with_trigger_arm_emitted_and_condition_keys_are_blocked()
-> Result<(), Box<dyn Error>> {
    let temp_guard = TempDirectoryGuard::create("key-collisions")?;
    let repository = ScriptRepository::new_with_base_dir(temp_guard.directory_path.clone())?;

    let mut trigger_collision_script = ScriptDocument::new_building_upgrade_template();
    trigger_collision_script.kill_switch_key = Some(KeyCode::F7);
    trigger_collision_script.visual_program.trigger = TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Key(KeyCode::F7),
    };
    let trigger_report = inspect_script_validation(&trigger_collision_script);
    assert!(trigger_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithTriggerKey));
    assert!(matches!(
        repository.save_script(&trigger_collision_script),
        Err(StorageError::ValidationRejected(_))
    ));

    let mut arm_collision_script = ScriptDocument::new_building_upgrade_template();
    arm_collision_script.kill_switch_key = Some(KeyCode::F6);
    arm_collision_script.arm_toggle_key = Some(KeyCode::F6);
    let arm_report = inspect_script_validation(&arm_collision_script);
    assert!(arm_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithArmToggleKey));
    assert!(matches!(
        repository.save_script(&arm_collision_script),
        Err(StorageError::ValidationRejected(_))
    ));

    let mut emitted_key_script = ScriptDocument::new_building_upgrade_template();
    emitted_key_script.kill_switch_key = Some(KeyCode::R);
    emitted_key_script
        .visual_program
        .append_root_block(StatementKind::KeyTap {
            key_code: KeyCode::R,
            hold_duration_ms: 10,
        });
    let emitted_key_report = inspect_script_validation(&emitted_key_script);
    assert!(emitted_key_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithEmittedKey));
    assert!(matches!(
        repository.save_script(&emitted_key_script),
        Err(StorageError::ValidationRejected(_))
    ));

    let mut type_text_collision_script = ScriptDocument::new_building_upgrade_template();
    type_text_collision_script.kill_switch_key = Some(KeyCode::LeftShift);
    type_text_collision_script
        .visual_program
        .append_root_block(StatementKind::TypeText {
            text: "Upgrade".to_string(),
            keystroke_delay_ms: 15,
            jitter_ms: 5,
        });
    let type_text_report = inspect_script_validation(&type_text_collision_script);
    assert!(type_text_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithEmittedKey));

    let mut condition_collision_script = ScriptDocument::new_building_upgrade_template();
    condition_collision_script.kill_switch_key = Some(KeyCode::Space);
    condition_collision_script
        .visual_program
        .append_root_block(StatementKind::IfThen {
            condition: ConditionExpr::IsInputPressed {
                input: InputTrigger::Key(KeyCode::Space),
            },
            then_blocks: Vec::new(),
        });
    let condition_report = inspect_script_validation(&condition_collision_script);
    assert!(condition_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithConditionKey));

    assert!(repository.load_all_scripts()?.is_empty());
    Ok(())
}

#[test]
fn invalid_target_scope_and_timing_ranges_fail_validation() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.target_scope = TargetScope::SelectedGame(GameTargetRule::default());
    script
        .visual_program
        .append_root_block(StatementKind::WaitRandomUniform {
            min_delay_ms: 80,
            max_delay_ms: 20,
        });
    script
        .visual_program
        .append_root_block(StatementKind::WaitRandomGaussian {
            base_delay_ms: 45,
            min_offset_ms: 25.0,
            max_offset_ms: 10.0,
            mean_offset_ms: 15.0,
            std_dev_ms: 2.5,
        });

    let report = inspect_script_validation(&script);
    assert!(report.has_issue(ValidationIssueCode::InvalidSelectedGameTarget));
    assert!(report.has_issue(ValidationIssueCode::InvalidTimingRange));
    assert!(report.has_issue(ValidationIssueCode::InvalidGaussianParameters));
}

#[test]
fn valid_script_round_trips_atomically_and_rotates_backups_to_cap() -> Result<(), Box<dyn Error>> {
    let temp_guard = TempDirectoryGuard::create("save-load-backups")?;
    let repository = ScriptRepository::new_with_base_dir(temp_guard.directory_path.clone())?;

    let mut script = ScriptDocument::new_building_upgrade_template();
    script.name = "Horde Building Rapid Clicker".to_string();
    script.kill_switch_key = Some(KeyCode::F8);
    script.arm_toggle_key = Some(KeyCode::F6);
    script.intercept_trigger_input = true;

    let saved_path = repository.save_script(&script)?;
    assert!(saved_path.is_file());

    let loaded = repository.load_script(&script.id)?;
    assert_eq!(loaded.name, script.name);
    assert_eq!(loaded.kill_switch_key, Some(KeyCode::F8));
    assert_eq!(loaded.arm_toggle_key, Some(KeyCode::F6));
    assert!(loaded.intercept_trigger_input);
    assert_eq!(
        loaded.effective_trigger_input(),
        Some(InputTrigger::Mouse(MouseButton::Left))
    );
    assert_eq!(loaded.visual_program.total_block_count(), 2);

    let overwrite_count: usize = MAX_SCRIPT_BACKUPS.saturating_add(5);
    for revision_number in 0..overwrite_count {
        script.description = format!("Revision {revision_number}");
        repository.save_script(&script)?;
    }

    let retained_backups = repository.list_backups_for_script(&script.id)?;
    assert_eq!(retained_backups.len(), MAX_SCRIPT_BACKUPS);

    for directory_entry in fs::read_dir(repository.scripts_directory())? {
        let file_name_text = directory_entry?.file_name().to_string_lossy().into_owned();
        assert!(!file_name_text.ends_with(".tmp"));
    }

    assert!(repository.delete_script(&script.id)?);
    assert!(!repository.delete_script(&script.id)?);
    assert!(repository.load_all_scripts()?.is_empty());
    assert_eq!(
        repository.list_backups_for_script(&script.id)?.len(),
        MAX_SCRIPT_BACKUPS
    );

    Ok(())
}
