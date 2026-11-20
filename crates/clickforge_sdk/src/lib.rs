pub mod context;
pub mod input_types;
pub mod ipc_protocol;

pub use context::ScriptContext;
pub use input_types::{GaussianRangeMs, InputTrigger, KeyCode, MouseButton};
pub use ipc_protocol::{
    IPC_SOCKET_ENV_VAR, IpcRequest, IpcResponse, MAX_IPC_LINE_BYTES, read_ipc_message,
    write_ipc_message,
};

use thiserror::Error;

/// Structured errors returned by ClickForge SDK operations and IPC calls.
#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("Environment variable '{variable_name}' is not set")]
    MissingSocketEnvVar { variable_name: &'static str },

    #[error("Invalid Gaussian range: {reason}")]
    InvalidGaussianRange { reason: String },

    #[error("Invalid timing range: {reason}")]
    InvalidTimingRange { reason: String },

    #[error("Invalid script argument: {reason}")]
    InvalidArgument { reason: String },

    #[error("Failed to connect to ClickForge IPC socket at '{socket_path}': {source}")]
    IpcConnectionFailed {
        socket_path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("ClickForge IPC I/O error: {source}")]
    IpcIoFailed {
        #[from]
        source: std::io::Error,
    },

    #[error("ClickForge IPC JSON serialization error: {source}")]
    IpcSerializationFailed {
        #[from]
        source: serde_json::Error,
    },

    #[error("ClickForge host closed the IPC connection")]
    IpcConnectionClosed,

    #[error("ClickForge host rejected request: {message}")]
    HostRejectedRequest { message: String },

    #[error("Unexpected ClickForge IPC response (expected {expected}, received {actual})")]
    UnexpectedResponse {
        expected: &'static str,
        actual: String,
    },

    #[error("IPC payload of {byte_count} bytes exceeds limit of {max_bytes} bytes")]
    PayloadTooLarge { byte_count: usize, max_bytes: usize },
}

/// Convenience result alias for ClickForge scripts and SDK operations.
pub type ScriptResult<ValueType> = Result<ValueType, ScriptError>;

/// Connects a child script process using `CLICKFORGE_IPC_SOCKET` and executes its entrypoint.
pub fn execute_script_entrypoint<Entrypoint>(entrypoint: Entrypoint) -> ScriptResult<()>
where
    Entrypoint: FnOnce(&mut ScriptContext) -> ScriptResult<()>,
{
    let mut script_context = ScriptContext::connect_from_env()?;
    entrypoint(&mut script_context)
}
