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
cargo install cosq
```

### cargo binstall

Download a pre-built binary via [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall cosq
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
