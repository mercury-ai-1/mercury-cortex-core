//! Runtime coordination — startup, shutdown, and shared context.
pub mod context;
pub mod core;
pub mod lock;
pub mod signal;
pub mod status;
pub use context::{RuntimeConfig, RuntimeContext};
pub use core::Runtime;
pub use lock::RwLockExt;
pub use signal::wait_shutdown_signal;
