//! Poison-safe RwLock extension trait.
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

use crate::service::ServiceError;

/// Extension trait for working with `RwLock` state that may be poisoned.
pub trait RwLockExt<T> {
    /// Acquire a read guard, recovering from poison.
    fn read_unpoison(&self) -> RwLockReadGuard<'_, T>;
    /// Acquire a write guard, recovering from poison.
    fn write_unpoison(&self) -> RwLockWriteGuard<'_, T>;
    /// Acquire a read guard, surfacing poison as a [`ServiceError`] instead
    /// of panicking.
    ///
    /// Mirrors [`read_unpoison`](RwLockExt::read_unpoison)'s recovery intent
    /// but returns a handled [`ServiceError`] on a genuinely poisoned lock so
    /// callers can degrade instead of panicking.
    fn read_result(&self) -> Result<RwLockReadGuard<'_, T>, ServiceError>;
}

impl<T> RwLockExt<T> for RwLock<T> {
    fn read_unpoison(&self) -> RwLockReadGuard<'_, T> {
        self.read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_unpoison(&self) -> RwLockWriteGuard<'_, T> {
        self.write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn read_result(&self) -> Result<RwLockReadGuard<'_, T>, ServiceError> {
        self.read()
            .map_err(|_| ServiceError::Internal("runtime state lock poisoned".into()))
    }
}
