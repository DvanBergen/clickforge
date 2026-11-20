use std::io::{BufRead, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::input_types::{KeyCode, MouseButton};
use crate::{ScriptError, ScriptResult};

/// Environment variable storing the per-run Unix domain socket path for child scripts.
pub const IPC_SOCKET_ENV_VAR: &str = "CLICKFORGE_IPC_SOCKET";

/// Maximum serialized byte length for a single newline-delimited IPC frame (64 KiB).
pub const MAX_IPC_LINE_BYTES: usize = 65_536;

/// Request sent from an isolated Rust script child process to the ClickForge host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    MouseClick {
        mouse_button: MouseButton,
        hold_duration_ms: u64,
    },
    MouseDown {
        mouse_button: MouseButton,
    },
    MouseUp {
        mouse_button: MouseButton,
    },
    MouseMoveRelative {
        delta_x_px: i32,
        delta_y_px: i32,
    },
    MouseScroll {
        vertical_steps: i32,
    },
    KeyTap {
        key_code: KeyCode,
        hold_duration_ms: u64,
    },
    KeyDown {
        key_code: KeyCode,
    },
    KeyUp {
        key_code: KeyCode,
    },
    TypeText {
        text: String,
        keystroke_delay_ms: u64,
        jitter_ms: u64,
    },
    QueryMousePressed {
        mouse_button: MouseButton,
    },
    QueryKeyPressed {
        key_code: KeyCode,
    },
    QueryShouldStop,
    LogInfo {
        message: String,
    },
}

/// Response returned from the ClickForge host to the child script process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    Ack,
    BoolValue { is_true: bool },
    Error { message: String },
}

/// Serializes and writes one newline-delimited JSON frame with strict size enforcement.
pub fn write_ipc_message<WriterType, MessageType>(
    writer: &mut WriterType,
    message: &MessageType,
) -> ScriptResult<()>
where
    WriterType: Write,
    MessageType: Serialize,
{
    let serialized_bytes = serde_json::to_vec(message)?;
    if serialized_bytes.len() > MAX_IPC_LINE_BYTES {
        return Err(ScriptError::PayloadTooLarge {
            byte_count: serialized_bytes.len(),
            max_bytes: MAX_IPC_LINE_BYTES,
        });
    }
    writer.write_all(&serialized_bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Reads and deserializes one newline-delimited JSON frame using bounded memory.
///
/// Returns `Ok(None)` when the peer closes the stream cleanly at a frame boundary.
pub fn read_ipc_message<ReaderType, MessageType>(
    reader: &mut ReaderType,
) -> ScriptResult<Option<MessageType>>
where
    ReaderType: BufRead,
    MessageType: DeserializeOwned,
{
    let max_read_bytes = (MAX_IPC_LINE_BYTES as u64).saturating_add(1);
    let mut line_buffer = Vec::new();
    let bytes_read = reader
        .take(max_read_bytes)
        .read_until(b'\n', &mut line_buffer)?;
    if bytes_read == 0 {
        return Ok(None);
    }
    if line_buffer.len() > MAX_IPC_LINE_BYTES {
        return Err(ScriptError::PayloadTooLarge {
            byte_count: line_buffer.len(),
            max_bytes: MAX_IPC_LINE_BYTES,
        });
    }
    let decoded_message = serde_json::from_slice(&line_buffer)?;
    Ok(Some(decoded_message))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn ipc_roundtrip_serializes_and_deserializes_requests() -> ScriptResult<()> {
        let mut wire_bytes = Vec::new();
        let original_request = IpcRequest::MouseClick {
            mouse_button: MouseButton::Left,
            hold_duration_ms: 12,
        };
        write_ipc_message(&mut wire_bytes, &original_request)?;

        let mut cursor = Cursor::new(wire_bytes);
        let decoded: Option<IpcRequest> = read_ipc_message(&mut cursor)?;
        assert_eq!(decoded, Some(original_request));

        let eof_frame: Option<IpcRequest> = read_ipc_message(&mut cursor)?;
        assert_eq!(eof_frame, None);
        Ok(())
    }

    #[test]
    fn ipc_rejects_oversized_payload_lines() {
        let oversized_message = IpcRequest::LogInfo {
            message: "x".repeat(MAX_IPC_LINE_BYTES + 128),
        };
        let mut wire_bytes = Vec::new();
        let write_result = write_ipc_message(&mut wire_bytes, &oversized_message);
        assert!(matches!(
            write_result,
            Err(ScriptError::PayloadTooLarge { .. })
        ));

        let mut raw_oversized = vec![b'a'; MAX_IPC_LINE_BYTES + 16];
        raw_oversized.push(b'\n');
        let mut cursor = Cursor::new(raw_oversized);
        let read_result: ScriptResult<Option<IpcRequest>> = read_ipc_message(&mut cursor);
        assert!(matches!(
            read_result,
            Err(ScriptError::PayloadTooLarge { .. })
        ));
    }
}
