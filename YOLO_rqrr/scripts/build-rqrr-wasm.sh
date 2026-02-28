#!/usr/bin/env bash
# Build the rqrr-decode WASM package for browser use.
# Requires: wasm-pack (cargo install wasm-pack)
#
# Usage: ./scripts/build-rqrr-wasm.sh

set -euo pipefail
cd "$(dirname "$0")/../rqrr-wasm"

echo "Building rqrr-decode WASM package..."
wasm-pack build --target web --release --out-dir pkg

echo ""
echo "Build complete!"
echo "  Output: rqrr-wasm/pkg/"
ls -lh pkg/*.wasm pkg/*.js 2>/dev/null || true
echo ""
echo "The worker imports from ../rqrr-wasm/pkg/rqrr_decode.js"
