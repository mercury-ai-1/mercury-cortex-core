use std::sync::{Arc, RwLock};

use mercury_cortex_core::runtime::RwLockExt;
use mercury_cortex_core::service::ServiceError;

#[test]
fn read_result_returns_ok_on_healthy_lock() {
    let lock = RwLock::new(42_u32);
    let guard = lock.read_result().unwrap();
    assert_eq!(*guard, 42);
}

#[test]
fn read_result_returns_err_on_poisoned_lock_without_panicking() {
    let lock = Arc::new(RwLock::new(7_u32));
    let clone = lock.clone();
    let handle = std::thread::spawn(move || {
        let _guard = clone.write().unwrap();
        panic!("poison the lock");
    });
    let _ = handle.join();

    let result = lock.read_result();
    assert!(
        matches!(result, Err(ServiceError::Internal(_))),
        "poisoned lock must surface as a handled error, not a panic: {result:?}"
    );
}
