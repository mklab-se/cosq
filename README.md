<p align="center">
  <img src="https://raw.githubusercontent.com/mklab-se/cosq/main/media/cosq-horizontal.png" alt="cosq" width="600">
</p>

<h1 align="center">cosq</h1>

<p align="center">
  A CLI to query your <a href="https://learn.microsoft.com/en-us/azure/cosmos-db/">Azure Cosmos DB</a> instances from the command line.
</p>

<p align="center">
  <a href="https://github.com/mklab-se/cosq/actions/workflows/ci.yml"><img src="https://github.com/mklab-se/cosq/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/cosq"><img src="https://img.shields.io/crates/v/cosq.svg" alt="crates.io"></a>
  <a href="https://github.com/mklab-se/cosq/releases/latest"><img src="https://img.shields.io/github/v/release/mklab-se/cosq" alt="GitHub Release"></a>
  <a href="https://github.com/mklab-se/homebrew-tap/blob/main/Formula/cosq.rb"><img src="https://img.shields.io/badge/dynamic/regex?url=https%3A%2F%2Fraw.githubusercontent.com%2Fmklab-se%2Fhomebrew-tap%2Fmain%2FFormula%2Fcosq.rb&search=%5Cd%2B%5C.%5Cd%2B%5C.%5Cd%2B&label=homebrew&prefix=v&color=orange" alt="Homebrew"></a>
  <a href="https://github.com/mklab-se/cosq/blob/main/LICENSE.md"><img src="https://img.shields.io/crates/l/cosq.svg" alt="License"></a>
</p>

## Quick Start

```bash
# Install (macOS / Linux)
brew install mklab-se/tap/cosq

# Or via cargo
cargo install cosq

# Login to Azure
cosq auth login

# Initialize with a Cosmos DB account
cosq init

# Run a query
cosq query "SELECT * FROM c"

# Output as table or CSV
cosq query "SELECT * FROM c" --output table
cosq query "SELECT * FROM c" --output csv

# Pipe-friendly (JSON to stdout, metadata to stderr)
cosq query "SELECT c.name FROM c" -q | jq '.[].name'
```

## Stored Queries

Save and reuse parameterized queries as `.cosq` files:

```bash
# Create a stored query (opens in editor)
cosq queries create recent-users

# List all stored queries
cosq queries list

# Run a stored query (interactive parameter prompts)
cosq run recent-users

# Run with parameters from the command line
cosq run recent-users -- --days 7

# Browse and pick a query interactively
cosq run
```

## AI Query Generation

Generate stored queries from natural language using your preferred AI provider:

```bash
# Set up AI (auto-detects Claude, Codex, Copilot, Ollama, or Azure OpenAI)
cosq ai init

# Generate a query from a description
cosq queries generate "active users by region in the last 30 days"
```

See [INSTALL.md](INSTALL.md) for all installation methods, shell completions, and platform-specific instructions.

## Development

```bash
cargo build              # Build
cargo test               # Run tests
cargo clippy             # Lint
cargo fmt                # Format
```

## License

MIT
