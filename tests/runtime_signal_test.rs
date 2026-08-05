use std::process::Command;
use std::time::Duration;

use mercury_cortex_core::runtime::wait_shutdown_signal;

fn send_signal_to_self(sig: &str) {
    let status = Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(std::process::id().to_string())
        .status()
        .expect("kill binary must exist");
    assert!(status.success(), "kill -{sig} must succeed");
}

async fn assert_resolves_on(sig: &str) {
    let handle = tokio::spawn(wait_shutdown_signal());
    // Give tokio time to register the signal handler before sending.
    tokio::time::sleep(Duration::from_millis(300)).await;
    send_signal_to_self(sig);
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("wait_shutdown_signal must resolve")
        .expect("task must not panic");
}

#[tokio::test]
async fn wait_shutdown_signal_resolves_on_sigint_sigterm_sighup_and_not_without_signal() {
    // Negative control first: no signal within a bounded wait -> must NOT resolve.
    tokio::select! {
        _ = wait_shutdown_signal() => panic!("resolved without any signal"),
        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
    }

    assert_resolves_on("INT").await; // ctrl_c path
    assert_resolves_on("TERM").await;
    assert_resolves_on("HUP").await;
}
