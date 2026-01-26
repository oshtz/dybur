#!/bin/bash
# Build CLI for Tauri bundling
# This script builds the TypeScript CLI and copies it to the tray app's resources folder
# The CLI requires Node.js to be installed on the user's system

set -e

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CLI_DIR="$PROJECT_ROOT/packages/cli"
RESOURCES_DIR="$PROJECT_ROOT/apps/tray/src-tauri/resources"

echo "Building CLI (Node.js required at runtime)..."

# Build TypeScript packages first
echo "Building TypeScript packages..."
cd "$PROJECT_ROOT"
pnpm build

# Create resources directory if it doesn't exist
mkdir -p "$RESOURCES_DIR"

# Copy the bundled CLI JS to resources
SOURCE_JS="$CLI_DIR/dist/cli.js"
DEST_JS="$RESOURCES_DIR/cli.js"

if [ ! -f "$SOURCE_JS" ]; then
    echo "Error: $SOURCE_JS not found. Run 'pnpm build' first."
    exit 1
fi

echo "Copying $SOURCE_JS to $DEST_JS"
cp "$SOURCE_JS" "$DEST_JS"

echo "CLI built successfully!"
echo "Location: $DEST_JS"
echo "Size: $(du -h "$DEST_JS" | cut -f1)"
echo ""
echo "Note: Users need Node.js installed to use the CLI."
