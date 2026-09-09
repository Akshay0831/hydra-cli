#!/usr/bin/env bash
set -e

echo "=== Installing Hydra CLI ==="

# Check requirements
command -v git >/dev/null 2>&1 || { echo "Error: git is required but not installed." >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "Error: cargo (Rust) is required but not installed." >&2; exit 1; }

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_DIR"

echo "1. Initializing and updating Git submodules..."
git submodule update --init --recursive

echo "2. Building release binary..."
cargo build --release --bin hydra-cli

INSTALL_DIR="/usr/local/bin"
if [ -w "$INSTALL_DIR" ]; then
    cp "$REPO_DIR/target/release/hydra-cli" "$INSTALL_DIR/hydra-cli"
    echo "3. Installed hydra-cli to $INSTALL_DIR/hydra-cli"
else
    echo "Installing to ~/.cargo/bin..."
    mkdir -p "$HOME/.cargo/bin"
    cp "$REPO_DIR/target/release/hydra-cli" "$HOME/.cargo/bin/hydra-cli"
    echo "3. Installed hydra-cli to $HOME/.cargo/bin/hydra-cli"
fi

echo ""
echo "=== Hydra CLI successfully installed! ==="
echo "Run 'hydra-cli --help' to get started."
