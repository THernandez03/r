#!/usr/bin/env sh
# install.sh — install the r (Rust version manager) binary
#
# Solves the chicken-and-egg problem: you don't need Rust installed to get r.
# After installing r, use it to manage your Rust toolchains:
#   r stable    # downloads & activates stable Rust
#   r nightly   # downloads & activates nightly Rust
set -e

RED='\033[1;31m'
GREEN='\033[1;32m'
CYAN='\033[1;36m'
NC='\033[0m'

REPO="THernandez03/r"
BIN="r"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS
case "$(uname -s)" in
  Linux)  OS="linux" ;;
  Darwin) OS="darwin" ;;
  *)
    printf "${RED}Error: Unsupported OS: $(uname -s)${NC}\n" >&2
    exit 1
    ;;
esac

# Detect architecture
case "$(uname -m)" in
  x86_64 | amd64)   ARCH="x64" ;;
  aarch64 | arm64)  ARCH="arm64" ;;
  *)
    printf "${RED}Error: Unsupported architecture: $(uname -m)${NC}\n" >&2
    exit 1
    ;;
esac

ARTIFACT="${BIN}-${OS}-${ARCH}"

# Fetch the latest release tag from GitHub
printf "${CYAN}Fetching latest release...${NC}\n"
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' \
  | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$TAG" ]; then
  printf "${RED}Error: Failed to fetch latest release tag.${NC}\n" >&2
  exit 1
fi

URL="https://github.com/${REPO}/releases/download/${TAG}/${ARTIFACT}"

printf "${CYAN}Downloading ${BIN} ${TAG} (${OS}/${ARCH})...${NC}\n"
mkdir -p "$INSTALL_DIR"
curl -fsSL "$URL" -o "${INSTALL_DIR}/${BIN}"
chmod +x "${INSTALL_DIR}/${BIN}"

printf "\n${GREEN}✓ Installed ${BIN} ${TAG} to ${INSTALL_DIR}/${BIN}${NC}\n\n"
printf "Make sure the following are in your PATH:\n"
printf "  ${CYAN}export PATH=\"\$HOME/.local/bin:\$PATH\"${NC}   # for the r binary\n"
printf "  ${CYAN}export PATH=\"\$HOME/.r/bin:\$PATH\"${NC}        # for managed Rust toolchain binaries\n\n"
printf "zsh users: add ${CYAN}disable r${NC} after your PATH exports (zsh has a built-in 'r' alias).\n\n"
printf "Then install a Rust toolchain:\n"
printf "  ${CYAN}r stable${NC}\n"
