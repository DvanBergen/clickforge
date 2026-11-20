use std::fs;
use std::io::{BufWriter, ErrorKind, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clickforge_sdk::{
    IPC_SOCKET_ENV_VAR, IpcRequest, IpcResponse, MAX_IPC_LINE_BYTES, write_ipc_message,
};
use rand::RngExt;

pub use crate::engine::rust_compiler::{
    compile_rust_script, default_script_build_cache_dir, resolve_clickforge_sdk_path,
};
use crate::engine::timing::{
    perform_key_tap_on_sink, perform_mouse_click_on_sink, perform_type_text_on_sink,
};
use crate::models::{InputSink, PhysicalInputReader, ScriptDocument};

const IPC_CONNECT_POLL_SLICE_MS: u64 = 2;
const CHILD_CONNECT_TIMEOUT_MS: u64 = 5_000;
const CHILD_EXIT_GRACE_TIMEOUT_MS: u64 = 250;
const IPC_READ_TIMEOUT_MS: u64 = 5;
const IPC_WRITE_TIMEOUT_MS: u64 = 500;
const IPC_READ_CHUNK_BYTES: usize = 4_096;
const CHILD_STDERR_LIMIT_BYTES: usize = 4_096;

/// Optional telemetry callbacks invoked while hosting an isolated compiled Rust script.
#[derive(Default)]
pub struct RustExecutionHooks {
    pub on_delay_sampled_ms: Option<Box<dyn FnMut(u64) + Send>>,
    pub on_action_executed: Option<Box<dyn FnMut() + Send>>,
    pub on_log_message: Option<Box<dyn FnMut(String) + Send>>,
}

impl RustExecutionHooks {
    fn report_delay_sampled_ms(&mut self, delay_ms: u64) {
        if let Some(callback) = self.on_delay_sampled_ms.as_mut() {
            callback(delay_ms);
        }
    }

    fn report_action_executed(&mut self) {
        if let Some(callback) = self.on_action_executed.as_mut() {
            callback();
        }
    }

    fn report_log_message(&mut self, message: String) {
        if let Some(callback) = self.on_log_message.as_mut() {
            callback(message);
        }
    }
}

struct IsolatedChildGuard {
    child_process: Option<Child>,
    socket_path: PathBuf,
    input_sink: Arc<dyn InputSink>,
}

impl IsolatedChildGuard {
    fn terminate_and_reap(&mut self) {
        if let Some(mut child) = self.child_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn read_stderr_summary(&mut self) -> String {
        let Some(child) = self.child_process.as_mut() else {
            return String::new();
        };
        let Some(mut stderr_pipe) = child.stderr.take() else {
            return String::new();
        };
        let mut buffer = Vec::new();
        let mut limited_reader = stderr_pipe.by_ref().take(CHILD_STDERR_LIMIT_BYTES as u64);
        let _ = limited_reader.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).trim().to_string()
    }
}

impl Drop for IsolatedChildGuard {
    fn drop(&mut self) {
        self.terminate_and_reap();
        let _ = fs::remove_file(&self.socket_path);
        let _ = self.input_sink.release_all_pressed();
    }
}

/// Spawns a compiled script binary connected to a dedicated Unix socket and serves SDK requests.
///
/// Guarantees immediate `SIGKILL`, child reaping, socket cleanup, and virtual input release via RAII.
pub fn run_compiled_rust_script(
    binary_path: &Path,
    input_sink: Arc<dyn InputSink>,
    physical_input: Arc<dyn PhysicalInputReader>,
    abort_flag: &AtomicBool,
    hooks: &mut RustExecutionHooks,
) -> Result<(), String> {
    if abort_flag.load(Ordering::SeqCst) {
        let _ = input_sink.release_all_pressed();
        return Ok(());
    }
    if !binary_path.is_file() {
        return Err(format!(
            "Compiled script binary does not exist at '{}'",
            binary_path.display()
        ));
    }

    let socket_path = build_unique_socket_path();
    let listener = UnixListener::bind(&socket_path).map_err(|bind_error| {
        format!(
            "Failed to bind IPC Unix socket '{}': {bind_error}",
            socket_path.display()
        )
    })?;
    listener.set_nonblocking(true).map_err(|config_error| {
        format!("Failed to set nonblocking IPC listener: {config_error}")
    })?;

    let spawned_child = Command::new(binary_path)
        .env(IPC_SOCKET_ENV_VAR, &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|spawn_error| {
            let _ = fs::remove_file(&socket_path);
            format!(
                "Failed to spawn compiled script '{}': {spawn_error}",
                binary_path.display()
            )
        })?;

    let mut child_guard = IsolatedChildGuard {
        child_process: Some(spawned_child),
        socket_path,
        input_sink: Arc::clone(&input_sink),
    };

    let Some(mut client_stream) = accept_child_connection(&listener, &mut child_guard, abort_flag)?
    else {
        return Ok(());
    };

    configure_stream_timeouts(&client_stream)?;
    let write_stream = client_stream
        .try_clone()
        .map_err(|clone_error| format!("Failed to clone IPC UnixStream: {clone_error}"))?;
    let mut response_writer = BufWriter::new(write_stream);
    let mut pending_buffer = Vec::new();

    while let Some(request) =
        read_next_ipc_request(&mut client_stream, &mut pending_buffer, abort_flag)?
    {
        let response = dispatch_ipc_request(
            request,
            input_sink.as_ref(),
            physical_input.as_ref(),
            abort_flag,
            hooks,
        );
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }
        if let Err(write_error) = write_ipc_message(&mut response_writer, &response) {
            if abort_flag.load(Ordering::SeqCst) {
                break;
            }
            return Err(format!(
                "Failed to send IPC response to script: {write_error}"
            ));
        }
    }

    if abort_flag.load(Ordering::SeqCst) {
        child_guard.terminate_and_reap();
        return Ok(());
    }
    verify_child_exit_status(&mut child_guard)
}

fn configure_stream_timeouts(stream: &UnixStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_millis(IPC_READ_TIMEOUT_MS)))
        .map_err(|timeout_error| {
            format!("Failed to configure IPC read timeout: {timeout_error}")
        })?;
    stream
        .set_write_timeout(Some(Duration::from_millis(IPC_WRITE_TIMEOUT_MS)))
        .map_err(|timeout_error| format!("Failed to configure IPC write timeout: {timeout_error}"))
}

fn accept_child_connection(
    listener: &UnixListener,
    child_guard: &mut IsolatedChildGuard,
    abort_flag: &AtomicBool,
) -> Result<Option<UnixStream>, String> {
    let deadline = Instant::now() + Duration::from_millis(CHILD_CONNECT_TIMEOUT_MS);
    loop {
        if abort_flag.load(Ordering::SeqCst) {
            return Ok(None);
        }
        match listener.accept() {
            Ok((stream, _peer_address)) => return Ok(Some(stream)),
            Err(accept_error) if accept_error.kind() == ErrorKind::WouldBlock => {}
            Err(accept_error) => return Err(format!("IPC socket accept failed: {accept_error}")),
        }
        if let Some(child) = child_guard.child_process.as_mut()
            && let Ok(Some(exit_status)) = child.try_wait()
        {
            let stderr_summary = child_guard.read_stderr_summary();
            child_guard.child_process = None;
            if exit_status.success() {
                return Ok(None);
            }
            return Err(format_child_exit_error(exit_status, &stderr_summary));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Timed out waiting {CHILD_CONNECT_TIMEOUT_MS} ms for script process to connect"
            ));
        }
        std::thread::sleep(Duration::from_millis(IPC_CONNECT_POLL_SLICE_MS));
    }
}

fn read_next_ipc_request(
    client_stream: &mut UnixStream,
    pending_buffer: &mut Vec<u8>,
    abort_flag: &AtomicBool,
) -> Result<Option<IpcRequest>, String> {
    let mut read_chunk = [0_u8; IPC_READ_CHUNK_BYTES];

    loop {
        if abort_flag.load(Ordering::SeqCst) {
            return Ok(None);
        }
        if let Some(newline_index) = pending_buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = pending_buffer.drain(..=newline_index).collect();
            if line_bytes.len() > MAX_IPC_LINE_BYTES {
                return Err(format!(
                    "IPC frame exceeded maximum of {MAX_IPC_LINE_BYTES} bytes"
                ));
            }
            let decoded_request: IpcRequest =
                serde_json::from_slice(&line_bytes).map_err(|decode_error| {
                    format!("Invalid IPC JSON frame from script: {decode_error}")
                })?;
            return Ok(Some(decoded_request));
        }
        if pending_buffer.len() > MAX_IPC_LINE_BYTES {
            return Err(format!(
                "IPC line buffer exceeded maximum of {MAX_IPC_LINE_BYTES} bytes"
            ));
        }

        match client_stream.read(&mut read_chunk) {
            Ok(0) => return Ok(None),
            Ok(bytes_read) => pending_buffer.extend_from_slice(&read_chunk[..bytes_read]),
            Err(io_error)
                if matches!(
                    io_error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(io_error) => return Err(format!("IPC socket read error: {io_error}")),
        }
    }
}

fn dispatch_ipc_request(
    request: IpcRequest,
    input_sink: &dyn InputSink,
    physical_input: &dyn PhysicalInputReader,
    abort_flag: &AtomicBool,
    hooks: &mut RustExecutionHooks,
) -> IpcResponse {
    let check_abort = || abort_flag.load(Ordering::SeqCst);

    match request {
        IpcRequest::MouseClick {
            mouse_button,
            hold_duration_ms,
        } => {
            let result = perform_mouse_click_on_sink(
                input_sink,
                mouse_button,
                hold_duration_ms,
                &check_abort,
            );
            hooks.report_action_executed();
            ack_or_error(result.map(|_| ()))
        }
        IpcRequest::MouseDown { mouse_button } => {
            hooks.report_action_executed();
            ack_or_error(
                input_sink
                    .mouse_down(mouse_button)
                    .map_err(|input_error| input_error.to_string()),
            )
        }
        IpcRequest::MouseUp { mouse_button } => {
            hooks.report_action_executed();
            ack_or_error(
                input_sink
                    .mouse_up(mouse_button)
                    .map_err(|input_error| input_error.to_string()),
            )
        }
        IpcRequest::MouseMoveRelative {
            delta_x_px,
            delta_y_px,
        } => {
            hooks.report_action_executed();
            ack_or_error(
                input_sink
                    .mouse_move_relative(delta_x_px, delta_y_px)
                    .map_err(|input_error| input_error.to_string()),
            )
        }
        IpcRequest::MouseScroll { vertical_steps } => {
            hooks.report_action_executed();
            ack_or_error(
                input_sink
                    .mouse_scroll(vertical_steps)
                    .map_err(|input_error| input_error.to_string()),
            )
        }
        IpcRequest::KeyTap {
            key_code,
            hold_duration_ms,
        } => {
            let result =
                perform_key_tap_on_sink(input_sink, key_code, hold_duration_ms, &check_abort);
            hooks.report_action_executed();
            ack_or_error(result.map(|_| ()))
        }
        IpcRequest::KeyDown { key_code } => {
            hooks.report_action_executed();
            ack_or_error(
                input_sink
                    .key_down(key_code)
                    .map_err(|input_error| input_error.to_string()),
            )
        }
        IpcRequest::KeyUp { key_code } => {
            hooks.report_action_executed();
            ack_or_error(
                input_sink
                    .key_up(key_code)
                    .map_err(|input_error| input_error.to_string()),
            )
        }
        IpcRequest::TypeText {
            text,
            keystroke_delay_ms,
            jitter_ms,
        } => handle_type_text_ipc(
            input_sink,
            &text,
            keystroke_delay_ms,
            jitter_ms,
            &check_abort,
            hooks,
        ),
        IpcRequest::QueryMousePressed { mouse_button } => IpcResponse::BoolValue {
            is_true: physical_input.is_mouse_pressed(mouse_button),
        },
        IpcRequest::QueryKeyPressed { key_code } => IpcResponse::BoolValue {
            is_true: physical_input.is_key_pressed(key_code),
        },
        IpcRequest::QueryShouldStop => IpcResponse::BoolValue {
            is_true: abort_flag.load(Ordering::SeqCst),
        },
        IpcRequest::LogInfo { message } => {
            hooks.report_log_message(message);
            IpcResponse::Ack
        }
    }
}

fn handle_type_text_ipc(
    input_sink: &dyn InputSink,
    text: &str,
    keystroke_delay_ms: u64,
    jitter_ms: u64,
    should_abort: &dyn Fn() -> bool,
    hooks: &mut RustExecutionHooks,
) -> IpcResponse {
    let mut sampled_delays_ms = Vec::new();
    let mut action_count: usize = 0;
    let result = perform_type_text_on_sink(
        input_sink,
        text,
        keystroke_delay_ms,
        jitter_ms,
        should_abort,
        &mut |delay_ms| sampled_delays_ms.push(delay_ms),
        &mut || action_count = action_count.saturating_add(1),
    );
    for delay_ms in sampled_delays_ms {
        hooks.report_delay_sampled_ms(delay_ms);
    }
    for _action_index in 0..action_count {
        hooks.report_action_executed();
    }
    ack_or_error(result.map(|_| ()))
}

fn ack_or_error(operation_result: Result<(), String>) -> IpcResponse {
    match operation_result {
        Ok(()) => IpcResponse::Ack,
        Err(message) => IpcResponse::Error { message },
    }
}

fn verify_child_exit_status(child_guard: &mut IsolatedChildGuard) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(CHILD_EXIT_GRACE_TIMEOUT_MS);
    loop {
        let Some(child) = child_guard.child_process.as_mut() else {
            return Ok(());
        };
        if let Ok(Some(exit_status)) = child.try_wait() {
            let stderr_summary = child_guard.read_stderr_summary();
            child_guard.child_process = None;
            if exit_status.success() {
                return Ok(());
            }
            return Err(format_child_exit_error(exit_status, &stderr_summary));
        }
        if Instant::now() >= deadline {
            child_guard.terminate_and_reap();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(IPC_CONNECT_POLL_SLICE_MS));
    }
}

fn format_child_exit_error(exit_status: std::process::ExitStatus, stderr_summary: &str) -> String {
    if stderr_summary.is_empty() {
        format!("Script process exited with status {exit_status}")
    } else {
        format!("Script process exited with status {exit_status}: {stderr_summary}")
    }
}

fn build_unique_socket_path() -> PathBuf {
    let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
    let random_suffix: u32 = rand::rng().random();
    let process_id = std::process::id();
    std::env::temp_dir().join(format!(
        "clickforge-ipc-{process_id}-{timestamp_ms}-{random_suffix:08x}.sock"
    ))
}

#[cfg(test)]
#[path = "rust_script_runner_tests.rs"]
mod tests;
