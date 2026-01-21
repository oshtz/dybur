#!/bin/bash
# Build CLI sidecar for Tauri bundling
# This script builds the dybur CLI and copies it to the tray app's binaries folder

set -e

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CLI_DIR="$PROJECT_ROOT/apps/cli"
BINARIES_DIR="$PROJECT_ROOT/apps/tray/src-tauri/binaries"

# Detect target triple
TARGET=$(rustc -vV | grep "host:" | cut -d' ' -f2)
echo "Building for target: $TARGET"

# Build CLI in release mode
echo "Building dybur CLI..."
cd "$CLI_DIR"
cargo build --release

# Create binaries directory if it doesn't exist
mkdir -p "$BINARIES_DIR"

# Determine binary name (with .exe on Windows)
if [[ "$TARGET" == *"windows"* ]]; then
    SOURCE_BINARY="$CLI_DIR/target/release/dybur.exe"
    DEST_BINARY="$BINARIES_DIR/dybur-$TARGET.exe"
else
    SOURCE_BINARY="$CLI_DIR/target/release/dybur"
    DEST_BINARY="$BINARIES_DIR/dybur-$TARGET"
fi

echo "Copying $SOURCE_BINARY to $DEST_BINARY"
cp "$SOURCE_BINARY" "$DEST_BINARY"

echo "Sidecar built successfully!"
echo "Binary location: $DEST_BINARY"
