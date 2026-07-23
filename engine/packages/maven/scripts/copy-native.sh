#!/usr/bin/env bash
# Copy host lemma_jni shared library into Maven resources for tests/packaging.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
PKG="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
TRIPLE="$(rustc -vV | awk '/^host:/{print $2}')"
case "$(uname -s)" in
  Darwin) LIB="liblemma_jni.dylib" ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT) LIB="lemma_jni.dll" ;;
  *) LIB="liblemma_jni.so" ;;
esac
SRC=""
for profile in debug release; do
  candidate="$TARGET_DIR/$profile/$LIB"
  if [[ -f "$candidate" ]]; then
    SRC="$candidate"
    break
  fi
done
if [[ -z "$SRC" ]]; then
  echo "copy-native: building lemma_jni (no $LIB under $TARGET_DIR/{debug,release})" >&2
  (cd "$ROOT" && cargo build -p lemma_jni)
  SRC="$TARGET_DIR/debug/$LIB"
fi
if [[ ! -f "$SRC" ]]; then
  echo "copy-native: missing $SRC" >&2
  exit 1
fi
DEST_DIR="$PKG/src/main/resources/native/$TRIPLE"
mkdir -p "$DEST_DIR"
cp -f "$SRC" "$DEST_DIR/$LIB"
echo "copy-native: $SRC -> $DEST_DIR/$LIB"
