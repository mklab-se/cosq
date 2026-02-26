# Installing cosq

## Prerequisites

cosq authenticates via the Azure CLI:

- Install the [Azure CLI](https://learn.microsoft.com/en-us/cli/azure/install-azure-cli) and run `az login`

If the Azure CLI is not installed, `cosq auth status` will provide instructions.

## Homebrew (macOS / Linux)

```bash
brew install mklab-se/tap/cosq
```

## Pre-built Binaries

Download the latest binary for your platform from [GitHub Releases](https://github.com/mklab-se/cosq/releases/latest):

| Platform | Archive |
|---|---|
| macOS (Apple Silicon) | `cosq-v*-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `cosq-v*-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `cosq-v*-x86_64-unknown-linux-gnu.tar.gz` |
| Windows (x86_64) | `cosq-v*-x86_64-pc-windows-msvc.zip` |

Extract and move the binary to a directory in your `PATH`:

```bash
# macOS / Linux
tar xzf cosq-v*-*.tar.gz
sudo mv cosq /usr/local/bin/
```

## cargo install

Compile from source via crates.io (requires Rust 1.85+):

```bash
cargo install cosq
```

## Build from Source

```bash
git clone https://github.com/mklab-se/cosq.git
cd cosq
cargo build --release
```

The binary is at `target/release/cosq`. Requires Rust 1.85 or later.

## cargo binstall

If you already have [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) installed, it can download a pre-built binary from GitHub Releases instead of compiling from source — combining the convenience of `cargo install` with the speed of a binary download:

```bash
cargo binstall cosq
```

If you don't have cargo-binstall, install it first:

```bash
cargo install cargo-binstall
```

For most users, Homebrew or a direct binary download from [GitHub Releases](https://github.com/mklab-se/cosq/releases/latest) is simpler.

## Shell Completions

### Dynamic Completions (recommended)

Dynamic completions include tab-completion for stored query names. Add to your shell config:

**Bash** — add to `~/.bashrc`:
```bash
source <(COMPLETE=bash cosq)
```

**Zsh** — add to `~/.zshrc`:
```bash
source <(COMPLETE=zsh cosq)
```

**Fish** — add to `~/.config/fish/config.fish`:
```bash
source (COMPLETE=fish cosq | psub)
```

### Static Completions

If you prefer static completions (no stored query name tab-completion), use `cosq completion <shell>`:

**Bash** — add to `~/.bashrc`:
```bash
source <(cosq completion bash)
```

**Zsh** — add to `~/.zshrc`:
```bash
source <(cosq completion zsh)
```

**Fish** — save to completions directory:
```bash
cosq completion fish > ~/.config/fish/completions/cosq.fish
```

**PowerShell** — add to profile:
```powershell
cosq completion powershell >> $PROFILE
```

## Verify Installation

```bash
cosq --version
```
