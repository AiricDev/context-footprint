#!/bin/bash
# Setup FastAPI fixture for E2E testing.
# Clones FastAPI repository and generates SCIP index.
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FASTAPI_DIR="$SCRIPT_DIR/fastapi"

echo "Setting up FastAPI fixture..."

# Clone FastAPI if not already present
if [ ! -d "$FASTAPI_DIR" ]; then
    echo "Cloning FastAPI repository..."
    git clone --depth 1 --branch 0.104.1 https://github.com/tiangolo/fastapi.git "$FASTAPI_DIR"
    echo "FastAPI cloned to $FASTAPI_DIR"
else
    echo "FastAPI directory already exists at $FASTAPI_DIR"
fi

# Check if scip-python is available
if ! command -v scip-python &>/dev/null; then
    echo "ERROR: scip-python not found in PATH"
    echo "Install it with: pip install scip-python"
    exit 1
fi

# Generate SCIP index
echo "Generating SCIP index for FastAPI..."
cd "$FASTAPI_DIR"

# Index the main fastapi package
scip-python index . --project-name fastapi --output index.scip

echo "SCIP index generated at $FASTAPI_DIR/index.scip"
echo "E2E test test_fastapi_project is now ready to run"
