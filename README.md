<div align="center">

# mercury-cortex-core

**The foundational library for the Mercury Cortex knowledge engine.**

[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange)](https://www.rust-lang.org/)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

</div>

---

## Overview

`mercury-cortex-core` is the core library crate for the Mercury Cortex knowledge engine. It owns all business logic, data access, and engine infrastructure. It has no knowledge of HTTP, MCP, CLI, or IPC transport — purely business logic.

**Consumers:**

- `mercury-cortex` (CLI/MCP application via path dependency)
- Future API frontends and extensions

## Architecture

```
mercury-cortex-core/src/
├── lib.rs              — Re-exports all modules, SurrealDb type alias
├── util.rs             — Shared utilities (record ID parsing, path canonicalization)
│
├── client/             — CoreClient facade (single public entry point)
│   ├── mod.rs          — CoreClient, Paths, CoreError
│   ├── database.rs     — DatabaseClient: backup/restore/reset/migrate/export
│   ├── graph.rs        — GraphClient: edge queries
│   ├── profile.rs      — ProfileClient: get/upsert/email_exists
│   └── project.rs      — ProjectClient: register/scaffold/config
│
├── db/                 — Database layer
│   ├── connect.rs      — data_dir(), connect(), initialize(), retry logic
│   ├── pool.rs         — CircuitBreaker, DbPool
│   ├── retry.rs        — retry(), retry_with_breaker(), reset_shared_breaker()
│   ├── backup.rs       — backup(), list_backups(), restore()
│   ├── export.rs       — export_tables(), list_tables(), ExportFilter
│   └── reset.rs        — reset(), list_resettable_tables(), ResetMode
│
├── engine/             — Knowledge engine
│   ├── error.rs        — EngineError enum
│   ├── index/          — File indexing subsystem
│   │   ├── engine.rs       — IndexEngine (importer + runtime index + search)
│   │   ├── importer.rs     — Importer (reads .mercury-cortex/temp/, upserts file_data)
│   │   ├── file_data_repo.rs — FileDataRepository trait + SurrealFileDataRepository
│   │   ├── runtime_index.rs — RuntimeIndex + FileEntry (in-memory metadata cache)
│   │   ├── search.rs       — SearchQuery, SearchResult, scoring
│   │   ├── hash.rs         — hash_bytes(), hash_file() (SHA-256)
│   │   ├── mcignore.rs     — McIgnore (compiled .mcignore pattern set)
│   │   └── cache.rs        — FileMetadataCache (LRU)
│   └── knowledge/      — Engine lifecycle & state
│       ├── engine.rs       — KnowledgeEngine (start/stop/search/submit_metadata)
│       ├── context.rs      — EngineState (started_at, project_id, project_root)
│       └── event_log.rs    — EventLog (bounded FIFO audit trail)
│
├── runtime/            — Runtime coordination
│   ├── core.rs         — Runtime (bootstraps DB + engine + signal handler)
│   ├── context.rs      — RuntimeContext (db, engine, status, shutdown)
│   ├── status.rs       — RuntimePhase, HealthStatus, ErrorCode, RuntimeStatus
│   ├── lock.rs         — RwLockExt trait (read_unpoison, write_unpoison)
│   └── signal.rs       — wait_shutdown_signal()
│
├── schema/             — Database schema & migrations
│   └── migration/
│       ├── run.rs          — run_pending(), verify_schema(), MigrationReport
│       ├── registry.rs     — 5 migrations (v001–v005), expected_tables()
│       └── registry/       — Individual migration files
│
└── service/            — Business logic layer
    ├── profile.rs      — ProfileService
    ├── project.rs      — ProjectService
    ├── file_data.rs    — FileDataService
    ├── graph.rs        — GraphService
    └── scaffold.rs     — File scaffolding (AGENTS.md, .mcignore, config.json)
```

## Public API

| Type | Role |
|------|------|
| `CoreClient` | Single public entry point; wraps all engine/service access via sub-clients |
| `KnowledgeEngine` | Start/stop engine, search files, import metadata, project status |
| `IndexEngine` | Import metadata JSON, manage runtime index, search |
| `Runtime` | Owns db + engine, manages lifecycle phases, graceful shutdown |
| `ProjectService` | Project registration and management |
| `ProfileService` | User profile data operations |
| `GraphService` | Knowledge graph relations |
| `FileDataService` | File metadata listing and deletion |

## Modules

- **`client`** — `CoreClient` facade, sub-clients (`ProfileClient`, `ProjectClient`, `DatabaseClient`, `GraphClient`), error types, path utilities
- **`db`** — Database connection (`connect`, `initialize`), backup/restore, export, reset, connection pooling with circuit breaker
- **`engine`** — Core engine: `KnowledgeEngine` (lifecycle, search, import), `IndexEngine` (importer, runtime index, search, hash, mcignore), `EventLog`
- **`runtime`** — Runtime lifecycle (db + engine), context, status reporting, signal handling
- **`schema`** — Schema migration runner, verification, migration registry (v001–v005)
- **`service`** — Business services: project, profile, file_data, graph, and file scaffolding
- **`util`** — Shared utilities (`record_id_to_string`, `project_id_value`, `canonicalize_root_path`)

## Usage

```rust
use mercury_cortex_core::client::CoreClient;

// Open with default data directory (~/.mercury/cortex/)
let client = CoreClient::open()?;

// Or specify a custom data directory
let client = CoreClient::open_with_data_dir("/path/to/data".into())?;

// Access sub-clients for domain operations
let profile = client.profile().get().await?;
let project = client.project().register(params).await?;
let tables = client.database().list_tables().await?;
```

For detailed API documentation, build with:

```bash
cargo doc --open
```

## Testing

```bash
# Run all tests
cargo test

# Run with clippy
cargo clippy -- -D warnings
```

Tests use temporary directories and isolated database instances. See `tests/` for integration tests (13 test files covering DB, client, engine, runtime, and service behaviors) and inline modules for unit tests.

## Cross-repo Development

This library is consumed by `mercury-cortex`. Changes here affect the CLI.

```bash
# After making changes here, test with the CLI:
cargo test --manifest-path ../mercury-cortex/Cargo.toml
```

## Security

See [SECURITY.md](SECURITY.md) for details on:

- Database connection and encryption
- Importer path traversal prevention
- `.mcignore` enforcement during import
- File hash integrity
- Schema migration safety
- Backup/restore atomicity

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, public API conventions, and contribution guidelines.

## License

Apache-2.0 — Copyright 2026 Mercury Cortex Contributors

See [LICENSE](LICENSE) for the full license text.
