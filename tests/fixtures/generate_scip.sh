#!/bin/bash
# Generate SCIP index for simple_python fixture. Requires scip-python on PATH.
set -e
cd "$(dirname "$0")/simple_python"
if command -v scip-python &>/dev/null; then
  scip-python index . --output index.scip
  echo "Generated index.scip"
else
  echo "scip-python not found; install it to generate index.scip"
  exit 1
fi
