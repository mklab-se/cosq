# Cosq 1.0 Phase 1 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modernize cosq's engine — verified wire version, parallel + partition-scoped queries, cached tokens, multi-account profiles, listing commands, current deps — as the base for shell/AI/search phases.

**Architecture:** Evolve the existing 3-crate workspace (`cosq` CLI, `cosq-core` config/stored-queries, `cosq-client` REST). All changes stay within the hand-rolled REST client (approved decision; no SDK).

**Tech Stack:** Rust 2024, tokio, reqwest 0.12, serde_yaml_ng (replacing serde_yaml), wiremock (new dev-dep), clap 4.

**Spec:** `docs/superpowers/specs/2026-07-07-cosq-1.0-design.md` §4 (+ §2 facts). Phases 2–4 get their own plans.

## Global Constraints
- Read-only tool: no write operations, ever.
- Gates per CLAUDE.md: `cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings && cargo test` (workspace); also `--all-targets` clippy clean.
- Live testing only against Kristofer's Azure (Cosmos account discovered via az); create nothing outside existing accounts; the spike may create a tiny vector-enabled container in an existing account and MUST delete it.
- Results→stdout, metadata/RU→stderr (existing contract, don't break).
- Commit per task; conventional commits; no secrets in code or fixtures.

---

### Task 0: Housekeeping (deps + doc fixes)
**Files:** `Cargo.toml` (workspace), `crates/*/Cargo.toml`, `README.md:100`, `doc/ai-reference.md:231`, `crates/cosq-client/src/error.rs:25-29,62-72`
- [ ] `serde_yaml = "0.9"` → `serde_yaml = { package = "serde_yaml_ng", version = "0.10" }`; `colored` → `"3"`; fix any colored 3 API fallout (`.dimmed()` etc. unchanged; check `set_override`).
- [ ] Delete dead `ClientError::OpenAI`/`LocalAgent` variants + their arms.
- [ ] README: `cosq ai init` → `cosq ai config` + `cosq ai enable`. ai-reference: `pad(start=10)`→`pad(10)`, `truncate(length=30)`→`truncate(30)`.
- [ ] Gates green → commit `chore: modernize deps, fix stale docs, drop dead error variants`.

### Task 1: Wire-version + search-functions spike (LIVE)
**Files:** `crates/cosq-client/src/cosmos.rs:14` (constant + findings comment); scratch script (not committed)
- [ ] Discover a Cosmos account via `az cosmosdb list`; create a throwaway container WITH vector policy + full-text policy in an existing account/db (smallest RU), insert 3 docs with tiny vectors (e.g. 8-dim) via REST PUT (spike-only write, allowed for setup), then:
- [ ] Probe `x-ms-version` values (`2018-12-31`, `2020-07-15`, and the newest dated values Azure accepts) on plain queries → record which are accepted.
- [ ] Run `SELECT TOP 2 ... ORDER BY VectorDistance(c.v, [..])`, `FullTextScore`+`ORDER BY RANK`, and `RRF(...)` through `POST /docs` per version → record which succeed.
- [ ] Set `API_VERSION` to the newest fully-working value; add a findings comment block above it (versions probed, what worked, date). Delete the container. Commit `feat(client): adopt verified x-ms-version (spike findings recorded)`.

### Task 2: Parallel fan-out
**Files:** `crates/cosq-client/src/cosmos.rs:200-337`; test `crates/cosq-client/tests/query_engine.rs` (new, wiremock)
**Interfaces (produces):** `QueryResult { documents: Vec<Value>, request_charge: f64, per_range: Vec<(String, f64)> }` (extend existing return type with `per_range`).
- [ ] Add `wiremock = "0.6"` dev-dep. Failing test: mock `/pkranges` (3 ranges) + per-range `/docs` responses with `x-ms-request-charge`; assert all 3 range queries are issued, documents concatenated in range order, RU summed, `per_range` populated. A slow-range mock (`set_delay`) must not block the others (elapsed < sum of delays).
- [ ] Implement: `futures::stream::iter(ranges).map(|r| query_range(...)).buffered(8)` collecting in order (buffered preserves order); continuation loop stays per range. `-v` prints per-range RU to stderr (CLI side: `crates/cosq/src/commands/query.rs`).
- [ ] Tests pass → commit `feat(client): parallel cross-partition fan-out with per-range RU`.

### Task 3: Partition scoping + --pk/--first/--max-items
**Files:** `crates/cosq-client/src/cosmos.rs` (single-partition path), new `crates/cosq-core/src/pk_detect.rs`; CLI flags in `crates/cosq/src/cli.rs` (query + run), threading in `crates/cosq/src/commands/{query,run,common}.rs`; tests inline + wiremock
**Interfaces (produces):**
```rust
// cosq-core
pub fn detect_pk_equality(sql: &str, pk_path: &str, params: &[(String, Value)]) -> Option<Value>
// matches WHERE c.<pk> = <literal|@param> (case-insensitive keywords, AND-composable,
// bails to None on OR at top level); pk_path like "/customerId" or "/address/zip"
// cosmos.rs
pub async fn query_scoped(&self, db, container, query, params, pk_value: &Value, opts) -> QueryResult
// sends x-ms-documentdb-partitionkey: [<value>] with NO fanout
```
- [ ] Failing unit tests for `detect_pk_equality`: literal string/number, `@param` resolution, no-match (different field), OR → None, nested path `/a/b`, `c["field"]` bracket syntax → match.
- [ ] Implement (string scanning, no SQL parser dep: tokenize on whitespace/operators; conservative — return None when unsure).
- [ ] Container metadata: fetch + cache partitionKey path per container (client method `get_container(&db,&c) -> ContainerMeta { pk_paths: Vec<String> }`).
- [ ] Wire: query/run resolve pk (explicit `--pk` > detected) → `query_scoped`, else fan-out. `--first N` stops draining pages/ranges once N docs collected (fan-out: short-circuit buffered stream); `--max-items` sets `x-ms-max-item-count`.
- [ ] wiremock tests: scoped query sends the pk header and hits no `/pkranges`; `--first 5` stops requesting after 5 docs. Commit `feat: partition-scoped queries, --pk/--first/--max-items`.

### Task 4: Token cache
**Files:** `crates/cosq-client/src/auth.rs`; test inline (tempfile HOME override via `COSQ_CACHE_DIR` env knob)
**Interfaces:** existing `get_token(resource) -> String` keeps signature; adds disk cache.
- [ ] Failing test: with `COSQ_CACHE_DIR` set, first `get_token` (az mocked via injectable `TokenSource` trait — refactor az call behind `trait TokenSource { fn fetch(&self, resource) -> Result<TokenInfo> }`) writes `tokens.json` (0600) with `expires_on`; second call within validity does NOT invoke the source; expired entry re-fetches.
- [ ] Implement: cache file `{cache_dir}/tokens.json` keyed by resource, 5-min expiry skew, permissions 0o600, corrupt cache → ignore + refetch. az `expiresOn` parsed from `az account get-access-token` JSON (switch from `-o tsv` accessToken-only to JSON to get expiry).
- [ ] Commit `feat(client): cached AAD tokens (one az call per expiry window)`.

### Task 5: Profiles
**Files:** `crates/cosq-core/src/config.rs` (rewrite model), `crates/cosq/src/commands/init.rs`, `common.rs`, `cli.rs` (global `--profile`, env `COSQ_PROFILE`); tests inline
**Interfaces (produces):**
```rust
pub struct Config { pub default_profile: Option<String>, pub profiles: BTreeMap<String, Profile> }
pub struct Profile { pub subscription: String, pub account: String, pub endpoint: String,
                     pub database: Option<String>, pub container: Option<String>,
                     pub embed_models: BTreeMap<String, String> /* container -> ailloy node; used phase 4 */ }
impl Config { pub fn active(&self, selected: Option<&str>) -> Result<(&str, &Profile)> } // flag > COSQ_PROFILE > default_profile > sole entry
```
- [ ] Failing tests: parse profile yaml; `active()` precedence incl. sole-entry fallback + unknown-profile error listing available; save round-trip. No migration from old format (spec: none) — loader error for old shape says "config format changed in 1.0 — run `cosq init`".
- [ ] Implement; `cosq init` writes/updates a named profile (`--name`, default "default"); last-used db/container writes to the active profile.
- [ ] Commit `feat!: multi-account profiles (--profile / COSQ_PROFILE)`.

### Task 6: Listing commands
**Files:** `crates/cosq/src/cli.rs`, new `crates/cosq/src/commands/list.rs`
- [ ] `cosq databases` and `cosq containers [--db]`: call existing client list methods, honor `-o table|json` (default table: name + pk path for containers), RU to stderr. assert_cmd test against wiremock (set endpoint via profile config pointing at mock; token source stubbed with `COSQ_TEST_TOKEN` env honored by TokenSource).
- [ ] Commit `feat: databases/containers listing commands`.

### Task 7: Phase gate
- [ ] Full gates + `cargo build --release`; CHANGELOG "Unreleased" section describing phase 1 (release happens after phase 4); README quick-start updated for profiles.
- [ ] Live smoke: `cosq init` against real account → `databases`/`containers` → a scoped query (`--pk`) and a fan-out query with `-v` per-range RU → verify token cache file exists with 0600.
- [ ] Commit `feat: phase 1 foundation complete`.

## Milestone map (later plans)
- **Phase 2 — shell**: reedline REPL, context, completions (needs schema cards stub for fields → cards module lands early in phase 3; field completion activates then).
- **Phase 3 — schema cards + ask** (+ `queries generate` refactor).
- **Phase 4 — search + doctor + skill/docs + release 1.0.0** (incl. `test-live-experience` skill with ailloy-eval judging).

## Self-review
Spec §4 fully covered (engine→T1-3, auth/profiles→T4-5, housekeeping→T0/T6, read-only preserved). Types consistent (QueryResult/Profile used across tasks). The spike's temporary container is the only write and is deleted in-task. No placeholders.
