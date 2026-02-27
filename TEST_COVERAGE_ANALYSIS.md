# Test Coverage Analysis

*Generated: 2026-02-27*

## Current State: 99 tests across 12/25 source files

| Crate | Source Files | Files w/ Tests | Test Functions | Assessment |
|-------|-------------|----------------|----------------|------------|
| **cosq** (CLI) | 14 | 5 | 32 | Weak |
| **cosq-client** | 8 | 5 | 24 | Weak |
| **cosq-core** | 3 | 2 | 43 | Good |

All 99 tests pass. Zero doc-tests exist. No integration test directories (`tests/`) in any crate.

## Well-Tested Areas

- **`cosq-core/config.rs`** (18 tests) — Good roundtrip serialization, backward compat, AI config helpers
- **`cosq-core/stored_query.rs`** (25 tests) — Solid parsing, parameter validation, multi-step queries, error paths
- **`cosq/output.rs`** (14 tests) — All 5 output formats covered, template filters tested
- **`cosq-client/error.rs`** (6 tests) — Certificate error detection, message extraction

## Priority Coverage Gaps (Ranked by Risk)

### 1. `cosq-client/auth.rs` — 0 tests, critical path

Every operation depends on auth. The JSON response parsing (`AzAccountInfo` deserialization), empty-token detection, "az login" hint detection in stderr, and non-zero exit code handling are all untested because the logic is tightly coupled to subprocess calls.

**Recommendation:** Extract response parsing into testable pure functions, then test deserialization of `az account show` output, error classification, and token extraction.

### 2. `cosq-client/arm.rs` — 0 tests, fragile parsing

Contains inline resource group extraction by splitting ARM resource IDs on `/resourceGroups/` — a brittle string operation. The `SubscriptionListResponse`, `CosmosAccountResource`, and role assignment structs are never tested for deserialization.

**Recommendation:** Add deserialization tests with sample ARM API JSON, and extract + test the resource group parsing as a standalone function.

### 3. `cosq/commands/common.rs` — 0 tests, pure business logic

The `resolve_database()` and `resolve_container()` functions implement a 4-level fallback chain (CLI arg > query metadata > config file > interactive picker). This is pure, easily testable logic with zero coverage.

**Recommendation:** Test all fallback combinations — each level returning `Some` while earlier levels are `None`.

### 4. `cosq/update.rs` — 0 tests, complex logic

152 lines with cache expiry (24-hour TTL), semver comparison, install method detection (Homebrew vs cargo-binstall vs cargo), and crates.io response parsing — all untested.

**Recommendation:** Test `read_cache()` expiry logic, `detect_install_method()`, and version comparison.

### 5. `cosq-client/cosmos.rs` — 7 tests, but only deserialization

The core query engine has tests for response deserialization and header formatting, but zero coverage of: pagination via `x-ms-continuation`, cross-partition fanout, request charge accumulation, 403 error handling, and query parameter construction.

**Recommendation:** Test `query_partition()` pagination logic and error handling paths with mock HTTP responses.

### 6. `cosq/commands/run.rs` — 4 tests, only `parse_cli_params()`

This is a 499-line file with the `resolve_template_str()` fallback chain (CLI > metadata > file), multi-step vs single-step dispatch, and AI-assisted template recovery entirely untested.

**Recommendation:** Test `resolve_template_str()` with various input combinations.

### 7. `cosq/commands/pipeline.rs` — 3 tests, missing error paths

`build_step_params()` is tested for happy path and one error case, but missing: step not yet executed, field not found in results, and the parallel execution logic in `execute()`.

**Recommendation:** Add error-path tests for `build_step_params()` and mock-based tests for layer execution order.

### 8. `cosq/commands/queries.rs` — 9 tests, only utility functions

The 923-line file's CRUD commands (`list`, `create`, `edit`, `delete`, `show`) and the `generate()` AI flow are completely untested. Only helpers like `generate_filename()`, `strip_markdown_fences()`, and `truncate_for_prompt()` have coverage.

**Recommendation:** Test `build_system_prompt()` construction and `format_sample_documents()` truncation edge cases.

### 9. `cosq/cli.rs` — 0 tests, 301 lines

No tests for argument parsing. A `Cli::try_parse_from()` test suite could verify that flags, subcommands, and global options parse correctly and catch regressions from clap definition changes.

**Recommendation:** Add `try_parse_from` tests for major subcommands.

### 10. `cosq-client/local_agent.rs` — 3 tests, only `which()` and `is_available()`

The actual command construction (`invoke_claude()`, `invoke_codex()`, `invoke_copilot()`), timeout handling, and `run_command()` error paths are untested.

**Recommendation:** Test command argument assembly as pure functions.

## Potential Bug Found During Analysis

**`cosq/src/output.rs:56-64` — `truncate_filter()` panics on multi-byte UTF-8**

The `truncate_filter()` function uses byte-level string slicing (`value[..max]`) which will panic at runtime if the truncation point falls within a multi-byte UTF-8 character (e.g., accented characters, CJK text, emoji). This is a real correctness bug, not just a missing test — any Cosmos DB document containing non-ASCII text in a field rendered through a MiniJinja template with `| truncate` could crash the CLI.

**Fix:** Use `value.char_indices()` to find the correct byte boundary, or use `value.chars().take(max).collect::<String>()`.

## Structural Gaps

1. **No integration tests** — No `tests/` directory in any crate. The CLI binary is never exercised end-to-end.
2. **No mocking infrastructure** — External service interactions (Azure CLI, ARM API, Cosmos DB, AI providers) cannot be tested because there's no trait-based abstraction for HTTP clients or subprocess execution.
3. **No doc-tests** — Zero doc-tests across both library crates, despite having public APIs.
4. **Error display formatting untested** — `ClientError::Display`, `ConfigError::Display`, `StoredQueryError::Display` are all untested.

## Recommended Action Plan

### Quick wins (pure functions, no mocking needed)

1. `commands/common.rs` — test `resolve_database`/`resolve_container` fallback chain
2. `update.rs` — test cache expiry, version comparison, install method detection
3. `cli.rs` — test argument parsing with `try_parse_from`
4. `arm.rs` — test resource group extraction from resource IDs
5. `stored_query.rs` — add missing `BelowMin` validation, circular dependency, number parsing edge cases

### Medium effort (extract-and-test pattern)

6. `auth.rs` — extract JSON parsing, test deserialization and error classification
7. `cosmos.rs` — test pagination logic and error paths
8. `local_agent.rs` — test command argument construction
9. `commands/run.rs` — test `resolve_template_str()` fallback

### Larger effort (requires architectural changes)

10. Add trait-based HTTP client abstraction for mock-based testing of `cosmos.rs`, `arm.rs`, `openai.rs`, `ollama.rs`
11. Add CLI integration tests using `assert_cmd` or similar
12. Add doc-tests to public APIs in `cosq-core` and `cosq-client`
