use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use rand::RngExt;

use crate::input_types::{GaussianRangeMs, KeyCode, MouseButton};
use crate::ipc_protocol::{
    IPC_SOCKET_ENV_VAR, IpcRequest, IpcResponse, read_ipc_message, write_ipc_message,
};
use crate::{ScriptError, ScriptResult};

/// Sleep chunk granularity in milliseconds so kill-switch checks abort waits immediately.
pub const INTERRUPT_SLICE_MS: u64 = 2;

/// Maximum permitted single wait duration in milliseconds (10 minutes).
pub const MAX_WAIT_DURATION_MS: u64 = 600_000;

/// Maximum permitted single key or mouse hold duration in milliseconds (1 minute).
pub const MAX_HOLD_DURATION_MS: u64 = 60_000;

/// Maximum permitted byte length for a single `type_text` call.
pub const MAX_TYPE_TEXT_BYTES: usize = 16_384;

/// Maximum permitted byte length for a single `log_info` message.
pub const MAX_LOG_MESSAGE_BYTES: usize = 4_096;

/// Client context connecting an isolated Rust script to the ClickForge host over Unix IPC.
pub struct ScriptContext {
    reader: BufReader<UnixStream>,
    writer: BufWriter<UnixStream>,
    is_stop_requested: bool,
    last_sampled_delay_ms: Option<u64>,
}

impl ScriptContext {
    /// Connects to the host Unix domain socket advertised in `CLICKFORGE_IPC_SOCKET`.
    pub fn connect_from_env() -> ScriptResult<Self> {
        let socket_path =
            std::env::var(IPC_SOCKET_ENV_VAR).map_err(|_| ScriptError::MissingSocketEnvVar {
                variable_name: IPC_SOCKET_ENV_VAR,
            })?;
        Self::connect_to_path(socket_path)
    }

    /// Connects to an explicit Unix domain socket path.
    pub fn connect_to_path<PathType: AsRef<Path>>(socket_path: PathType) -> ScriptResult<Self> {
        let path_ref = socket_path.as_ref();
        let stream =
            UnixStream::connect(path_ref).map_err(|source| ScriptError::IpcConnectionFailed {
                socket_path: path_ref.display().to_string(),
                source,
            })?;
        Self::from_stream(stream)
    }

    /// Wraps an already connected Unix stream (used directly by tests and embedded harnesses).
    pub fn from_stream(stream: UnixStream) -> ScriptResult<Self> {
        let write_stream = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer: BufWriter::new(write_stream),
            is_stop_requested: false,
            last_sampled_delay_ms: None,
        })
    }

    /// Presses and releases a mouse button with a bounded hold duration in milliseconds.
    pub fn mouse_click(
        &mut self,
        mouse_button: MouseButton,
        hold_duration_ms: u64,
    ) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        validate_hold_duration_ms(hold_duration_ms)?;
        self.send_ack_request(IpcRequest::MouseClick {
            mouse_button,
            hold_duration_ms,
        })
    }

    /// Holds down a virtual mouse button until released or cleared on stop.
    pub fn mouse_down(&mut self, mouse_button: MouseButton) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        self.send_ack_request(IpcRequest::MouseDown { mouse_button })
    }

    /// Releases a previously pressed virtual mouse button.
    pub fn mouse_up(&mut self, mouse_button: MouseButton) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        self.send_ack_request(IpcRequest::MouseUp { mouse_button })
    }

    /// Moves the virtual pointer by a relative pixel offset.
    pub fn mouse_move_relative(&mut self, delta_x_px: i32, delta_y_px: i32) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        self.send_ack_request(IpcRequest::MouseMoveRelative {
            delta_x_px,
            delta_y_px,
        })
    }

    /// Emits vertical mouse wheel scroll steps (positive up, negative down).
    pub fn mouse_scroll(&mut self, vertical_steps: i32) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        self.send_ack_request(IpcRequest::MouseScroll { vertical_steps })
    }

    /// Presses and releases a keyboard key with a bounded hold duration in milliseconds.
    pub fn key_tap(&mut self, key_code: KeyCode, hold_duration_ms: u64) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        validate_hold_duration_ms(hold_duration_ms)?;
        self.send_ack_request(IpcRequest::KeyTap {
            key_code,
            hold_duration_ms,
        })
    }

    /// Holds down a virtual keyboard key until released or cleared on stop.
    pub fn key_down(&mut self, key_code: KeyCode) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        self.send_ack_request(IpcRequest::KeyDown { key_code })
    }

    /// Releases a previously pressed virtual keyboard key.
    pub fn key_up(&mut self, key_code: KeyCode) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        self.send_ack_request(IpcRequest::KeyUp { key_code })
    }

    /// Types ASCII text with per-keystroke base delay and symmetric jitter in milliseconds.
    pub fn type_text(
        &mut self,
        text: &str,
        keystroke_delay_ms: u64,
        jitter_ms: u64,
    ) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        if text.len() > MAX_TYPE_TEXT_BYTES {
            return Err(ScriptError::InvalidArgument {
                reason: format!(
                    "type_text payload of {} bytes exceeds limit of {} bytes",
                    text.len(),
                    MAX_TYPE_TEXT_BYTES
                ),
            });
        }
        validate_wait_duration_ms(keystroke_delay_ms)?;
        validate_wait_duration_ms(jitter_ms)?;
        self.send_ack_request(IpcRequest::TypeText {
            text: text.to_string(),
            keystroke_delay_ms,
            jitter_ms,
        })
    }

    /// Queries whether a physical mouse button is currently held down.
    pub fn is_mouse_pressed(&mut self, mouse_button: MouseButton) -> ScriptResult<bool> {
        if self.is_stop_requested {
            return Ok(false);
        }
        self.send_bool_request(IpcRequest::QueryMousePressed { mouse_button })
    }

    /// Queries whether a physical keyboard key is currently held down.
    pub fn is_key_pressed(&mut self, key_code: KeyCode) -> ScriptResult<bool> {
        if self.is_stop_requested {
            return Ok(false);
        }
        self.send_bool_request(IpcRequest::QueryKeyPressed { key_code })
    }

    /// Returns true once the host signals a stop, focus loss, kill-switch, or IPC disconnect.
    pub fn should_stop(&mut self) -> bool {
        if self.is_stop_requested {
            return true;
        }
        match self.send_bool_request(IpcRequest::QueryShouldStop) {
            Ok(true) => {
                self.is_stop_requested = true;
                true
            }
            Ok(false) => false,
            Err(_) => {
                self.is_stop_requested = true;
                true
            }
        }
    }

    /// Sleeps for a fixed duration in 2 ms interruptible chunks checking `should_stop()`.
    pub fn wait_fixed_ms(&mut self, delay_ms: u64) -> ScriptResult<()> {
        validate_wait_duration_ms(delay_ms)?;
        self.last_sampled_delay_ms = Some(delay_ms);
        if delay_ms == 0 || self.should_stop() {
            return Ok(());
        }
        let target_duration = Duration::from_millis(delay_ms);
        let started_at = Instant::now();
        loop {
            let elapsed = started_at.elapsed();
            if elapsed >= target_duration {
                break;
            }
            let remaining = target_duration.saturating_sub(elapsed);
            let slice_duration = remaining.min(Duration::from_millis(INTERRUPT_SLICE_MS));
            std::thread::sleep(slice_duration);
            if self.should_stop() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Samples a uniform integer delay within `[min_delay_ms, max_delay_ms]` and sleeps interruptibly.
    pub fn wait_uniform_ms(&mut self, min_delay_ms: u64, max_delay_ms: u64) -> ScriptResult<()> {
        if min_delay_ms > max_delay_ms {
            return Err(ScriptError::InvalidTimingRange {
                reason: format!(
                    "min_delay_ms ({min_delay_ms}) cannot exceed max_delay_ms ({max_delay_ms})"
                ),
            });
        }
        validate_wait_duration_ms(max_delay_ms)?;
        let sampled_delay_ms = if min_delay_ms == max_delay_ms {
            min_delay_ms
        } else {
            rand::rng().random_range(min_delay_ms..=max_delay_ms)
        };
        self.wait_fixed_ms(sampled_delay_ms)
    }

    /// Samples a clamped Gaussian delay in milliseconds and sleeps interruptibly.
    pub fn wait_gaussian_ms(&mut self, range_ms: GaussianRangeMs) -> ScriptResult<()> {
        range_ms.validate()?;
        let sampled_delay_ms = range_ms.sample_clamped_ms().round().max(0.0) as u64;
        self.wait_fixed_ms(sampled_delay_ms)
    }

    /// Adds a clamped Gaussian jitter offset to `base_delay_ms` and sleeps interruptibly.
    pub fn wait_base_plus_gaussian_ms(
        &mut self,
        base_delay_ms: u64,
        offset_range_ms: GaussianRangeMs,
    ) -> ScriptResult<()> {
        validate_wait_duration_ms(base_delay_ms)?;
        offset_range_ms.validate()?;
        let offset_ms = offset_range_ms.sample_clamped_ms().round().max(0.0) as u64;
        let total_delay_ms = base_delay_ms.checked_add(offset_ms).ok_or_else(|| {
            ScriptError::InvalidTimingRange {
                reason: "base_delay_ms + Gaussian offset overflowed u64".to_string(),
            }
        })?;
        self.wait_fixed_ms(total_delay_ms)
    }

    /// Sends an informational log line to the ClickForge telemetry drawer.
    pub fn log_info(&mut self, message: impl AsRef<str>) -> ScriptResult<()> {
        if self.is_stop_requested {
            return Ok(());
        }
        let message_text = message.as_ref();
        if message_text.len() > MAX_LOG_MESSAGE_BYTES {
            return Err(ScriptError::InvalidArgument {
                reason: format!(
                    "log_info message of {} bytes exceeds limit of {} bytes",
                    message_text.len(),
                    MAX_LOG_MESSAGE_BYTES
                ),
            });
        }
        self.send_ack_request(IpcRequest::LogInfo {
            message: message_text.to_string(),
        })
    }

    /// Returns the most recent delay duration sampled or waited by this context.
    pub fn last_sampled_delay_ms(&self) -> Option<u64> {
        self.last_sampled_delay_ms
    }

    fn send_ack_request(&mut self, request: IpcRequest) -> ScriptResult<()> {
        match self.exchange_request(request)? {
            IpcResponse::Ack => Ok(()),
            IpcResponse::Error { message } => Err(ScriptError::HostRejectedRequest { message }),
            other_response => Err(ScriptError::UnexpectedResponse {
                expected: "ack",
                actual: format!("{other_response:?}"),
            }),
        }
    }

    fn send_bool_request(&mut self, request: IpcRequest) -> ScriptResult<bool> {
        match self.exchange_request(request)? {
            IpcResponse::BoolValue { is_true } => Ok(is_true),
            IpcResponse::Error { message } => Err(ScriptError::HostRejectedRequest { message }),
            other_response => Err(ScriptError::UnexpectedResponse {
                expected: "bool_value",
                actual: format!("{other_response:?}"),
            }),
        }
    }

    fn exchange_request(&mut self, request: IpcRequest) -> ScriptResult<IpcResponse> {
        write_ipc_message(&mut self.writer, &request)?;
        let maybe_response = read_ipc_message(&mut self.reader)?;
        maybe_response.ok_or(ScriptError::IpcConnectionClosed)
    }
}

fn validate_wait_duration_ms(delay_ms: u64) -> ScriptResult<()> {
    if delay_ms > MAX_WAIT_DURATION_MS {
        return Err(ScriptError::InvalidTimingRange {
            reason: format!(
                "wait duration {delay_ms} ms exceeds limit of {MAX_WAIT_DURATION_MS} ms"
            ),
        });
    }
    Ok(())
}

fn validate_hold_duration_ms(hold_duration_ms: u64) -> ScriptResult<()> {
    if hold_duration_ms > MAX_HOLD_DURATION_MS {
        return Err(ScriptError::InvalidTimingRange {
            reason: format!(
                "hold duration {hold_duration_ms} ms exceeds limit of {MAX_HOLD_DURATION_MS} ms"
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, BufWriter};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use super::*;

    #[test]
    fn context_sends_actions_and_interrupts_wait_on_stop() -> ScriptResult<()> {
        let (client_stream, server_stream) = UnixStream::pair()?;
        let stop_flag = Arc::new(AtomicBool::new(false));
        let server_stop_flag = Arc::clone(&stop_flag);

        let server_thread = thread::spawn(move || -> ScriptResult<Vec<IpcRequest>> {
            let write_stream = server_stream.try_clone()?;
            let mut server_reader = BufReader::new(server_stream);
            let mut server_writer = BufWriter::new(write_stream);
            let mut received_requests = Vec::new();

            while let Some(request) = read_ipc_message::<_, IpcRequest>(&mut server_reader)? {
                let response = match &request {
                    IpcRequest::QueryMousePressed { .. } => {
                        IpcResponse::BoolValue { is_true: true }
                    }
                    IpcRequest::QueryKeyPressed { .. } => IpcResponse::BoolValue { is_true: false },
                    IpcRequest::QueryShouldStop => IpcResponse::BoolValue {
                        is_true: server_stop_flag.load(Ordering::SeqCst),
                    },
                    _ => IpcResponse::Ack,
                };
                received_requests.push(request);
                write_ipc_message(&mut server_writer, &response)?;
            }
            Ok(received_requests)
        });

        let mut script_context = ScriptContext::from_stream(client_stream)?;
        assert!(script_context.is_mouse_pressed(MouseButton::Left)?);
        script_context.mouse_click(MouseButton::Left, 12)?;
        script_context.log_info("clicked once")?;

        let trigger_stop_flag = Arc::clone(&stop_flag);
        let stopper_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(12));
            trigger_stop_flag.store(true, Ordering::SeqCst);
        });

        let wait_started = Instant::now();
        script_context.wait_fixed_ms(1_500)?;
        let waited_ms = wait_started.elapsed().as_millis();
        assert!(
            waited_ms < 250,
            "wait did not interrupt promptly: {waited_ms} ms"
        );
        assert!(script_context.should_stop());

        // Subsequent action after stop becomes a safe no-op.
        script_context.mouse_click(MouseButton::Right, 10)?;
        drop(script_context);

        let _ = stopper_thread.join();
        let recorded = match server_thread.join() {
            Ok(result) => result?,
            Err(_) => panic!("mock IPC server thread panicked"),
        };
        assert!(recorded.contains(&IpcRequest::MouseClick {
            mouse_button: MouseButton::Left,
            hold_duration_ms: 12,
        }));
        assert!(!recorded.contains(&IpcRequest::MouseClick {
            mouse_button: MouseButton::Right,
            hold_duration_ms: 10,
        }));
        Ok(())
    }
}
