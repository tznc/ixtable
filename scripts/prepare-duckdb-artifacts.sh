#!/usr/bin/env bash
set -euo pipefail

# duckdb-rs 1.10505.0 binds DuckDB 1.5.5. Extensions are ABI-specific, so
# these versions must move together. Runtime installation/autoload is disabled.
#
# The app links the official prebuilt libduckdb (DUCKDB_LIB_DIR in
# .cargo/config.toml points at $DEST/lib) instead of compiling DuckDB from source.
#
# Each platform dir gets the official compressed `<ext>.duckdb_extension.gz`
# (the only file the app bundles; the app unpacks it into the per-user state
# dir on first use, see src-tauri/src/data/extensions.rs) and an uncompressed
# copy that dev builds and tests load directly.
VERSION=1.5.5
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/src-tauri/resources/duckdb"

fetch() {
  platform="$1"; target="$2"; expected="$3"; extension="${4:-sqlite_scanner}"
  url="https://extensions.duckdb.org/v${VERSION}/${platform}/${extension}.duckdb_extension.gz"
  tmp="$(mktemp)"; trap 'rm -f "$tmp"' RETURN
  curl --fail --location --proto '=https' --tlsv1.2 "$url" -o "$tmp"
  # shasum ships with macOS/Perl; Git Bash on Windows and minimal Linux have sha256sum.
  if command -v shasum >/dev/null 2>&1; then sha=(shasum -a 256); else sha=(sha256sum); fi
  printf '%s  %s\n' "$expected" "$tmp" | "${sha[@]}" -c -
  mkdir -p "$DEST/$target"
  # mktemp files are 0600; installed resources must be readable by every user.
  cp "$tmp" "$DEST/$target/${extension}.duckdb_extension.gz"
  chmod 644 "$DEST/$target/${extension}.duckdb_extension.gz"
  gzip -dc "$tmp" > "$DEST/$target/${extension}.duckdb_extension"
  printf '%s\n' "$VERSION" > "$DEST/$target/VERSION"
}

fetch_lib() {
  archive="$1"; lib="$2"; expected="$3"
  url="https://github.com/duckdb/duckdb/releases/download/v${VERSION}/${archive}"
  tmp="$(mktemp)"; trap 'rm -f "$tmp"' RETURN
  curl --fail --location --proto '=https' --tlsv1.2 "$url" -o "$tmp"
  if command -v shasum >/dev/null 2>&1; then sha=(shasum -a 256); else sha=(sha256sum); fi
  printf '%s  %s\n' "$expected" "$tmp" | "${sha[@]}" -c -
  rm -rf "$DEST/lib"; mkdir -p "$DEST/lib"
  # Only the shared library and C header; the static archive and C++ header are unused.
  unzip -o -q "$tmp" "$lib" duckdb.h -d "$DEST/lib"
  # Windows links against the import library next to the DLL.
  if [ "$lib" = duckdb.dll ]; then unzip -o -q "$tmp" duckdb.lib -d "$DEST/lib"; fi
  chmod 644 "$DEST/lib/"*
  printf '%s\n' "$VERSION" > "$DEST/lib/VERSION"
}

case "${1:-}" in
  macos-universal)
    fetch osx_arm64 macos-arm64 d7514249b0cce24bb63856b4c752a889ef2f739c6fd821109988e4e13afd7058
    fetch osx_amd64 macos-x64 1b96e4ac03a4394708166f75236614a80fd1f9ab810fb3f35ea7aa5a9a833501
    fetch osx_arm64 macos-arm64 4fb5079e67b00e6643e6ee91545a355010004d1dad50b43f1c060de0cb789c8e postgres_scanner
    fetch osx_amd64 macos-x64 b8764ed496be635fbac3e5e6a6c8e3e3c2dbfcaa862ce0422c21cad6fcc6c353 postgres_scanner
    fetch_lib libduckdb-osx-universal.zip libduckdb.dylib 7b5b8915cc382d0708636fe6385c0cdad5a61c9ff8ba2638b3e2141640783155
    ;;
  windows-x64)
    fetch windows_amd64 windows-x64 b6139c7f3b40a1b3ba5ef605e4590eda4a55e4e8deefc8182a2644e4a5797f69
    fetch windows_amd64 windows-x64 65b31f002c70ac5f812d293b8552b977d1c0e6752d0a1127176454c8184ed001 postgres_scanner
    fetch_lib libduckdb-windows-amd64.zip duckdb.dll 8375eb1fcf2212e8a0817950354815d4dde9dd383c2d9fa7b8975b71e278c1bd
    ;;
  linux-x64)
    fetch linux_amd64 linux-x64 01292812092200c2d0b76324df9568d336ddaa5a198e7cc8fed124e84088e14e
    fetch linux_amd64 linux-x64 e0f631a5535f165468bc8a20501f8bc1490adbc877d38fcdff2f8d05531e1e5b postgres_scanner
    fetch_lib libduckdb-linux-amd64.zip libduckdb.so 1fb8ce388157d84a25abe685a8a2520bf00c00321821968e4bb398fd766e7abb
    ;;
  *) echo "usage: $0 {macos-universal|windows-x64|linux-x64}" >&2; exit 2 ;;
esac
