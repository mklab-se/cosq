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
description: Query Azure Cosmos DB — ad-hoc SQL, stored queries with parameters, multi-step pipelines, AI query generation, and flexible output formatting.
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

- `cosq query "<SQL>" --db <db> --container <c>` — run ad-hoc SQL
- `cosq run <name> -- --param value` — run a stored query
- `cosq queries list` — list stored queries
- `cosq queries generate "<description>"` — AI-generate a stored query
- `cosq queries create <name>` — create a new stored query
- `cosq auth status` — check Azure login
- `cosq ai status` — check AI feature status
"#
    );
}

fn print_reference() {
    print!("{REFERENCE_DOC}");
}
