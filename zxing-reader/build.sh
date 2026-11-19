#!/bin/sh
# Builds the scanner's QR reader from a release of zxing-cpp.
#
#   sh zxing-reader/build.sh v3.2.0-rc1
#
# Needs git and Docker. The compiler is Emscripten, run from its official
# container image at a fixed version, so nothing is installed on this
# machine and a build made later is made with the same tools.
#
# The source is the release's tag at zxing-cpp's repository. Each release
# known here is held to the commit it was at when it was first built: a tag
# that has moved since is refused.
#
# Leaves zxing-reader/.work/build-<release>/reader.wasm, and copies it to
# assets/js-wasm/zxing-<release without the v>.wasm.
#
# READER_EXTRA_FLAGS adds compiler flags (e.g. -msimd128), for trying things.
set -eu

RELEASE=${1:?which release of zxing-cpp, e.g. v3.2.0-rc1}
EMSDK_IMAGE=emscripten/emsdk:6.0.13
REPOSITORY=https://github.com/zxing-cpp/zxing-cpp

case "$RELEASE" in
  v3.1.1) COMMIT=287c85df6f961c8efbfb5ffd736cd9457b8b890e ;;
  v3.2.0-rc1) COMMIT=e17ae29906c9868fe8400b50b847cb7f01ebff3a ;;
  *)
    echo "No commit is on record for $RELEASE. Add one to zxing-reader/build.sh." >&2
    exit 1
    ;;
esac

HERE=$(cd "$(dirname "$0")" && pwd)
WORK=${READER_WORK:-$HERE/.work}
SOURCE=$WORK/src-$RELEASE
BUILD=$WORK/build-$RELEASE${READER_NAME:+-$READER_NAME}
OUT=$HERE/../assets/js-wasm/zxing-${RELEASE#v}${READER_NAME:+-$READER_NAME}.wasm

mkdir -p "$WORK"
if [ ! -d "$SOURCE" ]; then
  git clone --quiet --depth 1 --branch "$RELEASE" "$REPOSITORY" "$SOURCE"
fi
AT=$(git -C "$SOURCE" rev-parse HEAD)
if [ "$AT" != "$COMMIT" ]; then
  echo "$RELEASE is at $AT, not at $COMMIT as recorded. Not built." >&2
  exit 1
fi

docker run --rm \
  -v "$HERE":/reader:ro \
  -v "$SOURCE":/zxing-cpp:ro \
  -v "$WORK":/work \
  "$EMSDK_IMAGE" sh -c "
    set -e
    emcmake cmake -S /reader -B '/work/$(basename "$BUILD")' \
      -DZXING_SOURCE=/zxing-cpp -DREADER_EXTRA_FLAGS='${READER_EXTRA_FLAGS:-}' > '/work/$(basename "$BUILD").configure.log' 2>&1 \
      || { cat '/work/$(basename "$BUILD").configure.log'; exit 1; }
    cmake --build '/work/$(basename "$BUILD")' -j 8 > '/work/$(basename "$BUILD").build.log' 2>&1 \
      || { tail -40 '/work/$(basename "$BUILD").build.log'; exit 1; }
  "

cp "$BUILD/reader.wasm" "$OUT"
echo "$RELEASE ($COMMIT) -> $OUT ($(wc -c < "$OUT" | tr -d ' ') bytes)"
