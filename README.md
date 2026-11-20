# ClickForge

ClickForge is a Linux desktop application written in Rust (`eframe`/`egui`) for authoring, organizing, and running automated mouse and keyboard scripts under Wayland and X11. It supports a zero-compile visual block editor alongside an isolated native Rust scripting mode backed by `/dev/input/event*` (`evdev`) and `/dev/uinput`.

## Features

- **Target window and process attachment:** Two-tab picker (`Windows / Games` and `Processes`) combining live compositor window metadata (`niri` native socket IPC, Hyprland, Sway, X11 `xprop`) with native `/proc` scanning for Wine/Proton `.exe` binaries and `SteamAppId` values.
- **Scoped background execution:** Scripts run either in `Selected Game` scope (focus-gated using persistent App/Exe + Title matching or `Strict PID` mode) or `Global` scope. Multiple scripts remain enabled concurrently while a background coordinator activates profiles on focus change and updates a Linux `StatusNotifierItem` tray icon via `ksni`.
- **Visual block editor:** Drag-and-drop puzzle blocks and nested C-blocks (`If`, `If / Else`, `While`, `Repeat`) interpreted directly without compilation, with live active-block highlighting, local numeric variables, humanized timing (fixed, random uniform, and clamped Gaussian delays), and one-click export to native Rust.
- **Advanced native Rust mode:** Scripts compile against `clickforge_sdk` via a cached local Cargo project and execute inside a separate child process communicating over a per-run Unix domain socket (`CLICKFORGE_IPC_SOCKET`), isolating panics or infinite loops from the desktop app.
- **Mandatory Hard Kill-Switch gate:** Saving, compiling, and enabling any script is blocked until a dedicated Hard Kill-Switch key (such as `F8`) is assigned and verified not to collide with the trigger key, arm-toggle key, emitted keys, or condition keys.
- **Optional mouse trigger interception:** Per-script `Intercept Trigger` uses temporary `EVIOCGRAB` on physical pointer devices only while the script is armed and focused, forwarding pointer motion, scroll, and other buttons through `/dev/uinput` while suppressing raw trigger button events.

## Linux Input Permissions

ClickForge reads physical hardware input from `/dev/input/event*` and injects virtual events through `/dev/uinput` without running as root. Your user account needs group membership in `input` and read/write permissions on `/dev/uinput`.

Run the included helper script if `/dev/uinput` or `/dev/input/event*` reports permission denied or `No such device` (when the `uinput` kernel module is not loaded yet):

```bash
sudo ./scripts/install_udev_rules.sh
```

The script writes `/etc/udev/rules.d/99-clickforge-uinput.rules`, registers `/etc/modules-load.d/clickforge-uinput.conf`, reloads `udevadm`, and adds the invoking user to the `input` group. Log out and back in if group membership was updated.

To verify permissions without modifying system files:

```bash
id -nG
ls -la /dev/uinput
```

If `/dev/uinput` or `/dev/input/event*` is unavailable, ClickForge still launches in read-only authoring mode and reports the exact device error in the Telemetry & Logs drawer while refusing any `EVIOCGRAB` request.

## Build, Verification, and Run

Build the release binary:

```bash
./scripts/build_release.sh
```

Launch ClickForge with informational logging:

```bash
RUST_LOG=info ./target/release/clickforge
```

Run the complete verification gate (release overflow-checks check, 800-line file ceiling check, zero production `.unwrap()`/`.expect()` audit, `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo build --release`):

```bash
./scripts/verify_all.sh
```

## Wayland, niri, and Steam Proton Targeting

Under Wayland compositors such as `niri`, XWayland games launched through Steam Proton report the `xwayland-satellite` proxy PID as the window PID while exposing the window title and `app_id` (for example `steam_app_2560650` or `steam_app_default`).

ClickForge resolves the real game executable by scanning `/proc` (up to `MAX_PROCESS_SCAN_ENTRIES = 4096` processes per pass), extracting Windows `.exe` basenames from `/proc/<pid>/cmdline`, reading `SteamAppId` / `STEAM_COMPAT_APP_ID` / `SteamGameId` from `/proc/<pid>/environ` (with parent-PID inheritance up to 8 ancestors), and correlating those entries with live compositor windows.

Two matching modes are available in the target picker:

| Mode | Behavior |
| --- | --- |
| `PersistentAppAndTitle` | Matches across game restarts using `app_id`, window title substring, Steam App ID, and `.exe` name. |
| `StrictPid` | Matches only the specific running instance PID (including correlated Proton PIDs behind `xwayland-satellite`). |

When focus leaves the matched window (for example after Alt-Tab), the coordinator stops the script worker, releases any active `EVIOCGRAB`, and releases all pressed virtual buttons and keys.

## Visual Blocks, Kill-Switch, and Trigger Interception

A seeded starter script (`Building Upgrade Rapid Clicker`) is created on first launch when `~/.config/clickforge/scripts/` is empty:

- Trigger: `While Left Mouse is held`
- Action 1: `Wait 45 ms + Gaussian offset [10.0..20.0] ms (mean 15.0 ms, std_dev 2.5 ms)`
- Action 2: `Mouse Click Left (12 ms hold)`
- Hard Kill-Switch: `F8`, Arm Toggle: `F6`, Intercept Trigger: enabled

Safety rules enforced across validation, repository persistence, Rust compilation, the background coordinator, and the desktop UI:

1. Blank scripts start with `kill_switch_key = None`. Save, Compile & Check, and Enable controls remain blocked until a dedicated Hard Kill-Switch key is bound.
2. The Hard Kill-Switch key cannot equal the script's trigger key, optional arm-toggle key, any key emitted by `KeyTap`/`KeyDown`/`KeyUp`/`TypeText`, or any key checked in an `IsInputPressed` condition.
3. Pressing the bound Kill-Switch key halts all active visual and Rust workers, releases `EVIOCGRAB`, sends `SIGKILL` to isolated Rust child processes, and releases every pressed virtual key and mouse button.
4. `Intercept Trigger` grabs physical pointer devices only (never keyboards), refuses grabs when `/dev/uinput` is offline, waits for a focus-click button release before engaging, and releases grabs on focus loss, forwarding error, `Drop`, `SIGINT`, or `SIGTERM`.

## Advanced Rust SDK Reference

Click **Export to Rust** in the block canvas to translate any Visual Block script into compiling Rust source, or create an `Advanced Rust` script directly. Every script defines:

```rust
use clickforge_sdk::{GaussianRangeMs, MouseButton, ScriptContext, ScriptResult};

pub fn run_script(script_context: &mut ScriptContext) -> ScriptResult<()> {
    let jitter_range_ms = GaussianRangeMs::new(10.0, 20.0, 15.0, 2.5)?;
    while !script_context.should_stop() {
        if script_context.is_mouse_pressed(MouseButton::Left)? {
            script_context.wait_base_plus_gaussian_ms(45, jitter_range_ms)?;
            script_context.mouse_click(MouseButton::Left, 12)?;
        } else {
            script_context.wait_fixed_ms(5)?;
        }
    }
    Ok(())
}
```

### `ScriptContext` Methods

| Method | Signature | Description |
| --- | --- | --- |
| `mouse_click` | `(&mut self, mouse_button: MouseButton, hold_duration_ms: u64) -> ScriptResult<()>` | Presses and releases a virtual mouse button (max hold `60_000` ms). |
| `mouse_down` | `(&mut self, mouse_button: MouseButton) -> ScriptResult<()>` | Holds a virtual mouse button down until released or aborted. |
| `mouse_up` | `(&mut self, mouse_button: MouseButton) -> ScriptResult<()>` | Releases a virtual mouse button. |
| `mouse_move_relative` | `(&mut self, delta_x_px: i32, delta_y_px: i32) -> ScriptResult<()>` | Moves the virtual pointer by a relative offset in pixels. |
| `mouse_scroll` | `(&mut self, vertical_steps: i32) -> ScriptResult<()>` | Emits vertical scroll wheel steps (positive up, negative down). |
| `key_tap` | `(&mut self, key_code: KeyCode, hold_duration_ms: u64) -> ScriptResult<()>` | Presses and releases a virtual key (max hold `60_000` ms). |
| `key_down` | `(&mut self, key_code: KeyCode) -> ScriptResult<()>` | Holds a virtual key down until released or aborted. |
| `key_up` | `(&mut self, key_code: KeyCode) -> ScriptResult<()>` | Releases a virtual key. |
| `type_text` | `(&mut self, text: &str, keystroke_delay_ms: u64, jitter_ms: u64) -> ScriptResult<()>` | Types ASCII text with Shift handling and per-keystroke jitter (max `16_384` bytes). |
| `is_mouse_pressed` | `(&mut self, mouse_button: MouseButton) -> ScriptResult<bool>` | Reads current physical mouse button state from `evdev`. |
| `is_key_pressed` | `(&mut self, key_code: KeyCode) -> ScriptResult<bool>` | Reads current physical keyboard key state from `evdev`. |
| `should_stop` | `(&mut self) -> bool` | Returns `true` on stop, focus loss, Kill-Switch press, or IPC disconnect. |
| `wait_fixed_ms` | `(&mut self, delay_ms: u64) -> ScriptResult<()>` | Sleeps in 2 ms slices checking `should_stop()` (max `600_000` ms). |
| `wait_uniform_ms` | `(&mut self, min_delay_ms: u64, max_delay_ms: u64) -> ScriptResult<()>` | Sleeps an interruptible uniform integer duration in `[min_delay_ms, max_delay_ms]`. |
| `wait_gaussian_ms` | `(&mut self, range_ms: GaussianRangeMs) -> ScriptResult<()>` | Sleeps an interruptible clamped Gaussian delay. |
| `wait_base_plus_gaussian_ms` | `(&mut self, base_delay_ms: u64, offset_range_ms: GaussianRangeMs) -> ScriptResult<()>` | Adds a clamped Gaussian offset to `base_delay_ms` and sleeps interruptibly. |
| `log_info` | `(&mut self, message: impl AsRef<str>) -> ScriptResult<()>` | Appends a log entry (max `4_096` bytes) to the telemetry drawer. |
| `last_sampled_delay_ms` | `(&self) -> Option<u64>` | Returns the most recent delay duration recorded by the context. |

### Supporting SDK Types

- `MouseButton`: `Left`, `Right`, `Middle`, `Side`, `Extra`.
- `KeyCode`: 103 keys (`A`..`Z`, `Digit0`..`Digit9`, `F1`..`F12`, modifiers, arrows, navigation, punctuation, numpad).
- `GaussianRangeMs`: `GaussianRangeMs::new(min_ms, max_ms, mean_ms, std_dev_ms)` validates finite non-negative bounds (`min_ms <= mean_ms <= max_ms <= 600_000.0`, `std_dev_ms >= 0.0`) and clamps samples strictly within `[min_ms, max_ms]`.

## Storage Locations

- Scripts: `~/.config/clickforge/scripts/<script_id>.json`
- Rotated script backups (capped at `10` per script): `~/.config/clickforge/backups/`
- Application settings: `~/.config/clickforge/settings.json`
- Pre-warmed Rust script build cache: `~/.local/share/clickforge/script_build_cache/`
