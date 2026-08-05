# Security Policy

## Reporting Vulnerabilities

Please report security vulnerabilities through [GitHub Issues](https://github.com/mercury-ai-1/mercury-cortex-core/issues) or email (if provided).

Do NOT disclose publicly until a fix is available.

## Scope

This security policy covers `mercury-cortex-core` (the library crate).

For `mercury-cortex` CLI security, see the CLI repository.

## Threat Model

### Database Connection & Encryption

- `db::connect()` uses `kv-surrealkv` with exclusive file locking
- Encryption keys are percent-encoded for URL-safe transport
- `connect_with_retry()` implements exponential backoff
- Circuit breaker prevents repeated connection attempts to corrupt DBs

### Importer Path Traversal

- `importer.rs` validates all paths with `join_within_root()` to prevent directory traversal
- Metadata JSON files referencing paths outside the project root are rejected
- `is_safe_relative_path()` rejects `..` components

### `.mcignore` Enforcement

- `McIgnore` patterns are enforced by the `Importer` during `submit_metadata()`
- Excluded paths are never indexed — even if manually requested
- Pattern syntax follows gitignore (including negation)

### File Hash Integrity

- `hash_file()` and `hash_bytes()` use SHA-256 via the `sha2` crate
- Hash failures fail the import (not silently ignored)

### Schema Migrations

- `run_pending()` is idempotent — safe to run multiple times
- `verify_schema()` checks all expected tables and fields exist
- Migrations use SurrealDB transactions for atomicity

### Backup/Restore

- `backup()` copies the entire DB directory atomically
- `restore()` uses atomic rename for crash safety
- `db::reset::reset()` supports targeted table clearing

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |
| < 0.1   | No        |

Only the latest version receives security updates.
