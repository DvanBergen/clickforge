pub mod block_interpreter;
pub mod coordinator;
mod coordinator_loop;
mod coordinator_telemetry;
pub mod rust_codegen;
mod rust_compiler;
pub mod rust_script_runner;
pub mod timing;
mod worker_supervisor;

pub use coordinator::{CoordinatorHandle, CoordinatorStartConfig, start_coordinator};
pub use rust_codegen::{generate_rust_source_from_script, generate_rust_source_with_custom_blocks};
