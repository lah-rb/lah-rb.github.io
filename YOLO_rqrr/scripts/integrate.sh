#!/usr/bin/env bash
# Copy built artifacts to the main Kipukas site for production use.
#
# This copies:
#   1. ONNX model → assets/js-wasm/
#   2. rqrr WASM pkg → assets/js-wasm/rqrr-pkg/
#   3. JS runtime files → assets/js/
#
# Usage: ./scripts/integrate.sh

set -euo pipefail
PROJ="$(dirname "$0")/.."
SITE="$PROJ/.."

echo "Integrating YOLO+rqrr artifacts into main site..."
echo ""

# 1. ONNX model
if [ -f "$PROJ/models/yolo12n-qr.onnx" ]; then
  cp "$PROJ/models/yolo12n-qr.onnx" "$SITE/assets/js-wasm/"
  echo "✅ Copied yolo12n-qr.onnx → assets/js-wasm/"
else
  echo "⚠️  models/yolo12n-qr.onnx not found (run train + export first)"
fi

# 2. rqrr WASM package
if [ -d "$PROJ/rqrr-wasm/pkg" ]; then
  mkdir -p "$SITE/assets/js-wasm/rqrr-pkg"
  cp "$PROJ/rqrr-wasm/pkg/rqrr_decode.js" "$SITE/assets/js-wasm/rqrr-pkg/"
  cp "$PROJ/rqrr-wasm/pkg/rqrr_decode_bg.wasm" "$SITE/assets/js-wasm/rqrr-pkg/"
  echo "✅ Copied rqrr WASM pkg → assets/js-wasm/rqrr-pkg/"
else
  echo "⚠️  rqrr-wasm/pkg not found (run build-rqrr-wasm.sh first)"
fi

# 3. JS runtime files
if [ -d "$PROJ/web/src" ]; then
  cp "$PROJ/web/src/yolo-inference.js" "$SITE/assets/js/"
  cp "$PROJ/web/src/postprocess.js" "$SITE/assets/js/"
  cp "$PROJ/web/src/yolo-rqrr-worker.js" "$SITE/assets/js/"
  echo "✅ Copied JS runtime → assets/js/"
else
  echo "⚠️  web/src/ not found"
fi

echo ""
echo "Integration complete!"
echo ""
echo "Next steps:"
echo "  1. Update kipukas-worker.js or qr-camera.js to use the new worker"
echo "  2. Add yolo12n-qr.onnx to Service Worker precache list"
echo "  3. Test with: npx serve _site -l 4000"
