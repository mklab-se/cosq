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
      main.rs       # Entry point, logging setup, background update check
      cli.rs        # Clap CLI definitions and command dispatch
      banner.rs     # ASCII art logo
      update.rs     # Version update checker (queries crates.io, caches 24h)
      commands/
        mod.rs      # Command module exports
        auth.rs     # `cosq auth` (status/login/logout)
        completion.rs # `cosq completion` (bash/zsh/fish/powershell)
        init.rs     # `cosq init` (interactive Cosmos DB account setup)
  cosq-core/        # Core types and configuration
    src/
      lib.rs        # Module exports
      config.rs     # cosq.yaml config format (load/save/find)
  cosq-client/      # Azure Cosmos DB client and authentication
    src/
      lib.rs        # Module exports
      auth.rs       # Azure CLI auth (token acquisition, login status)
      arm.rs        # ARM discovery (subscriptions, Cosmos DB accounts)
      error.rs      # ClientError types with helpful hints
```

- **Workspace root** `Cargo.toml` defines shared dependencies and metadata
- All crates inherit `version`, `edition`, `authors`, `license`, `repository`, `rust-version` from workspace
- Single version bump in root `Cargo.toml` updates everything

## Key Patterns

- CLI built with `clap` derive macros + `clap_complete` for shell completions
- Async runtime: `tokio`
- Logging: `tracing` + `tracing-subscriber` with `-v`/`-vv` verbosity levels
- Colored output via `colored` crate (respects `--no-color`)
- Interactive prompts via `dialoguer` with fuzzy-select
- Error handling: `anyhow` (CLI), `thiserror` (libraries)
- Azure auth: delegates to `az` CLI for token acquisition
- Config: `cosq.yaml` in project directory, searched up parent dirs
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
