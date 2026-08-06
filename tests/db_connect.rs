//! Integration tests for the `db::connect` path helpers. Kept out of `src/`
//! per the repo policy of managing all tests under `tests/`.

use std::fs::OpenOptions;

use fs2::FileExt;
use tempfile::TempDir;

use mercury_cortex_core::db::connect::percent_encode;
use mercury_cortex_core::db::lock_is_held;

#[test]
fn percent_encode_escapes_reserved_chars_and_keeps_unreserved() {
    assert_eq!(percent_encode("a&b=c?d#e f%g"), "a%26b%3Dc%3Fd%23e%20f%25g");
    assert_eq!(percent_encode("plain-alnum._~"), "plain-alnum._~");
    assert_eq!(percent_encode("é"), "%C3%A9");
    assert_eq!(percent_encode("\n"), "%0A");
    assert_eq!(percent_encode("\t"), "%09");
}

#[test]
fn lock_is_held_false_when_no_lock_file() {
    let tmp = TempDir::new().unwrap();
    assert!(!lock_is_held(tmp.path()).unwrap());
}

#[test]
fn lock_is_held_false_when_lock_file_present_but_unlocked() {
    let tmp = TempDir::new().unwrap();
    let lock = tmp.path().join("LOCK");
    std::fs::write(&lock, b"stale-pid\n").unwrap();
    assert!(!lock_is_held(tmp.path()).unwrap());
}

#[test]
fn lock_is_held_true_when_flock_held() {
    let tmp = TempDir::new().unwrap();
    let lock = tmp.path().join("LOCK");
    std::fs::write(&lock, b"some-pid\n").unwrap();
    let holder = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
    holder.lock_exclusive().unwrap();
    assert!(lock_is_held(tmp.path()).unwrap());
}

#[test]
fn lock_is_held_false_after_holder_releases() {
    let tmp = TempDir::new().unwrap();
    let lock = tmp.path().join("LOCK");
    std::fs::write(&lock, b"some-pid\n").unwrap();
    let holder = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
    holder.lock_exclusive().unwrap();
    drop(holder);
    assert!(!lock_is_held(tmp.path()).unwrap());
}

/// `MERCURY_CORTEX_DATA_DIR` overrides the resolved data directory verbatim.
///
/// All assertions live in one test because the override is a process-global
/// env var; splitting them across `#[test]` fns would race under the parallel
/// test runner.
#[test]
fn data_dir_env_override_wins_over_home() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("custom").join("nested");

    unsafe { std::env::set_var("MERCURY_CORTEX_DATA_DIR", &target) };
    let resolved = mercury_cortex_core::db::data_dir().unwrap();
    assert_eq!(resolved, target);
    unsafe { std::env::remove_var("MERCURY_CORTEX_DATA_DIR") };

    // Empty override is rejected rather than silently resolving to "".
    unsafe { std::env::set_var("MERCURY_CORTEX_DATA_DIR", "") };
    assert!(mercury_cortex_core::db::data_dir().is_err());
    unsafe { std::env::remove_var("MERCURY_CORTEX_DATA_DIR") };

    // Without the override, home_dir is used (either $HOME or the platform
    // profile dir) — just assert it resolves and is absolute.
    let home_based = mercury_cortex_core::db::data_dir().unwrap();
    assert!(home_based.is_absolute());
    assert_eq!(home_based.file_name().unwrap(), "cortex");
}
