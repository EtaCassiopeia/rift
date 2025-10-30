#!/bin/bash
# Rift Local Installation Script
#
# Builds and installs Rift from local source code instead of downloading from GitHub releases.
#
# Usage:
#   ./scripts/install-local.sh           # Build in release mode and install
#   ./scripts/install-local.sh --debug   # Build in debug mode and install
#
# Options:
#   RIFT_INSTALL_DIR=/usr/local/bin - Installation directory (default: /usr/local/bin or ~/.local/bin)
#   RIFT_NO_MODIFY_PATH=1 - Don't show PATH modification hints
#   RIFT_SKIP_BUILD=1 - Skip build step (use existing binary)

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
BINARY_NAME="rift"
SOURCE_BINARY_NAME="rift-http-proxy"

# Get the script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Determine installation directory
get_install_dir() {
    if [ -n "$RIFT_INSTALL_DIR" ]; then
        echo "$RIFT_INSTALL_DIR"
    elif [ -w "/usr/local/bin" ]; then
        echo "/usr/local/bin"
    else
        mkdir -p "$HOME/.local/bin"
        echo "$HOME/.local/bin"
    fi
}

# Build Rift from source
build_rift() {
    local build_mode="$1"

    log_info "Building Rift from source..."
    cd "$PROJECT_ROOT"

    if [ "$build_mode" = "debug" ]; then
        log_info "Building in debug mode..."
        cargo build
    else
        log_info "Building in release mode (this may take a while)..."
        cargo build --release
    fi

    log_success "Build completed"
}

# Install Rift
install_rift() {
    local build_mode="$1"
    local install_dir=$(get_install_dir)
    local target_dir

    if [ "$build_mode" = "debug" ]; then
        target_dir="${PROJECT_ROOT}/target/debug"
    else
        target_dir="${PROJECT_ROOT}/target/release"
    fi

    local binary_path="${target_dir}/${SOURCE_BINARY_NAME}"

    if [ ! -f "$binary_path" ]; then
        log_error "Binary not found at ${binary_path}. Did the build succeed?"
    fi

    log_info "Installing to ${install_dir}..."

    # Check if we need sudo
    if [ -w "$install_dir" ]; then
        cp "$binary_path" "${install_dir}/${BINARY_NAME}"
        chmod +x "${install_dir}/${BINARY_NAME}"
        # Create mb symlink for Mountebank compatibility
        ln -sf "${install_dir}/${BINARY_NAME}" "${install_dir}/mb"
    else
        log_info "Requesting sudo access to install to ${install_dir}..."
        sudo cp "$binary_path" "${install_dir}/${BINARY_NAME}"
        sudo chmod +x "${install_dir}/${BINARY_NAME}"
        sudo ln -sf "${install_dir}/${BINARY_NAME}" "${install_dir}/mb"
    fi

    # Verify installation
    if command -v "${install_dir}/${BINARY_NAME}" &> /dev/null; then
        log_success "Rift installed successfully!"
        echo ""
        "${install_dir}/${BINARY_NAME}" --version || true
    else
        log_success "Rift installed to ${install_dir}/${BINARY_NAME}"
    fi

    # Check if install_dir is in PATH
    if [[ ":$PATH:" != *":${install_dir}:"* ]]; then
        log_warning "${install_dir} is not in your PATH"

        if [ -z "$RIFT_NO_MODIFY_PATH" ]; then
            echo ""
            echo "Add the following to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
            echo ""
            echo "  export PATH=\"${install_dir}:\$PATH\""
            echo ""
        fi
    fi

    echo ""
    log_info "Quick start:"
    echo "  # Start in Mountebank mode"
    echo "  rift --port 2525"
    echo ""
    echo "  # Or use the mb alias"
    echo "  mb --port 2525"
    echo ""
    echo "  # Start with a config file"
    echo "  rift --rift-config config.yaml"
}

# Uninstall
uninstall_rift() {
    local install_dir=$(get_install_dir)

    log_info "Uninstalling Rift from ${install_dir}..."

    if [ -w "$install_dir" ]; then
        rm -f "${install_dir}/${BINARY_NAME}" "${install_dir}/mb"
    else
        sudo rm -f "${install_dir}/${BINARY_NAME}" "${install_dir}/mb"
    fi

    log_success "Rift uninstalled"
}

# Show usage
usage() {
    echo "Rift Local Installation Script"
    echo ""
    echo "Usage: $0 [options] [command]"
    echo ""
    echo "Commands:"
    echo "  install       Build and install (default)"
    echo "  uninstall     Remove installed binary"
    echo ""
    echo "Options:"
    echo "  --debug       Build in debug mode (faster build, slower runtime)"
    echo "  --help, -h    Show this help message"
    echo ""
    echo "Environment variables:"
    echo "  RIFT_INSTALL_DIR    Installation directory (default: /usr/local/bin or ~/.local/bin)"
    echo "  RIFT_NO_MODIFY_PATH Skip PATH modification hints"
    echo "  RIFT_SKIP_BUILD     Skip build step (use existing binary)"
}

# Main
main() {
    local build_mode="release"
    local command="install"

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --debug)
                build_mode="debug"
                shift
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            install)
                command="install"
                shift
                ;;
            uninstall|remove)
                command="uninstall"
                shift
                ;;
            *)
                log_error "Unknown option: $1. Use --help for usage."
                ;;
        esac
    done

    case "$command" in
        install)
            if [ -z "$RIFT_SKIP_BUILD" ]; then
                build_rift "$build_mode"
            else
                log_info "Skipping build (RIFT_SKIP_BUILD=1)"
            fi
            install_rift "$build_mode"
            ;;
        uninstall)
            uninstall_rift
            ;;
    esac
}

main "$@"
