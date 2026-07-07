# Cosq 1.0 Phase 2 — Shell Implementation Plan

> Executed inline (superpowers:executing-plans). Spec §5.

**Goal:** `cosq shell` — persistent REPL with context, completions, history.

## Tasks
1. **Dep + skeleton**: `reedline = "0.40"` (cosq crate). New `commands/shell.rs`:
   `ShellContext { config: Config, profile_name: String, profile: Profile, client: CosmosClient, database: Option<String>, container: Option<String>, format: OutputFormat }`.
   Prompt `cosq (profile) db/container » ` via a custom `reedline::Prompt`.
2. **Input dispatch**: lines starting `:` → meta; `?` → ask (stub until phase 3);
   otherwise SQL. Multi-line: custom `Validator` — input is complete when it
   ends with `;` OR is a single line whose parens/quotes balance; `;` stripped
   before execution.
3. **Meta commands**: `:help`, `:quit`/`:exit`, `:profile <p>` (re-resolves
   client), `:db <d>`, `:container <c>`, `:format <f>`, `:queries` (list stored),
   `:run <name> [--key value...]`, `:schema`/`:search`/`:explain` stubs that
   say which phase wires them (removed before release).
4. **SQL execution**: same path as `cosq query` (pk auto-scope, QueryOptions,
   per-range RU on `-v`), results via `output::write_results` with the
   context's format; RU to stderr dim line.
5. **Completion**: `reedline::Completer` — meta command names; `:db`/`:container`
   args from cached listings (fetched lazily, cached in context); `:run` args
   from stored-query names; `:format` from format names.
6. **History**: `FileBackedHistory` at `~/.cosq/history` (500 entries),
   Ctrl-R via reedline defaults.
7. **Tests**: input-classification + validator unit tests (pure logic);
   completer candidates test. Interactive loop itself is exercised live.
8. Gate: fmt/clippy/test + live smoke (scripted stdin into the shell).
