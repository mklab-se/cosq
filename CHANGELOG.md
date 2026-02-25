# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-02-25

### Added

- **Azure authentication commands** — `cosq auth status` shows Azure CLI login status and tests Cosmos DB token acquisition. `cosq auth login` and `cosq auth logout` wrap the Azure CLI for convenience
- **Interactive account setup (`cosq init`)** — discovers Azure subscriptions and Cosmos DB accounts via ARM APIs, lets you select interactively (with fuzzy search), and saves the selection to `cosq.yaml`. Supports `--account` and `--subscription` flags for non-interactive use
- **Shell completions** — `cosq completion <shell>` generates completions for Bash, Zsh, Fish, and PowerShell
- **Background update checker** — queries crates.io for the latest version, caches results for 24 hours at `~/.cache/cosq/`, and prints a notification if a newer version is available. Detects install method (Homebrew, cargo, cargo-binstall) and shows the appropriate upgrade command. Disable with `COSQ_NO_UPDATE_CHECK=1`
- **cosq-core crate** — core types and configuration. `Config` struct for `cosq.yaml` with load/save and parent directory search
- **cosq-client crate** — Azure Cosmos DB client library with Azure CLI authentication (`AzCliAuth`), ARM resource discovery (`ArmClient` for subscriptions and Cosmos DB accounts), and typed error handling (`ClientError`) with helpful hints
- **Hero image and centered layout in README**
- **INSTALL.md** — dedicated installation guide with shell completions documentation
- **CHANGELOG.md** — this file

### Changed

- Running `cosq` with no subcommand now shows help instead of the ASCII banner (better UX for a query tool)
- Workspace expanded from 1 crate to 3 (cosq, cosq-core, cosq-client)

## [0.1.1] - 2026-02-24

### Changed

- Renamed package from `cosq-cli` to `cosq` for simpler `cargo install cosq`
- Bumped version to reflect the rename

## [0.1.0] - 2026-02-24

### Added

- Initial release
- CLI skeleton with `clap` derive macros, ASCII banner, and version command
- Async runtime with `tokio`
- Logging via `tracing` with `-v`/`-vv` verbosity levels
- Colored output via `colored` (respects `--no-color`)
- CI/CD pipeline: GitHub Actions for build/test/lint, release workflow for cross-platform binaries, Homebrew tap, and crates.io publishing
- Published to crates.io
- Cross-platform release binaries (Linux, macOS Intel+ARM, Windows)
