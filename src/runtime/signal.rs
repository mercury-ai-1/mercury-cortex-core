//! Shared graceful-shutdown signal handling.
//!
//! The runtime, the MCP server, and the daemon each block until a
//! SIGINT/SIGTERM/SIGHUP arrives. This is that logic in one place.
//!
//! UNIX signal kinds are unix-only: `tokio::signal::unix` is gated on
//! `cfg(unix)`. On Windows we wait only on `ctrl_c` (Ctrl+C / Ctrl+Break),
//! which tokio supports cross-platform.

use tokio::signal::ctrl_c;

#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

/// Resolve once a shutdown signal has been received.
///
/// On unix this completes on the first of SIGINT, SIGTERM, or SIGHUP. It
/// registers tokio's signal handlers for the current process (preventing the
/// default action), so a caller that has invoked it will not be terminated by
/// those signals and can run graceful-shutdown logic afterwards. On Windows
/// only `Ctrl+C`/`Ctrl+Break` are observed (there is no SIGTERM/SIGHUP).
pub async fn wait_shutdown_signal() {
    #[cfg(unix)]
    {
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
    #[cfg(not(unix))]
    {
        let _ = ctrl_c().await;
    }
}
