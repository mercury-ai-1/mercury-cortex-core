//! Shared graceful-shutdown signal handling.
//!
//! The runtime, the MCP server, and the daemon each block until a
//! SIGINT/SIGTERM/SIGHUP arrives. This is that logic in one place.

use tokio::signal::ctrl_c;
use tokio::signal::unix::{SignalKind, signal};

/// Resolve once SIGINT, SIGTERM, or SIGHUP has been received.
///
/// This future completes on the first of the three signals. It registers
/// tokio's signal handlers for the current process (preventing the default
/// action), so a caller that has invoked it will not be terminated by those
/// signals and can run graceful-shutdown logic afterwards.
pub async fn wait_shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).ok();
    let mut sighup = signal(SignalKind::hangup()).ok();
    tokio::select! {
        _ = ctrl_c() => {}
        () = async {
            if let Some(ref mut sig) = sigterm {
                sig.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {}
        () = async {
            if let Some(ref mut sig) = sighup {
                sig.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {}
    }
}
