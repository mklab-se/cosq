//! AI agent skill information for cosq
//!
//! Provides skill file content and reference documentation to help
//! AI coding agents (like Claude Code) work effectively with cosq.

const REFERENCE_DOC: &str = include_str!("../../doc/ai-reference.md");

/// Run the `cosq ai skill` command.
///
/// - No flags: print a human-readable setup guide.
/// - `--emit`: print a Claude Code skill markdown file to stdout.
/// - `--reference`: print comprehensive reference documentation.
pub fn run(emit: bool, reference: bool) {
    if emit {
        print_skill_file();
    } else if reference {
        print_reference();
    } else {
        print_guide();
    }
}

fn print_guide() {
    println!(
        r#"cosq AI Skill Setup
===================

cosq is a CLI for querying Azure Cosmos DB instances. A skill helps AI
agents create and execute Cosmos DB queries using cosq.

To create the skill file, run:

  cosq ai skill --emit > ~/.claude/skills/cosq.md

Or ask your AI agent:

  "Use `cosq ai skill --emit` to set up a skill for querying Cosmos DB"

The skill instructs the AI agent to run `cosq ai skill --reference` at
runtime to fetch full documentation, so the agent always has up-to-date
command details and query syntax without bloating the skill file itself.
"#
    );
}

fn print_skill_file() {
    print!(
        r#"---
name: cosq
description: Query Azure Cosmos DB (read-only) — natural-language ask, ad-hoc SQL, semantic/full-text search, schema cards, query doctor, stored queries, pipelines, shell, and flexible output formatting.
---

# cosq — Azure Cosmos DB CLI

Use cosq when the user needs to query or interact with Azure Cosmos DB.

## Before you start

Run this command to get full, up-to-date reference documentation:

```bash
cosq ai skill --reference
```

Read the output carefully — it covers every command, the stored query format,
multi-step syntax, parameter passing, output formats, and common workflows.

## Quick command reference

- `cosq ask "<question>" -y -o json` — natural-language question → SQL → results
- `cosq query "<SQL>" --db <db> --container <c> [--pk <v>] [--first N]` — ad-hoc SQL
- `cosq search "<text>" [--mode vector|text|hybrid] [--top N]` — semantic/full-text search
- `cosq schema <container> --json` — the container's schema card (fields, types, policies)
- `cosq explain "<SQL>"` — query cost, index usage, and recommendations
- `cosq databases` / `cosq containers` — listings (with `--json`)
- `cosq run <name> -- --param value` — run a stored query
- `cosq queries list` / `generate` / `create` — stored query management
- `cosq shell` — interactive REPL (also accepts piped scripts)
- `cosq auth status` / `cosq ai status` — login and AI health
- `--profile <name>` selects the account profile; `-q` + `-o json` for clean piping
"#
    );
}

fn print_reference() {
    print!("{REFERENCE_DOC}");
}
