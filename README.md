# r — Interactively Manage Your Rust Versions

`r` is a simple, no-fuss Rust version manager. Download, cache, and switch between Rust toolchain versions with a single command. No need to have Rust pre-installed — grab a pre-built binary and bootstrap from there.

## Features

- Install any released Rust toolchain by version, alias, or `nightly`/`beta`
- Interactive version picker (arrow keys)
- Version caching — no re-downloading
- Symlink-based activation — exposes `rustc`, `cargo`, `rustfmt`, `clippy`, `rustdoc`, and the full toolchain
- List local and remote versions
- Run a specific version without activating it

## Supported Platforms

| OS      | Architectures   |
| ------- | --------------- |
| Linux   | x86_64, aarch64 |
| macOS   | x86_64, aarch64 |
| Windows | x86_64, aarch64 |

## Installation

### Pre-built binary (no Rust required)

```bash
curl -fsSL https://raw.githubusercontent.com/THernandez03/r/main/install.sh | sh
```

This installs `r` to `~/.local/bin/r`. You can override the destination:

```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/THernandez03/r/main/install.sh | sh
```

### From source (requires Rust)

```bash
cargo install --git https://github.com/THernandez03/r
```

### Manual

Download the latest binary from [Releases](https://github.com/THernandez03/r/releases) and place it in your `PATH`.

## Setup

Add both directories to your `PATH`:

```bash
# bash / zsh
export R_PREFIX="$HOME/.r"
export PATH="$HOME/.local/bin:$PATH"   # for the r binary
export PATH="$R_PREFIX/bin:$PATH"      # for managed Rust toolchain binaries
```

Optional environment variables:

| Variable      | Default         | Description                            |
| ------------- | --------------- | -------------------------------------- |
| `R_PREFIX`    | `~/.r`          | Root installation prefix               |
| `R_CACHE_DIR` | `~/.r/versions` | Where downloaded toolchains are stored |

## Usage

```bash
# Install and activate a version
r 1.87.0
r stable
r nightly
r beta

# Interactive picker from cached versions
r

# List cached versions
r ls

# List recent remote versions
r ls-remote

# Fetch into cache without activating
r fetch 1.87.0

# Show path to the cached rustc binary
r which stable

# Run a specific version's rustc
r run 1.86.0 -- --version

# Remove a cached version (interactive picker if no version given)
r remove 1.86.0
r rm 1.86.0         # alias

# Remove all except active
r prune

# Show info
r info

# Update r itself
r update

# Fully remove r + all cached versions (requires confirmation)
r uninstall
```

## Version Aliases

| Alias     | Resolves to                       |
| --------- | --------------------------------- |
| `stable`  | Latest stable release             |
| `latest`  | Same as `stable`                  |
| `lts`     | Same as `stable`                  |
| `beta`    | Current beta toolchain            |
| `nightly` | Latest nightly build              |
| `canary`  | Same as `nightly`                 |
| `next`    | Same as `nightly`                 |
| `edge`    | Same as `nightly`                 |
| `1.87`    | Latest patch in 1.87.x            |
| `v1.87.0` | Exact version (no network needed) |

## Related Projects

| Project                                | Runtime                 |
| -------------------------------------- | ----------------------- |
| [n](https://github.com/THernandez03/n) | Node.js version manager |
| [b](https://github.com/THernandez03/b) | Bun version manager     |
| [z](https://github.com/THernandez03/z) | Zig version manager     |
| [d](https://github.com/THernandez03/d) | Deno version manager    |

## License

MIT
