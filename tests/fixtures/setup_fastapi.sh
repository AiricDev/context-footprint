#!/bin/bash
# Setup FastAPI fixture for E2E testing.
# Clones FastAPI repository. Generate semantic_data.json with your LSP-based extractor and place in fastapi/.
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

echo "Generate semantic_data.json in $FASTAPI_DIR using your LSP-based extractor, then run the E2E test."
