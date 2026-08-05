# Contributing to mercury-cortex-core

Thank you for your interest in contributing!

## Getting Started

### Prerequisites

- Rust 1.85+ (edition 2024)

### Build

```bash
cargo build
```

### Test

```bash
cargo test
cargo clippy -- -D warnings
```

## Public API Contract

This crate is consumed by `mercury-cortex` and potentially other frontends. All `pub` items are part of the public API.

**Rules:**

- All modules and types in `lib.rs` are `pub` — they are the stable API surface
- New public types/methods must have doc comments (`///`)
- Breaking changes require a version bump and consumer update
- Internal implementation details should be `pub(crate)`, never `pub`

## Project Structure

```
src/
├── client/             — CoreClient facade (single entry point)
│   ├── mod.rs          — CoreClient, Paths, CoreError
│   ├── database.rs     — DatabaseClient: backup/restore/reset/migrate/export
│   ├── graph.rs        — GraphClient: edge queries
│   ├── profile.rs      — ProfileClient: get/upsert/email_exists
│   └── project.rs      — ProjectClient: register/scaffold/config
├── db/                 — connect, initialize, backup, export, reset, pool, retry
├── engine/
│   ├── error.rs        — EngineError enum
│   ├── index/          — IndexEngine, Importer, FileDataRepository, RuntimeIndex,
│   │                     search, hash, McIgnore, cache
│   └── knowledge/      — KnowledgeEngine, EngineState, EventLog
├── runtime/            — Runtime, RuntimeContext, status, lock, signal
├── schema/             — Migration runner, verification, registry (v001–v005)
├── service/
│   ├── profile.rs      — ProfileService
│   ├── project.rs      — ProjectService
│   ├── file_data.rs    — FileDataService
│   ├── graph.rs        — GraphService
│   └── scaffold.rs     — File scaffolding (AGENTS.md, .mcignore, config.json)
└── util.rs             — record_id_to_string, project_id_value, canonicalize_root_path
```

## Determinism Requirements

Several core operations must be deterministic:

- **`db::export::export_tables()`** — Sorts tables alphabetically, serializes via BTreeMap (deterministic JSON key order)
- **`schema::migrations::run_pending()`** — Runs migrations in order by name

When modifying these functions, ensure determinism is preserved.

## Development Workflow

1. Create a branch
2. Make changes
3. Run `cargo test`, `cargo clippy`
4. Test with `mercury-cortex`: `cargo test --manifest-path ../mercury-cortex/Cargo.toml`
5. Submit PR

## Testing

- **Unit tests:** Inline in `src/` files
- **Integration tests:** `tests/` directory (13 test files covering DB, client, engine, runtime, and service behaviors)
- **Test helpers:** `setup_db()`, `create_test_engine()`, `create_test_context()` — use temporary directories for all DB instances (never touch real data)

## Commit Messages

Use conventional commits: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`

## License

Apache-2.0 — By contributing, you agree that your contributions will be licensed under the same license.
