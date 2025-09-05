#!/usr/bin/env bash

# Build script for clog
# Builds the clog tool in release mode with optimizations

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Building clog...${NC}"

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo is not installed${NC}"
    echo "Please install Rust from https://rustup.rs/"
    exit 1
fi

# Clean previous builds
if [ "$1" = "--clean" ]; then
    echo -e "${YELLOW}Cleaning previous builds...${NC}"
    cargo clean
fi

# Build in release mode with optimizations
cargo build --release

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ Build successful!${NC}"
    echo -e "Binary location: ${YELLOW}target/release/clog${NC}"
    
    # Show binary size
    SIZE=$(ls -lh target/release/clog | awk '{print $5}')
    echo -e "Binary size: ${YELLOW}${SIZE}${NC}"
else
    echo -e "${RED}✗ Build failed${NC}"
    exit 1
fi