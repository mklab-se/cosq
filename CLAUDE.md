# cosq

A CLI to query your Azure Cosmos DB instances.

## Commands

```bash
cargo build              # Build all crates
cargo test --workspace   # Run all tests
cargo clippy --workspace -- -D warnings  # Lint (CI-enforced)
cargo fmt --all -- --check               # Format check (CI-enforced)
cargo run -- --help      # Run the CLI
```

## Architecture

Rust workspace with three crates:

```
crates/
  cosq/             # CLI binary (package and binary name: cosq)
    src/
      main.rs       # Entry point, logging setup, dynamic completions, background update check
      cli.rs        # Clap CLI definitions, command dispatch, dynamic completion candidates
      banner.rs     # ASCII art logo
      update.rs     # Version update checker (queries crates.io, caches 24h)
      output.rs     # Output formatting (JSON, JSON-compact, table, CSV, MiniJinja templates)
      commands/
        mod.rs      # Command module exports
        auth.rs     # `cosq auth` (status/login/logout)
        completion.rs # `cosq completion` (static + dynamic completion tip)
        init.rs     # `cosq init` (interactive Cosmos DB account setup)
        ai.rs       # `cosq ai init` (interactive AI provider setup)
        query.rs    # `cosq query` (SQL query execution with output formatting)
        run.rs      # `cosq run` (execute stored queries with parameters)
        queries.rs  # `cosq queries` (list/create/edit/delete/show/generate stored queries)
  cosq-core/        # Core types and configuration
    src/
      lib.rs        # Module exports
      config.rs     # Config format (load/save from ~/.config/cosq/), AiProvider enum, AiConfig
      stored_query.rs # Stored query format (.cosq files), parameter resolution, query discovery
  cosq-client/      # Azure Cosmos DB client and authentication
    src/
      lib.rs        # Module exports
      auth.rs       # Azure CLI auth (token acquisition, login status)
      arm.rs        # ARM discovery (subscriptions, Cosmos DB accounts, RBAC role management)
      cosmos.rs     # Cosmos DB data plane client (query, parameterized query, list databases/containers)
      ai.rs         # Unified AI dispatcher (routes to Azure OpenAI, local agents, or Ollama)
      openai.rs     # Azure OpenAI client (chat completions with AAD token auth)
      local_agent.rs # Local CLI agent integration (claude, codex, copilot subprocess invocation)
      ollama.rs     # Ollama HTTP API client (local LLM chat completions, model listing)
      error.rs      # ClientError types with helpful hints
```

- **Workspace root** `Cargo.toml` defines shared dependencies and metadata
- All crates inherit `version`, `edition`, `authors`, `license`, `repository`, `rust-version` from workspace
- Single version bump in root `Cargo.toml` updates everything

## Key Patterns

- CLI built with `clap` derive macros + `clap_complete` for shell completions (static and dynamic)
- Dynamic completions: `CompleteEnv` + `ArgValueCandidates` for runtime stored query name tab-completion
- Async runtime: `tokio`
- Logging: `tracing` + `tracing-subscriber` with `-v`/`-vv` verbosity levels
- Colored output via `colored` crate (respects `--no-color`)
- Interactive prompts via `dialoguer` with fuzzy-select
- Error handling: `anyhow` (CLI), `thiserror` (libraries)
- Azure auth: delegates to `az` CLI for token acquisition
- Cosmos DB data plane: REST API with AAD token auth, parameterized queries, pagination via `x-ms-continuation`
- Stored queries: `.cosq` files with YAML front matter + SQL body, stored in `~/.cosq/queries/` (user) and `.cosq/queries/` (project, overrides user)
- Output formatting: JSON (default), JSON-compact, table (comfy-table), CSV, MiniJinja templates
- AI query generation: multi-provider support via unified dispatcher — Azure OpenAI API, local CLI agents (claude, codex, copilot), and Ollama local LLMs. Configured via `cosq ai init`
- Config: `~/.config/cosq/config.yaml` (via `dirs::config_dir()`), includes optional `database`/`container`/`ai` sections
- Update checker: background task, cached at `~/.cache/cosq/`, skip with `COSQ_NO_UPDATE_CHECK=1`

## Releasing

1. Bump `version` in root `Cargo.toml`
2. Commit and push to main
3. Tag: `git tag v0.X.Y && git push origin v0.X.Y`
4. Release workflow builds binaries (Linux, macOS Intel+ARM, Windows), creates GitHub Release, updates Homebrew tap (`mklab-se/homebrew-tap`), publishes to crates.io

**Required GitHub secrets:**
- `CARGO_REGISTRY_TOKEN` (in `crates-io` environment)
- `HOMEBREW_TAP_TOKEN` (GitHub PAT with repo scope for `mklab-se/homebrew-tap`)

## Code Style

- Edition 2024, MSRV 1.85
- `cargo clippy` with `-D warnings` (zero warnings policy)
- `cargo fmt` enforced in CI

## Quality Requirements

### Testing
- **Always run the full test suite before declaring work complete:** `cargo test --workspace`
- **Always run the full CI check before pushing:** `cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings && cargo test --workspace`
- Write unit tests for all new functionality — aim for high code coverage
- Test edge cases and error paths, not just the happy path
- For code that interacts with external services (Azure, crates.io), test the parsing/logic locally with mock data
- Run the CLI binary to verify commands work end-to-end (e.g. `cargo run -- init`, `cargo run -- auth status`)

### Documentation
- **Before pushing or releasing, review all documentation for accuracy:**
  - `README.md` — features, quick start, badges
  - `INSTALL.md` — installation methods, shell completions
  - `CHANGELOG.md` — new entries for every user-visible change
  - `CLAUDE.md` — architecture, commands, patterns
- When adding new commands, flags, or crates, update all relevant docs in the same commit
- `CHANGELOG.md` must be updated for every release with a dated entry following Keep a Changelog format
