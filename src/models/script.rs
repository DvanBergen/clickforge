use std::time::{SystemTime, UNIX_EPOCH};

use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::models::blocks::{StatementKind, TriggerBlock, VisualProgram};
use crate::models::target::TargetScope;

/// Stable slug identifier for the built-in Building Upgrade Rapid Clicker template.
pub const BUILDING_UPGRADE_TEMPLATE_ID: &str = "template-building-upgrade-clicker";

/// Authoring and execution mode of a ClickForge script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScriptMode {
    #[default]
    VisualBlocks,
    AdvancedRust,
}

impl ScriptMode {
    /// Returns the human-readable UI label for this script mode.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::VisualBlocks => "Visual Blocks",
            Self::AdvancedRust => "Advanced Rust",
        }
    }
}

/// Persisted script definition unifying Visual Block programs and Advanced Rust sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptDocument {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mode: ScriptMode,
    pub is_enabled: bool,
    pub target_scope: TargetScope,
    pub kill_switch_key: Option<KeyCode>,
    pub arm_toggle_key: Option<KeyCode>,
    pub intercept_trigger_input: bool,
    pub visual_program: VisualProgram,
    pub rust_source: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl ScriptDocument {
    /// Generates a unique filename-safe script identifier using timestamp and random hex.
    pub fn generate_unique_id() -> String {
        let timestamp_ms = Self::current_unix_timestamp_ms();
        let random_suffix: u32 = rand::rng().random();
        format!("script-{timestamp_ms}-{random_suffix:08x}")
    }

    /// Returns the current wall-clock Unix timestamp in milliseconds, saturating at zero.
    pub fn current_unix_timestamp_ms() -> u64 {
        let Ok(elapsed_since_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return 0;
        };
        u64::try_from(elapsed_since_epoch.as_millis()).unwrap_or(u64::MAX)
    }

    /// Creates a blank script with `kill_switch_key: None` so the safety gate enforces binding.
    pub fn new_blank(name: impl Into<String>, target_scope: TargetScope, mode: ScriptMode) -> Self {
        let now_unix_ms = Self::current_unix_timestamp_ms();
        Self {
            id: Self::generate_unique_id(),
            name: name.into(),
            description: String::new(),
            mode,
            is_enabled: false,
            target_scope,
            kill_switch_key: None,
            arm_toggle_key: None,
            intercept_trigger_input: false,
            visual_program: VisualProgram::new(TriggerBlock::WhileInputHeld {
                trigger_input: InputTrigger::Mouse(MouseButton::Left),
            }),
            rust_source: Self::default_rust_starter_source(),
            created_at_unix_ms: now_unix_ms,
            updated_at_unix_ms: now_unix_ms,
        }
    }

    /// Builds the default Building Upgrade Rapid Clicker template script.
    pub fn new_building_upgrade_template() -> Self {
        let now_unix_ms = Self::current_unix_timestamp_ms();
        let mut visual_program = VisualProgram::new(TriggerBlock::WhileInputHeld {
            trigger_input: InputTrigger::Mouse(MouseButton::Left),
        });
        visual_program.append_root_block(StatementKind::WaitRandomGaussian {
            base_delay_ms: 45,
            min_offset_ms: 10.0,
            max_offset_ms: 20.0,
            mean_offset_ms: 15.0,
            std_dev_ms: 2.5,
        });
        visual_program.append_root_block(StatementKind::MouseClick {
            mouse_button: MouseButton::Left,
            hold_duration_ms: 12,
        });

        Self {
            id: BUILDING_UPGRADE_TEMPLATE_ID.to_string(),
            name: "Building Upgrade Rapid Clicker".to_string(),
            description:
                "While Left Mouse is held, waits 45 ms + Gaussian 10..20 ms and clicks Left Mouse."
                    .to_string(),
            mode: ScriptMode::VisualBlocks,
            is_enabled: false,
            target_scope: TargetScope::Global,
            kill_switch_key: Some(KeyCode::F8),
            arm_toggle_key: Some(KeyCode::F6),
            intercept_trigger_input: true,
            visual_program,
            rust_source: Self::building_upgrade_rust_source(),
            created_at_unix_ms: now_unix_ms,
            updated_at_unix_ms: now_unix_ms,
        }
    }

    /// Returns starter Advanced Rust code demonstrating the `clickforge_sdk` entrypoint.
    pub fn default_rust_starter_source() -> String {
        [
            "use clickforge_sdk::{GaussianRangeMs, MouseButton, ScriptContext, ScriptResult};",
            "",
            "pub fn run_script(script_context: &mut ScriptContext) -> ScriptResult<()> {",
            "    let jitter_range_ms = GaussianRangeMs::new(10.0, 20.0, 15.0, 2.5)?;",
            "    while !script_context.should_stop() {",
            "        if script_context.is_mouse_pressed(MouseButton::Left)? {",
            "            script_context.wait_base_plus_gaussian_ms(45, jitter_range_ms)?;",
            "            script_context.mouse_click(MouseButton::Left, 12)?;",
            "        } else {",
            "            script_context.wait_fixed_ms(5)?;",
            "        }",
            "    }",
            "    Ok(())",
            "}",
        ]
        .join("\n")
    }

    /// Returns the Rust source corresponding to the Building Upgrade Rapid Clicker template.
    pub fn building_upgrade_rust_source() -> String {
        Self::default_rust_starter_source()
    }

    /// Updates `updated_at_unix_ms` to the current timestamp.
    pub fn touch_updated_timestamp(&mut self) {
        self.updated_at_unix_ms = Self::current_unix_timestamp_ms();
    }

    /// Returns the trigger input that should be intercepted when `intercept_trigger_input` is enabled.
    pub fn effective_trigger_input(&self) -> Option<InputTrigger> {
        self.visual_program.trigger.trigger_input()
    }
}
