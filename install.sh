#!/usr/bin/env sh
# install.sh — install the r (Rust version manager) binary
#
# Solves the chicken-and-egg problem: you don't need Rust installed to get r.
# After installing r, use it to manage your Rust toolchains:
#   r install stable    # downloads & activates stable Rust
#   r install nightly   # downloads & activates nightly Rust
set -e

REPO="THernandez03/r"
BIN="r"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS
case "$(uname -s)" in
  Linux)  OS="linux" ;;
  Darwin) OS="darwin" ;;
  *)
    echo "Unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

# Detect architecture
case "$(uname -m)" in
  x86_64 | amd64)   ARCH="x64" ;;
  aarch64 | arm64)  ARCH="arm64" ;;
  *)
    echo "Unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

ARTIFACT="${BIN}-${OS}-${ARCH}"

# Fetch the latest release tag from GitHub
echo "Fetching latest release..."
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' \
  | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$TAG" ]; then
  echo "Failed to fetch latest release tag." >&2
  exit 1
fi

URL="https://github.com/${REPO}/releases/download/${TAG}/${ARTIFACT}"

echo "Downloading ${BIN} ${TAG} (${OS}/${ARCH})..."
mkdir -p "$INSTALL_DIR"
curl -fsSL "$URL" -o "${INSTALL_DIR}/${BIN}"
chmod +x "${INSTALL_DIR}/${BIN}"

echo ""
echo "Installed ${BIN} ${TAG} to ${INSTALL_DIR}/${BIN}"
echo ""
echo "Make sure the following are in your PATH:"
echo "  export PATH=\"\$HOME/.local/bin:\$PATH\"   # for the r binary"
echo "  export PATH=\"\$HOME/.r/bin:\$PATH\"        # for managed Rust toolchain binaries"
echo ""
echo "Then install a Rust toolchain:"
echo "  r install stable"
