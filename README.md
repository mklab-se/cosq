# cosq

[![CI](https://github.com/mklab-se/cosq/actions/workflows/ci.yml/badge.svg)](https://github.com/mklab-se/cosq/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cosq-cli.svg)](https://crates.io/crates/cosq-cli)
[![GitHub Release](https://img.shields.io/github/v/release/mklab-se/cosq)](https://github.com/mklab-se/cosq/releases/latest)
[![Homebrew](https://img.shields.io/badge/dynamic/regex?url=https%3A%2F%2Fraw.githubusercontent.com%2Fmklab-se%2Fhomebrew-tap%2Fmain%2FFormula%2Fcosq.rb&search=%5Cd%2B%5C.%5Cd%2B%5C.%5Cd%2B&label=homebrew&prefix=v&color=orange)](https://github.com/mklab-se/homebrew-tap/blob/main/Formula/cosq.rb)
[![License](https://img.shields.io/crates/l/cosq-cli.svg)](https://github.com/mklab-se/cosq/blob/main/LICENSE.md)

A CLI to query your Azure Cosmos DB instances from the command line.

## Installation

### Homebrew (macOS / Linux)

```bash
brew install mklab-se/tap/cosq
```

### Pre-built Binaries

Download the latest binary for your platform from [GitHub Releases](https://github.com/mklab-se/cosq/releases/latest):

| Platform | Archive |
|---|---|
| macOS (Apple Silicon) | `cosq-v*-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `cosq-v*-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `cosq-v*-x86_64-unknown-linux-gnu.tar.gz` |
| Windows (x86_64) | `cosq-v*-x86_64-pc-windows-msvc.zip` |

Extract and move the binary to a directory in your `PATH`:

```bash
tar xzf cosq-v*-*.tar.gz
sudo mv cosq /usr/local/bin/
```

### cargo install

Compile from source via crates.io (requires Rust 1.85+):

```bash
cargo install cosq-cli
```

### cargo binstall

Download a pre-built binary via [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall cosq-cli
```

### Build from Source

```bash
git clone https://github.com/mklab-se/cosq.git
cd cosq
cargo build --release
```

The binary is at `target/release/cosq`.

## Verify Installation

```bash
cosq --version
```

## Development

```bash
cargo build              # Build
cargo test               # Run tests
cargo clippy             # Lint
cargo fmt                # Format
```

## License

MIT
