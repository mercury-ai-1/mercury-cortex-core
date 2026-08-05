//! Content hashing for the Knowledge Engine.
//!
//! Used to hash indexed files during import so the runtime index records
//! their content hash.

use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::engine::error::EngineError;

/// Compute the SHA-256 hash of arbitrary byte data.
///
/// Used primarily for testing and for hashing in-memory content.
#[must_use]
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex_encode(hasher.finalize())
}

/// Compute the SHA-256 hash of a file on disk.
///
/// Returns [`EngineError::Io`] when the file cannot be read (e.g. it does
/// not exist, is a directory, or permissions are insufficient).
pub fn hash_file(path: &Path) -> Result<String, EngineError> {
    let data = std::fs::read(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            EngineError::Internal(anyhow::anyhow!("file not found (path redacted)"))
        } else {
            EngineError::Io(e)
        }
    })?;
    Ok(hash_bytes(&data))
}

fn hex_encode(hash: sha2::digest::Output<Sha256>) -> String {
    let mut s = String::with_capacity(hash.len() * 2);
    for b in &hash {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}
