#!/usr/bin/env bash
set -euo pipefail

echo "[INFO] Checking and installing benchmark prerequisites..."
echo ""

# Detect macOS package manager
HAS_BREW=0
if command -v brew >/dev/null 2>&1; then
  HAS_BREW=1
fi

# Rust / Cargo
if ! command -v cargo >/dev/null 2>&1; then
  echo "[WARN] cargo not found."
  if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "[INFO] Installing Rust toolchain via rustup (non-interactive)..."
    curl https://sh.rustup.rs -sSf | sh -s -- -y
    source "$HOME/.cargo/env"
  else
    echo "[ERROR] Please install Rust/Cargo (https://rustup.rs/) and re-run."
    exit 1
  fi
else
  echo "[OK] cargo found: $(cargo --version)"
fi

# oha (HTTP load testing tool)
if ! command -v oha >/dev/null 2>&1; then
  echo "[INFO] Installing oha via cargo..."
  cargo install oha
else
  echo "[OK] oha found: $(oha --version)"
fi

# jq (JSON processor)
if ! command -v jq >/dev/null 2>&1; then
  echo "[WARN] jq not found."
  if [[ $HAS_BREW -eq 1 ]]; then
    echo "[INFO] Installing jq via Homebrew..."
    brew install jq
  else
    echo "[ERROR] Please install jq (https://stedolan.github.io/jq/) and re-run."
    exit 1
  fi
else
  echo "[OK] jq found: $(jq --version)"
fi

# docker
if ! command -v docker >/dev/null 2>&1; then
  echo "[ERROR] Docker not found. Please install Docker Desktop and re-run."
  exit 1
else
  echo "[OK] docker found: $(docker --version)"
fi

# docker compose (v2) or docker-compose (v1)
if docker compose version >/dev/null 2>&1; then
  echo "[OK] docker compose (v2) available: $(docker compose version --short)"
elif command -v docker-compose >/dev/null 2>&1; then
  echo "[OK] docker-compose (v1) available: $(docker-compose --version)"
else
  echo "[ERROR] docker compose not found. Install Docker Desktop or docker-compose plugin."
  exit 1
fi

# curl (should be available on macOS by default)
if ! command -v curl >/dev/null 2>&1; then
  echo "[ERROR] curl not found. This is required for health checks."
  exit 1
else
  echo "[OK] curl found: $(curl --version | head -n1)"
fi

echo ""
echo "[SUCCESS] All benchmark tools are installed and ready!"
echo ""
echo "Next steps:"
echo "  1. Ensure you have the mimeo-solo-local-image Docker image built"
echo "  2. Run: ./benchmarks/run-mimeo-comparison.sh"
