#!/usr/bin/env bash

# Install script for clog
# Installs the clog tool to system or user directory

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default installation directory
DEFAULT_INSTALL_DIR="$HOME/.local/bin"
INSTALL_DIR=""

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --prefix)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --system)
            INSTALL_DIR="/usr/local/bin"
            shift
            ;;
        --help|-h)
            echo "clog installation script"
            echo ""
            echo "Usage: ./install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --prefix DIR    Install to custom directory"
            echo "  --system        Install to /usr/local/bin (requires sudo)"
            echo "  --help, -h      Show this help message"
            echo ""
            echo "Default installation directory: $DEFAULT_INSTALL_DIR"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Run './install.sh --help' for usage information"
            exit 1
            ;;
    esac
done

# Use default if no directory specified
if [ -z "$INSTALL_DIR" ]; then
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
fi

echo -e "${GREEN}Installing clog...${NC}"

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo is not installed${NC}"
    echo "Please install Rust from https://rustup.rs/"
    exit 1
fi

# Check if binary exists
if [ ! -f "target/release/clog" ]; then
    echo -e "${YELLOW}Binary not found. Building clog...${NC}"
    cargo build --release
    if [ $? -ne 0 ]; then
        echo -e "${RED}✗ Build failed${NC}"
        exit 1
    fi
fi

# Create installation directory if it doesn't exist
if [ ! -d "$INSTALL_DIR" ]; then
    echo -e "${YELLOW}Creating directory: $INSTALL_DIR${NC}"
    mkdir -p "$INSTALL_DIR"
fi

# Check if we need sudo
NEED_SUDO=false
if [ "$INSTALL_DIR" = "/usr/local/bin" ] || [ "$INSTALL_DIR" = "/usr/bin" ]; then
    NEED_SUDO=true
fi

# Install the binary
echo -e "${BLUE}Installing to: $INSTALL_DIR/clog${NC}"

if [ "$NEED_SUDO" = true ]; then
    if ! command -v sudo &> /dev/null; then
        echo -e "${RED}Error: sudo is required to install to $INSTALL_DIR${NC}"
        echo "Please run as root or choose a different directory with --prefix"
        exit 1
    fi
    sudo cp target/release/clog "$INSTALL_DIR/clog"
    sudo chmod +x "$INSTALL_DIR/clog"
else
    cp target/release/clog "$INSTALL_DIR/clog"
    chmod +x "$INSTALL_DIR/clog"
fi

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ Installation successful!${NC}"
    
    # Check if installation directory is in PATH
    if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        echo -e "${YELLOW}Warning: $INSTALL_DIR is not in your PATH${NC}"
        echo "Add the following line to your shell configuration file:"
        echo -e "${BLUE}export PATH=\"\$PATH:$INSTALL_DIR\"${NC}"
    else
        # Test the installation
        if command -v clog &> /dev/null; then
            VERSION=$(clog --version 2>&1 || echo "unknown")
            echo -e "Installed: ${GREEN}clog${NC} $VERSION"
            echo -e "Run '${BLUE}clog --help${NC}' to get started"
        fi
    fi
else
    echo -e "${RED}✗ Installation failed${NC}"
    exit 1
fi