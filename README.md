# ClickForge

ClickForge is a Linux desktop workbench written in Rust (`eframe`/`egui`) for authoring, organizing, and running automated mouse and keyboard scripts under Wayland and X11. Scripts run directly through a zero-compile visual snap-block interpreter or compile into isolated native Rust binaries backed by `/dev/input/event*` (`evdev`) and `/dev/uinput`.

## Workspace Layout

The left sidebar switches between two top-level views and groups scripts under `Selected Game` and `Global`:

- **`Active` tab (Live Control Center):** Displays the global engine status hero card, focused window metadata, hardware input readiness (`/dev/input/event*` and `/dev/uinput`), emergency stop and disable-all controls, and live status cards for every script. When `/dev/uinput` is unavailable, a top diagnostic banner reports the exact device error and setup command.
- **`Editor` tab (Script Builder):** Combines the script settings bar (scope, target attachment, Hard Kill-Switch capture, optional Arm Toggle capture, `Intercept Trigger`, Save, Revert, and Enable controls) with either the Visual Blocks palette and canvas or the native Rust editor panel.
- **Collapsible Telemetry Drawer and Tray:** The bottom `Telemetry` drawer streams runtime and compiler logs alongside a Gaussian delay histogram. A `StatusNotifierItem` tray icon (`ksni`) tracks active execution, while `Ctrl+S` saves, `Ctrl+Z` / `Ctrl+Shift+Z` (or `Ctrl+Y`) undo and redo visual block edits, and `Ctrl+Q` or the sidebar `Quit` button exits cleanly.

## Target Window and Process Picker

ClickForge uses a Cheat Engine style two-tab picker (`Windows / Games` and `Processes`) combining compositor window inspection with native Linux `/proc` scanning.

- **Compositor backends:** Queries open and focused windows from `niri` via native Unix socket IPC, Hyprland, Sway, and X11 (`xprop`).
- **Steam and Proton correlation:** Under Wayland compositors such as `niri`, XWayland games launched through Proton report the `xwayland-satellite` proxy PID as the window PID while exposing `app_id` values such as `steam_app_2560650` or `steam_app_default`. ClickForge scans `/proc` (up to `4096` processes per pass), extracts Windows `.exe` basenames from `/proc/<pid>/cmdline`, and reads `SteamAppId`, `STEAM_COMPAT_APP_ID`, and `SteamGameId` from `/proc/<pid>/environ` with parent-PID inheritance up to `8` ancestors to link the window to the real game executable.

| Matching Mode | Behavior |
| --- | --- |
| `PersistentAppAndTitle` | Matches across game restarts using `app_id`, window title substring, Steam App ID, and `.exe` name. |
| `StrictPid` | Matches only the selected running instance PID, including correlated Proton PIDs behind `xwayland-satellite`. |

Multiple scripts can remain enabled concurrently while a background coordinator activates each profile on focus match. When focus leaves a matched window, the coordinator stops the worker, releases any active `EVIOCGRAB`, and releases all pressed virtual buttons and keys.

## Visual Snap-Blocks and Reusable Custom Tiles

The visual editor provides 26 built-in drag-and-drop blocks across `Triggers`, `Control`, `Actions`, `Timing`, and `Vars`, plus a `My Blocks` section for user-defined tiles.

- **Trigger Hat and `+ AND` chords:** Supports `While Input Held`, `Toggle on Input`, `Once on Press`, and `Loop While Active`. Clicking `+ AND` on an input trigger attaches up to `4` additional simultaneous inputs (`MAX_TRIGGER_AND_INPUTS = 4`), requiring every input in the chord to be held together.
- **Chord-aware `Intercept Trigger`:** Uses temporary `EVIOCGRAB` on physical pointer devices only while a script is armed and focused. Pointer motion, scroll, and unrelated buttons forward through `/dev/uinput`, while the primary trigger button press is suppressed only when all required `+ AND` chord inputs are physically held; otherwise the raw press passes through normally.
- **Control and action blocks:** Includes nested C-blocks (`If`, `If / Else`, `While`, `Repeat N Times`), `Break Loop`, `Stop Script`, `Do Nothing` (an explicit no-op placeholder inside branches such as `If / Else`), `Mouse Click`, `Mouse Hold / Drag` (`MouseHoldWhilePressed`, holding a virtual mouse button down while a watched physical input stays pressed so real clicks and drags work), `Mouse Down`, `Mouse Up`, `Mouse Move`, `Mouse Scroll`, `Key Tap`, `Key Down`, `Key Up`, `Type Text`, and local numeric variables (`Set Variable`, `Change Variable`).
- **Humanized timing:** `Wait Fixed`, `Wait Uniform`, and `Wait Gaussian` (`WaitRandomGaussian`, adding a clamped Gaussian jitter offset to a base millisecond delay) sleep in interruptible slices and highlight the active block live on the canvas.
- **Reusable Custom Tiles (`My Blocks`):** Create up to `64` reusable multi-step tiles (up to `64` steps per tile, nesting depth up to `8` with cycle rejection) via `+ New Custom Tile` in the palette or `Save as Custom Tile...` in any block's right-click menu. Drag custom tiles from `My Blocks` into any script, modify definitions in place with `Edit Custom Tile`, or expand a call into editable canvas blocks with `Unpack Inline`.

A seeded starter script (`Building Upgrade Rapid Clicker`) is created on first launch when `~/.config/clickforge/scripts/` is empty:

- Trigger: `While Left Mouse is held`
- Step 1: `Wait 45 ms + Gaussian offset [10.0..20.0] ms (mean 15.0 ms, std_dev 2.5 ms)`
- Step 2: `Mouse Click Left (12 ms hold)`
- Bindings: Hard Kill-Switch `F8`, Arm Toggle `F6`, `Intercept Trigger` enabled

## Mandatory Hard Kill-Switch Safety Gate

Safety checks run across script validation, repository persistence, Rust compilation, the background coordinator, and the desktop UI:

1. Blank scripts initialize with `kill_switch_key = None`. Save, `Compile & Check`, and Enable controls remain locked until a dedicated Hard Kill-Switch key is bound.
2. Validation blocks collisions between the Hard Kill-Switch key and any trigger key (including `+ AND` chord inputs), the optional Arm Toggle key, any key emitted by `KeyTap`, `KeyDown`, `KeyUp`, or `TypeText`, any key checked in `IsInputPressed` or watched by `Mouse Hold / Drag`, and any key emitted or checked inside referenced `My Blocks` custom tiles.
3. Pressing the bound Kill-Switch key halts all active visual and Rust workers, releases `EVIOCGRAB`, sends `SIGKILL` to isolated Rust child processes, and releases every pressed virtual key and mouse button.
4. `Intercept Trigger` grabs physical pointer devices only (never keyboards), refuses grabs when `/dev/uinput` is offline, waits for a focus-click button release before engaging, and releases grabs on focus loss, forwarding error, `Drop`, `SIGINT`, or `SIGTERM`.

## Advanced Native Rust Mode

Click **Export to Rust** in the block canvas to convert any Visual Block script (including chord triggers, `Mouse Hold / Drag`, and typed helper functions for `My Blocks` custom tiles) into compiling Rust source, or author an `Advanced Rust` script directly.

Scripts compile against `crates/clickforge_sdk` inside a pre-warmed local Cargo build cache (`~/.local/share/clickforge/script_build_cache/`) and execute in an isolated child process communicating over a per-run Unix domain socket (`CLICKFORGE_IPC_SOCKET`). An RAII process guard terminates the child with `SIGKILL`, reaps the process, deletes the socket file, and releases all virtual inputs on stop, panic, focus loss, or Kill-Switch activation.

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

Supporting SDK types include `MouseButton` (`Left`, `Right`, `Middle`, `Side`, `Extra`), `KeyCode` (`103` keys covering `A`..`Z`, `Digit0`..`Digit9`, `F1`..`F12`, modifiers, arrows, navigation, punctuation, and numpad), and `GaussianRangeMs::new(min_ms, max_ms, mean_ms, std_dev_ms)` (validating `0.0 <= min_ms <= mean_ms <= max_ms <= 600_000.0` and `std_dev_ms >= 0.0`, clamping samples strictly within `[min_ms, max_ms]`).

## Linux Input Permissions

ClickForge reads physical hardware input from `/dev/input/event*` and injects virtual events through `/dev/uinput` without running as root. The user account needs membership in the `input` group and read/write permissions on `/dev/uinput`.

Run the included udev setup script if `/dev/uinput` or `/dev/input/event*` reports permission denied or `No such device`:

```bash
sudo ./scripts/install_udev_rules.sh
```

The script writes `/etc/udev/rules.d/99-clickforge-uinput.rules`, registers `/etc/modules-load.d/clickforge-uinput.conf`, runs `modprobe uinput`, reloads `udevadm`, and adds the invoking user to the `input` group. Log out and back in if group membership was updated.

Verify permissions without modifying system files:

```bash
id -nG
ls -la /dev/uinput
```

When `/dev/uinput` or `/dev/input/event*` is unavailable, ClickForge launches in authoring mode, displays the exact device error in the top diagnostic banner and `Telemetry` drawer, and refuses any `EVIOCGRAB` request.

## Build, Verification, and Run

Build the workspace release binary:

```bash
./scripts/build_release.sh
```

Launch ClickForge with informational logging:

```bash
RUST_LOG=info ./target/release/clickforge
```

Run the full 7-step verification suite (`[profile.release] overflow-checks = true`, 800-line file ceiling, zero production `.unwrap()`/`.expect()`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo build --release`):

```bash
./scripts/verify_all.sh
```

## Desktop Launcher Installation

Install the release binary, scalable SVG icon (`assets/clickforge.svg`), and XDG desktop entry:

```bash
./scripts/install_desktop_app.sh
```

If `target/release/clickforge` is missing, the installer runs `cargo build --release` first, then installs:

- Binary: `${XDG_BIN_HOME:-~/.local/bin}/clickforge`
- Icon: `${XDG_DATA_HOME:-~/.local/share}/icons/hicolor/scalable/apps/clickforge.svg`
- Desktop entry: `${XDG_DATA_HOME:-~/.local/share}/applications/clickforge.desktop` (setting `StartupWMClass=clickforge` to match `WAYLAND_APP_ID` in `src/main.rs`)

When present on the host, the script runs `desktop-file-validate`, `update-desktop-database`, and `gtk-update-icon-cache` (if `index.theme` exists).

## Storage Locations

- Scripts: `~/.config/clickforge/scripts/<script_id>.json`
- Reusable custom tiles (`My Blocks`): `~/.config/clickforge/custom_blocks.json`
- Rotated script backups (capped at `10` per script): `~/.config/clickforge/backups/`
- Application settings: `~/.config/clickforge/settings.json`
- Pre-warmed Rust script build cache: `~/.local/share/clickforge/script_build_cache/`
