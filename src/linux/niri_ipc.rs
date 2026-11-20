use std::env;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::models::target::ActiveWindowSnapshot;

/// Environment variable set by the `niri` Wayland compositor pointing to its IPC socket.
pub const NIRI_SOCKET_ENV_VAR: &str = "NIRI_SOCKET";

/// Compositor identifier recorded in `ActiveWindowSnapshot` and `InputSubsystemStatus`.
pub const NIRI_COMPOSITOR_NAME: &str = "niri";

const NIRI_REQUEST_TIMEOUT_MS: u64 = 1_500;
const NIRI_EVENT_STREAM_READ_TIMEOUT_MS: u64 = 250;
const MAX_NIRI_IPC_LINE_BYTES: usize = 2 * 1024 * 1024;

/// Parsed events emitted over `niri`'s persistent `"EventStream"` Unix socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NiriStreamEvent {
    WindowsChanged(Vec<ActiveWindowSnapshot>),
    WindowOpenedOrChanged(ActiveWindowSnapshot),
    WindowClosed { window_id: u64 },
    WindowFocusChanged { focused_window_id: Option<u64> },
    Ignored,
}

#[derive(Debug, Deserialize)]
struct RawNiriWindow {
    id: u64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    pid: Option<i64>,
    #[serde(default)]
    is_focused: bool,
}

impl RawNiriWindow {
    fn into_snapshot(self) -> ActiveWindowSnapshot {
        let sanitized_pid = self.pid.and_then(|raw_pid| u32::try_from(raw_pid).ok());
        ActiveWindowSnapshot {
            window_id: Some(self.id),
            title: self.title.unwrap_or_default(),
            app_id: self.app_id.unwrap_or_default(),
            pid: sanitized_pid,
            is_focused: self.is_focused,
            compositor_name: NIRI_COMPOSITOR_NAME.to_string(),
        }
    }
}

/// Active reader subscribed to `niri`'s `"EventStream"` socket.
pub struct NiriEventStream {
    reader: BufReader<UnixStream>,
}

impl NiriEventStream {
    /// Connects to `$NIRI_SOCKET` and subscribes to the live `"EventStream"`.
    pub fn connect(socket_path: &str) -> Result<Self, String> {
        let mut stream = UnixStream::connect(socket_path).map_err(|io_error| {
            format!("Failed to connect to niri socket '{socket_path}': {io_error}")
        })?;
        let write_timeout = Duration::from_millis(NIRI_REQUEST_TIMEOUT_MS);
        let read_timeout = Duration::from_millis(NIRI_REQUEST_TIMEOUT_MS);
        let _ = stream.set_write_timeout(Some(write_timeout));
        let _ = stream.set_read_timeout(Some(read_timeout));

        stream
            .write_all(b"\"EventStream\"\n")
            .map_err(|io_error| format!("Failed to request niri EventStream: {io_error}"))?;
        let _ = stream.flush();

        let mut reader = BufReader::new(stream);
        let ack_line = read_bounded_ipc_line(&mut reader)
            .map_err(|io_error| format!("Failed reading niri EventStream ack: {io_error}"))?;
        validate_event_stream_ack(&ack_line)?;

        let poll_read_timeout = Duration::from_millis(NIRI_EVENT_STREAM_READ_TIMEOUT_MS);
        let _ = reader.get_ref().set_read_timeout(Some(poll_read_timeout));
        Ok(Self { reader })
    }

    /// Reads the next event line, returning `Ok(None)` when a read timeout elapses cleanly.
    pub fn read_next_event(&mut self) -> Result<Option<NiriStreamEvent>, String> {
        let line_result = read_bounded_ipc_line(&mut self.reader);
        match line_result {
            Ok(line_text) => {
                if line_text.is_empty() {
                    return Err("niri EventStream closed by compositor".to_string());
                }
                parse_event_stream_line(&line_text).map(Some)
            }
            Err(io_error)
                if matches!(
                    io_error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(io_error) => Err(format!("niri EventStream read failed: {io_error}")),
        }
    }
}

/// Returns the trimmed `$NIRI_SOCKET` path when running inside a `niri` session.
pub fn detect_niri_socket_path() -> Option<String> {
    let raw_socket_path = env::var(NIRI_SOCKET_ENV_VAR).ok()?;
    let trimmed_path = raw_socket_path.trim();
    if trimmed_path.is_empty() {
        return None;
    }
    Some(trimmed_path.to_string())
}

/// Queries `niri` for all currently open desktop windows.
pub fn query_open_windows(socket_path: &str) -> Result<Vec<ActiveWindowSnapshot>, String> {
    let response_line = send_single_niri_request(socket_path, "\"Windows\"\n")?;
    parse_windows_response(&response_line)
}

/// Queries `niri` for the currently focused desktop window, if any.
pub fn query_focused_window(socket_path: &str) -> Result<Option<ActiveWindowSnapshot>, String> {
    let response_line = send_single_niri_request(socket_path, "\"FocusedWindow\"\n")?;
    parse_focused_window_response(&response_line)
}

/// Parses a `niri` JSON response to the `"Windows"` request.
pub fn parse_windows_response(raw_json_line: &str) -> Result<Vec<ActiveWindowSnapshot>, String> {
    let ok_payload = extract_niri_ok_payload(raw_json_line)?;
    let windows_value = ok_payload
        .get("Windows")
        .ok_or_else(|| "niri response missing 'Windows' field".to_string())?;
    let raw_windows: Vec<RawNiriWindow> = serde_json::from_value(windows_value.clone())
        .map_err(|parse_error| format!("Failed to deserialize niri Windows list: {parse_error}"))?;
    Ok(raw_windows
        .into_iter()
        .map(RawNiriWindow::into_snapshot)
        .collect())
}

/// Parses a `niri` JSON response to the `"FocusedWindow"` request.
pub fn parse_focused_window_response(
    raw_json_line: &str,
) -> Result<Option<ActiveWindowSnapshot>, String> {
    let ok_payload = extract_niri_ok_payload(raw_json_line)?;
    let focused_value = ok_payload
        .get("FocusedWindow")
        .ok_or_else(|| "niri response missing 'FocusedWindow' field".to_string())?;
    if focused_value.is_null() {
        return Ok(None);
    }
    let raw_window: RawNiriWindow =
        serde_json::from_value(focused_value.clone()).map_err(|parse_error| {
            format!("Failed to deserialize niri FocusedWindow object: {parse_error}")
        })?;
    Ok(Some(raw_window.into_snapshot()))
}

/// Parses a single newline-delimited JSON event emitted by `niri`'s `"EventStream"`.
pub fn parse_event_stream_line(raw_json_line: &str) -> Result<NiriStreamEvent, String> {
    let trimmed_line = raw_json_line.trim();
    if trimmed_line.is_empty() {
        return Ok(NiriStreamEvent::Ignored);
    }
    let root_value: Value = serde_json::from_str(trimmed_line)
        .map_err(|parse_error| format!("Invalid niri event JSON: {parse_error}"))?;
    let Some(root_object) = root_value.as_object() else {
        return Ok(NiriStreamEvent::Ignored);
    };

    if let Some(windows_changed) = root_object.get("WindowsChanged") {
        return parse_windows_changed_event(windows_changed);
    }
    if let Some(opened_or_changed) = root_object.get("WindowOpenedOrChanged") {
        return parse_window_opened_or_changed_event(opened_or_changed);
    }
    if let Some(window_closed) = root_object.get("WindowClosed") {
        return parse_window_closed_event(window_closed);
    }
    if let Some(focus_changed) = root_object.get("WindowFocusChanged") {
        return parse_window_focus_changed_event(focus_changed);
    }

    Ok(NiriStreamEvent::Ignored)
}

fn send_single_niri_request(socket_path: &str, request_json: &str) -> Result<String, String> {
    let mut stream = UnixStream::connect(socket_path).map_err(|io_error| {
        format!("Failed to connect to niri socket '{socket_path}': {io_error}")
    })?;
    let timeout = Duration::from_millis(NIRI_REQUEST_TIMEOUT_MS);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    stream
        .write_all(request_json.as_bytes())
        .map_err(|io_error| format!("Failed to send niri request: {io_error}"))?;
    let _ = stream.flush();

    let mut reader = BufReader::new(stream);
    read_bounded_ipc_line(&mut reader)
        .map_err(|io_error| format!("Failed to read niri response: {io_error}"))
}

fn read_bounded_ipc_line(reader: &mut BufReader<UnixStream>) -> io::Result<String> {
    let mut line_bytes = Vec::new();
    let max_bytes_u64 = u64::try_from(MAX_NIRI_IPC_LINE_BYTES).unwrap_or(u64::MAX);
    let bytes_read = reader
        .by_ref()
        .take(max_bytes_u64)
        .read_until(b'\n', &mut line_bytes)?;
    if bytes_read == 0 {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&line_bytes).trim().to_string())
}

fn validate_event_stream_ack(ack_line: &str) -> Result<(), String> {
    let ok_payload = extract_niri_ok_payload(ack_line)?;
    if ok_payload.as_str() == Some("Handled") {
        return Ok(());
    }
    Err(format!(
        "Unexpected niri EventStream ack payload: {ack_line}"
    ))
}

fn extract_niri_ok_payload(raw_json_line: &str) -> Result<Value, String> {
    let root_value: Value = serde_json::from_str(raw_json_line.trim())
        .map_err(|parse_error| format!("Invalid niri reply JSON: {parse_error}"))?;
    if let Some(error_value) = root_value.get("Err") {
        return Err(format!("niri returned IPC error: {error_value}"));
    }
    root_value
        .get("Ok")
        .cloned()
        .ok_or_else(|| "niri reply missing 'Ok' envelope".to_string())
}

fn parse_windows_changed_event(event_payload: &Value) -> Result<NiriStreamEvent, String> {
    let windows_value = event_payload
        .get("windows")
        .ok_or_else(|| "WindowsChanged event missing 'windows' array".to_string())?;
    let raw_windows: Vec<RawNiriWindow> = serde_json::from_value(windows_value.clone())
        .map_err(|parse_error| format!("Invalid WindowsChanged payload: {parse_error}"))?;
    let snapshots = raw_windows
        .into_iter()
        .map(RawNiriWindow::into_snapshot)
        .collect();
    Ok(NiriStreamEvent::WindowsChanged(snapshots))
}

fn parse_window_opened_or_changed_event(event_payload: &Value) -> Result<NiriStreamEvent, String> {
    let window_value = event_payload
        .get("window")
        .ok_or_else(|| "WindowOpenedOrChanged event missing 'window' object".to_string())?;
    let raw_window: RawNiriWindow = serde_json::from_value(window_value.clone())
        .map_err(|parse_error| format!("Invalid WindowOpenedOrChanged payload: {parse_error}"))?;
    Ok(NiriStreamEvent::WindowOpenedOrChanged(
        raw_window.into_snapshot(),
    ))
}

fn parse_window_closed_event(event_payload: &Value) -> Result<NiriStreamEvent, String> {
    let window_id = event_payload
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "WindowClosed event missing numeric 'id'".to_string())?;
    Ok(NiriStreamEvent::WindowClosed { window_id })
}

fn parse_window_focus_changed_event(event_payload: &Value) -> Result<NiriStreamEvent, String> {
    let Some(id_value) = event_payload.get("id") else {
        return Err("WindowFocusChanged event missing 'id' field".to_string());
    };
    if id_value.is_null() {
        return Ok(NiriStreamEvent::WindowFocusChanged {
            focused_window_id: None,
        });
    }
    let focused_window_id = id_value
        .as_u64()
        .ok_or_else(|| "WindowFocusChanged 'id' field is neither u64 nor null".to_string())?;
    Ok(NiriStreamEvent::WindowFocusChanged {
        focused_window_id: Some(focused_window_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_niri_windows_and_focused_window_replies() {
        let focused_reply = r#"{"Ok":{"FocusedWindow":{"id":92,"title":"Here Comes the Horde","app_id":"steam_app_2560650","pid":7205,"is_focused":true}}}"#;
        let focused = parse_focused_window_response(focused_reply);
        assert!(focused.is_ok());
        if let Ok(Some(snapshot)) = focused {
            assert_eq!(snapshot.window_id, Some(92));
            assert_eq!(snapshot.title, "Here Comes the Horde");
            assert_eq!(snapshot.app_id, "steam_app_2560650");
            assert_eq!(snapshot.pid, Some(7205));
            assert!(snapshot.is_focused);
            assert_eq!(snapshot.compositor_name, "niri");
        } else {
            panic!("Expected Some(ActiveWindowSnapshot)");
        }

        let null_focused = r#"{"Ok":{"FocusedWindow":null}}"#;
        assert_eq!(parse_focused_window_response(null_focused), Ok(None));

        let windows_reply = r#"{"Ok":{"Windows":[{"id":76,"title":null,"app_id":null,"pid":null,"is_focused":false},{"id":92,"title":"Editor","app_id":"antigravity","pid":426702,"is_focused":true}]}}"#;
        let parsed_windows = parse_windows_response(windows_reply);
        assert!(parsed_windows.is_ok());
        if let Ok(window_list) = parsed_windows {
            assert_eq!(window_list.len(), 2);
            assert_eq!(window_list[0].title, "");
            assert_eq!(window_list[0].pid, None);
            assert_eq!(window_list[1].window_id, Some(92));
        }
    }

    #[test]
    fn parses_niri_event_stream_variants_and_ignores_unrelated_events() {
        let windows_changed = r#"{"WindowsChanged":{"windows":[{"id":10,"title":"Game","app_id":"steam_app_123","pid":500,"is_focused":true}]}}"#;
        assert!(matches!(
            parse_event_stream_line(windows_changed),
            Ok(NiriStreamEvent::WindowsChanged(windows)) if windows.len() == 1
        ));

        let opened_changed = r#"{"WindowOpenedOrChanged":{"window":{"id":11,"title":"Chat","app_id":"discord","pid":600,"is_focused":false}}}"#;
        assert!(matches!(
            parse_event_stream_line(opened_changed),
            Ok(NiriStreamEvent::WindowOpenedOrChanged(snapshot)) if snapshot.window_id == Some(11)
        ));

        let focus_event = r#"{"WindowFocusChanged":{"id":11}}"#;
        assert_eq!(
            parse_event_stream_line(focus_event),
            Ok(NiriStreamEvent::WindowFocusChanged {
                focused_window_id: Some(11),
            })
        );

        let blur_event = r#"{"WindowFocusChanged":{"id":null}}"#;
        assert_eq!(
            parse_event_stream_line(blur_event),
            Ok(NiriStreamEvent::WindowFocusChanged {
                focused_window_id: None,
            })
        );

        let closed_event = r#"{"WindowClosed":{"id":11}}"#;
        assert_eq!(
            parse_event_stream_line(closed_event),
            Ok(NiriStreamEvent::WindowClosed { window_id: 11 })
        );

        let unrelated_event = r#"{"OverviewOpenedOrClosed":{"is_open":false}}"#;
        assert_eq!(
            parse_event_stream_line(unrelated_event),
            Ok(NiriStreamEvent::Ignored)
        );
    }

    #[test]
    fn rejects_niri_error_envelope_and_malformed_json() {
        let error_reply = r#"{"Err":"unsupported request"}"#;
        assert!(parse_windows_response(error_reply).is_err());
        assert!(parse_event_stream_line("{not-valid-json").is_err());
    }
}
