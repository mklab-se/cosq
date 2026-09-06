# Changelog

All notable changes to this project will be documented in this file.

## [1.1.0] - 2026-09-07

### Added

- Supply-chain transparency for release builds: binaries are built with `cargo auditable`
  (dependency list embedded in the executable, readable with `cargo audit bin` or `syft`), and
  a per-target CycloneDX 1.5 SBOM (`cosq-vX.Y.Z-<target>.cdx.json`) is attached to every
  GitHub release.

### Changed

- Dependencies upgraded to current majors: Ailloy 1.0 → 2.1, `comfy-table` 7 → 8,
  `reedline` 0.42 → 0.51, `inquire` 0.7 → 0.9, `dirs` 6 → 7 (the `cosq-client` crate no longer
  pins its own `dirs` 5), `base64` 0.22 → 0.23, plus `clap` 4.6, `tokio` 1.53, `uuid` 1.26,
  `minijinja` 2.24 and a `cargo update` across the lockfile. `reqwest` stays on 0.12 to share a
  single TLS stack with Ailloy.
- Minimum supported Rust version raised from 1.85 to 1.95 (required by `reedline` 0.51).
- GitHub Actions workflows moved to the Node 24 action majors (`actions/checkout@v7`,
  `actions/upload-artifact@v7`, `actions/download-artifact@v8`, `softprops/action-gh-release@v3`).

## [1.0.0] - 2026-07-07

From "run SQL from the terminal" to "talk to your data" — while staying a
lean, pipeable, **read-only** CLI. No backward compatibility with 0.x configs
(re-run `cosq init`).

### Added — AI layer

- **`cosq ask`**: natural-language question → Cosmos SQL (via ailloy
  structured output, grounded in the schema card) → executed answer, with
  `--save`, `--sql-only`, `--yes`, confidence-gated confirmation, and
  automatic partition scoping of generated SQL.
- **Schema cards**: cached per-container YAML knowledge (field paths/types/
  examples/value sets, AI descriptions, inferred cross-container
  relationships, partition key, vector/full-text policies) at
  `~/.cosq/schema/<profile>/…`, 7-day TTL, project-local `.cosq/schema/`
  override. `cosq schema [container] [--refresh] [--json]`.
- **`cosq search`**: semantic / full-text / hybrid search using Cosmos DB's
  native engine — query text embedded via ailloy (embed node matched by
  vector dimensions, remembered per container), `VectorDistance` with exact
  cross-partition score merge, BM25 `FullTextScore`, `RRF` hybrid, keyword
  CONTAINS fallback. No local vector store.
- **`cosq explain`** (query doctor): RU + per-range query metrics, decoded
  index utilization with recommended single/composite indexes, and an AI
  diagnosis with the concrete indexingPolicy fix.
- **`cosq shell`**: reedline REPL with context prompt
  (`cosq (profile) db/container »`), direct SQL (multi-line), `? question`
  ask-mode with composing follow-ups (conversation memory), `:` meta-commands,
  tab completion (metas, databases, containers, stored queries), persistent
  history, and scriptable non-TTY mode.
- `queries generate` now builds on schema cards (no per-run resampling).
- Refreshed `cosq ai skill` (agent skill + rewritten ai-reference).

### Phase 1 — Foundation

- **Parallel cross-partition fan-out** (bounded at 8 concurrent ranges) with
  per-range RU breakdown at `-v`; queries preserve range order.
- **Partition scoping**: queries whose WHERE clause pins the partition key run
  against a single partition (no fan-out); `--pk` forces it. New `--first`
  and `--max-items` flags.
- **Cached AAD tokens** (`~/.cache/cosq/tokens.json`, 0600) — one `az` call
  per expiry window instead of per command. `COSQ_CACHE_DIR` override.
- **Multi-account profiles** (breaking): config is now named profiles with
  `default_profile`; select with `--profile` / `COSQ_PROFILE`. Old configs get
  a clear "run `cosq init`" error. `cosq init --name <profile>` adds more.
  `COSQ_CONFIG_DIR` override for isolated setups.
- **`cosq databases` / `cosq containers`** listing commands (containers show
  partition key + vector/full-text policy indicators).
- Wire version `2020-07-15` with recorded live spike findings: vector/FTS/RANK
  queries execute per partition-key-range (cosq's fan-out) and pk-scoped;
  VectorDistance projects a mergeable score.
- Deps: serde_yaml → serde_yaml_ng, colored 3; dead error variants removed;
  stale docs fixed.

## [0.9.1] - 2026-07-07

### Changed

- Upgraded to ailloy 1.0: Azure OpenAI / Microsoft Foundry requests now use
  the unified `/openai/v1/` surface, models that reject sampling parameters
  are retried automatically, and current default models (gpt-5.4-mini,
  claude-sonnet-5) replace retiring ones.

## [0.9.0] - 2026-03-20

### Added

- **`cosq ai skill` command** — helps AI coding agents (like Claude Code) set up skills for working with cosq. Use `--emit` to output a ready-to-save skill file, or `--reference` for comprehensive reference documentation
- **`cosq ai status` subcommand** — explicit subcommand for showing AI status (same as running `cosq ai` with no subcommand)

## [0.8.0] - 2026-03-18

### Changed

- **Upgraded ailloy to v0.5** — AI configuration now uses ailloy's built-in interactive TUI (`config_tui`) instead of cosq's own status, enable/disable, and editor-based config commands. Removes ~170 lines of AI management code from cosq
- **`cosq ai config` is now interactive** — launches ailloy's interactive configuration flow instead of opening a YAML file in an editor
- **`cosq ai` status/enable/disable delegated to ailloy** — all AI subcommands now use ailloy's `config_tui` module for consistent behavior across ailloy-based tools

## [0.7.0] - 2026-03-06

### Changed

- **Replaced built-in AI providers with ailloy** — AI configuration now uses the `ailloy` unified AI library (`~/.config/ailloy/config.yaml`) instead of cosq's own per-provider setup. Removes ~840 lines of provider-specific code (`openai.rs`, `local_agent.rs`, `ollama.rs`) and the `AiProvider`/`AiConfig` types
- **Redesigned `cosq ai` subcommands** — replaced `cosq ai init` and `cosq ai status` with a cleaner set: `cosq ai` (status), `cosq ai test` (test connection), `cosq ai enable`/`disable` (toggle AI features), `cosq ai config` (interactive AI configuration)
- **AI config no longer stored in cosq config** — the `ai:` section in `~/.config/cosq/config.yaml` is no longer used. Run `cosq ai config` to set up ailloy instead

## [0.6.1] - 2026-02-26

### Fixed

- **Arrow key navigation in interactive prompts** — replaced `dialoguer` with `inquire` (uses `crossterm` for terminal I/O). Arrow keys now work reliably for navigating selection lists instead of displaying "A"/"B" characters
- **Query generation clarification loop** — when the AI returns a `.cosq` file that fails to parse, cosq now auto-retries with the parse error instead of showing the raw file content and asking "Your answer:" with no visible question
- **Removed "The AI needs clarification" phrasing** — cosq no longer refers to AI as a separate entity; clarification questions are shown directly

### Added

- **`cosq ai status` command** — shows the current AI provider configuration (provider, model, account, deployment, endpoint)

## [0.6.0] - 2026-02-26

### Added

- **Multi-step queries** — stored queries can now execute across multiple containers in a single run. Define `steps:` in the YAML front matter with per-step containers, and use `-- step: <name>` markers to separate SQL blocks. Steps execute in dependency order with automatic parallel execution for independent steps
- **Cross-step references** — use `@step.field` syntax (e.g., `@customer.id`) to reference the first result of a previous step, enabling chained queries like "find customer by name, then get their orders"
- **Pipeline executor (`commands/pipeline.rs`)** — topological sort of step dependencies into execution layers. Independent steps in the same layer run concurrently via `tokio::spawn`
- **Multi-step template rendering** — `render_multi_step_template()` makes each step's results available as top-level template variables by step name
- **Custom MiniJinja filters** — `truncate(length)` truncates strings with "..." suffix, `pad(width)` left-aligns strings to minimum width. Fixes "unknown filter" errors for AI-generated templates that use these common filters
- **AI-assisted template error recovery** — when a template fails during `cosq run`, cosq now offers to fix it via AI. The fixed template can be saved back to the `.cosq` file
- **Run-after-generate** — after `cosq queries generate` creates a query, cosq now offers to run it immediately to verify it works, or open it in an editor
- **Multi-container AI generation** — `cosq queries generate` now lets you pick one or multiple containers. When multiple containers are selected, the AI generates multi-step queries with the correct syntax. Fan-out queries are explicitly out of scope

### Changed

- `cosq queries generate` now always prompts for container selection (previously silently used the configured default, even when multiple containers existed)
- `CosmosClient` now implements `Clone` to support parallel step execution

## [0.5.0] - 2026-02-26

### Added

- **Schema-aware AI query generation** — `cosq queries generate` now connects to the target Cosmos DB container, samples real documents, and includes the schema context in the AI prompt. The AI only references fields that actually exist in your data
- **Automatic template generation** — AI-generated queries now always include a MiniJinja output template: table layout for lists, property-value layout for single results, CSV for "CSV" requests, and no template when "JSON" is requested
- **Fully interactive mode** — running `cosq queries generate` with no arguments interactively picks a database, container, and prompts for a natural language description
- **Multi-turn conversation** — if the AI needs clarification (e.g. ambiguous field names), it asks 1-3 questions and the user answers before generation (up to 3 rounds)
- **`--db` and `--container` flags** on `cosq queries generate` for non-interactive database/container selection
- **Shared DB/container resolution helper (`commands/common.rs`)** — standard fallback chain (CLI flag > query metadata > config > interactive picker) used by `query`, `run`, and `queries generate`. Auto-selects when only one option exists

### Changed

- `cosq queries generate` description argument is now optional (prompts interactively if omitted)
- Generated queries auto-populate `database` and `container` metadata from the resolved target
- Refactored `query.rs` and `run.rs` to use shared resolution helpers, eliminating ~100 lines of duplicated code

## [0.4.0] - 2026-02-26

### Added

- **Multi-provider AI support** — `cosq queries generate` now works with local CLI AI agents (Claude, Codex, Copilot), Ollama local LLMs, and Azure OpenAI API. No longer requires an Azure subscription for AI features
- **`cosq ai init` command** — interactive setup for AI provider configuration. Auto-detects available tools on the system, presents a fuzzy-select list, and guides through provider-specific setup (model selection, Ollama model picker, Azure OpenAI account/deployment input)
- **Local AI agent integration (`cosq-client/local_agent.rs`)** — invokes Claude (`claude -p`), Codex (`codex exec`), and Copilot (`copilot -p`) as subprocesses with provider-specific flags for non-interactive operation
- **Ollama client (`cosq-client/ollama.rs`)** — direct HTTP API integration for local LLMs via Ollama. Lists installed models for interactive selection, sends chat completions with system/user prompts
- **Unified AI dispatcher (`cosq-client/ai.rs`)** — single `generate_text()` function that routes to the configured provider (Azure OpenAI, local CLI agent, or Ollama)
- **`AiProvider` enum** — `azure-openai`, `claude`, `codex`, `copilot`, `ollama` with recommended default models per provider (sonnet for Claude, o4-mini for Codex, gpt-4.1 for Copilot)
- **Configurable model selection** — optional `model` field in AI config overrides the provider default. Each provider suggests a recommended model during `cosq ai init`

### Changed

- AI config now supports `provider` and `model` fields (backward-compatible — existing configs without `provider` default to `azure-openai`)
- `cosq queries generate` error message now directs to `cosq ai init` instead of manual YAML editing
- `generated_by` metadata now includes provider name and model (e.g., "Claude (sonnet)")

## [0.3.0] - 2026-02-26

### Added

- **Stored queries** — save SQL queries as `.cosq` files in `~/.cosq/queries/` (user-level) or `.cosq/queries/` (project-level). YAML front matter for metadata (description, database, container, parameters, templates) with the SQL body below. Project-level queries override user-level ones with the same name
- **`cosq run` command** — execute stored queries by name with parameterized inputs. Parameters can be passed via CLI (`-- --days 7 --status shipped`) or resolved interactively with fuzzy-select for choice parameters and text input with defaults. Running `cosq run` without a name opens an interactive query picker
- **`cosq queries` subcommands** — `list` (show all stored queries with descriptions), `create` (scaffold and open in editor), `edit` (open in `$VISUAL`/`$EDITOR`), `delete` (with confirmation), `show` (display query details, parameters, SQL, and template)
- **AI query generation (`cosq queries generate`)** — generate stored queries from natural language descriptions using Azure OpenAI. Automatically extracts parameters, generates SQL, creates MiniJinja output templates, and saves as `.cosq` files. Requires `ai:` config section with Azure OpenAI account and deployment
- **Output formatting** — new `--output` flag on `query` and `run` commands supporting `json` (default), `json-compact` (one line per document), `table` (Unicode columnar via comfy-table), and `csv`. Complex nested values are truncated intelligently in table/CSV output
- **Template output** — MiniJinja-based output templates via `--template <file>` or embedded in stored query files. Templates have access to `documents` array and all parameter values. Stored queries with templates auto-use them unless `--output` is explicitly specified
- **Parameterized Cosmos DB queries** — new `query_with_params()` method in cosq-client passes parameters to the Cosmos DB REST API natively via the `parameters` array, preventing SQL injection
- **Azure OpenAI client (`cosq-client/openai.rs`)** — chat completion via Azure OpenAI REST API with AAD token authentication (cognitive services scope), following the same pattern as the hoist project
- **AI configuration** — optional `ai:` section in `config.yaml` with `account`, `deployment`, and `api_version` fields for Azure OpenAI integration
- **Dynamic shell completions** — stored query names now tab-complete in `cosq run`, `cosq queries edit/delete/show`. Enable with `source <(COMPLETE=bash cosq)` (or zsh/fish). The `cosq completion` command now shows a tip about this
- **Interactive query picker** — `cosq run` without arguments shows a fuzzy-select list of all stored queries with descriptions

### Changed

- `cosq query` now supports `--output` and `--template` flags for output formatting (previously only JSON)
- Shell completions now support dynamic mode via `clap_complete` `CompleteEnv` for runtime query name resolution

## [0.2.2] - 2026-02-25

### Fixed

- **Native TLS root certificates** — switched from bundled Mozilla CA roots to OS-native certificate stores (`rustls-tls-native-roots`). Fixes `UnknownIssuer` TLS errors on corporate networks using TLS inspection with custom CA certificates
- **TLS error diagnostics** — certificate verification failures now show a specific message explaining the likely cause (corporate TLS inspection) with OS-specific fix instructions for macOS, Linux, and Windows

## [0.2.1] - 2026-02-25

### Added

- **SQL query execution (`cosq query`)** — execute SQL queries against Cosmos DB from the command line. Resolves database and container from `--db`/`--container` flags, `cosq.yaml` config, or interactive fuzzy-select prompts. Handles cross-partition queries via partition key range fanout, pagination via `x-ms-continuation`, displays request charge (RUs) on stderr, and outputs JSON to stdout for pipe-friendly workflows. First interactive selection is saved to `cosq.yaml` for subsequent runs
- **Automatic data plane RBAC setup** — `cosq init` now checks if the user has Cosmos DB data plane access and offers to assign the Data Contributor role automatically. Supports `--yes` flag for non-interactive use
- **Cosmos DB data plane client (`cosq-client/cosmos.rs`)** — REST API client with AAD token authentication, partition key range fanout, database/container listing, and paginated SQL query execution

### Changed

- Config moved from project-local `cosq.yaml` to `~/.config/cosq/config.yaml` (platform-appropriate via `dirs::config_dir()`), no longer pollutes project directories
- Config now supports optional `database` and `container` fields (backward-compatible)
- `cosq init` now accepts `--yes`/`-y` flag to auto-confirm prompts

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
