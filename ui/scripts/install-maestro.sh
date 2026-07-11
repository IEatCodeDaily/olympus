#!/usr/bin/env bash
set -euo pipefail

VERSION="${MAESTRO_VERSION:-2.6.1}"
INSTALL_DIR="${MAESTRO_DIR:-$HOME/.maestro}"
BASE_URL="https://github.com/mobile-dev-inc/Maestro/releases/download/cli-${VERSION}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

for command in curl unzip sha256sum; do
  command -v "$command" >/dev/null || { echo "ERROR: $command is required" >&2; exit 1; }
done
command -v java >/dev/null || { echo "ERROR: Java 17+ is required" >&2; exit 1; }

curl -fsSL "$BASE_URL/maestro.zip" -o "$TMP_DIR/maestro.zip"
curl -fsSL "$BASE_URL/checksums_sha256.txt" -o "$TMP_DIR/checksums_sha256.txt"
(
  cd "$TMP_DIR"
  sha256sum --check --strict checksums_sha256.txt
  unzip -q maestro.zip
)

mkdir -p "$INSTALL_DIR"
rm -rf "$INSTALL_DIR/bin" "$INSTALL_DIR/lib"
cp -a "$TMP_DIR/maestro/bin" "$TMP_DIR/maestro/lib" "$INSTALL_DIR/"

"$INSTALL_DIR/bin/maestro" --version
