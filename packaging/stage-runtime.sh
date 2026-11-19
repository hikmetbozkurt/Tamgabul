#!/usr/bin/env bash
set -euo pipefail

# Stage local llama-server binaries and compute SHA256SUMS.txt + runtime-manifest.json
# for offline macOS Tauri packaging. Never downloads binaries from the internet.

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "Usage: $0 <source-dir> [out-dir]" >&2
  exit 1
fi

SOURCE_DIR="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="${2:-$SCRIPT_DIR/staged/llama}"

if [ ! -d "$SOURCE_DIR" ]; then
  echo "Error: source directory does not exist: $SOURCE_DIR" >&2
  exit 1
fi

SERVER_BIN="$SOURCE_DIR/llama-server"
if [ ! -x "$SERVER_BIN" ]; then
  echo "Error: executable llama-server not found in: $SOURCE_DIR" >&2
  exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cp -p "$SERVER_BIN" "$OUT_DIR/llama-server"
shopt -s nullglob
for f in "$SOURCE_DIR"/*.dylib "$SOURCE_DIR"/*.metal "$SOURCE_DIR"/*.metallib "$SOURCE_DIR"/LICENSE* "$SOURCE_DIR"/NOTICE; do
  [ -f "$f" ] && cp -p "$f" "$OUT_DIR/"
done
shopt -u nullglob

VERSION_OUT="$("$OUT_DIR/llama-server" --version 2>&1 || true)"
VERSION_LINE="$(printf '%s\n' "$VERSION_OUT" | sed -n 's/^[[:space:]]*version:[[:space:]]*//p' | head -n 1)"
VERSION="${VERSION_LINE:-unknown}"

SUMS_FILE="$OUT_DIR/SHA256SUMS.txt"
MANIFEST_FILE="$OUT_DIR/runtime-manifest.json"

OS_NAME="$(uname -s | tr '[:upper:]' '[:lower:]')"
[ "$OS_NAME" = "darwin" ] && OS_NAME="macos"
ARCH_NAME="$(uname -m)"
[ "$ARCH_NAME" = "arm64" ] && ARCH_NAME="aarch64"

python3 - "$OUT_DIR" "$SUMS_FILE" "$MANIFEST_FILE" "$OS_NAME" "$ARCH_NAME" "$VERSION" <<'PYEOF'
import hashlib, json, pathlib, sys

out_dir = pathlib.Path(sys.argv[1])
sums_file = pathlib.Path(sys.argv[2])
manifest_file = pathlib.Path(sys.argv[3])
os_name, arch_name, version = sys.argv[4], sys.argv[5], sys.argv[6]

files = []
sum_lines = []
for p in sorted(out_dir.iterdir(), key=lambda x: x.name):
    if not p.is_file() or p.name in ("SHA256SUMS.txt", "runtime-manifest.json"):
        continue
    h = hashlib.sha256(p.read_bytes()).hexdigest()
    sum_lines.append(f"{h}  {p.name}")
    files.append({"name": p.name, "bytes": p.stat().st_size, "sha256": h})

sums_file.write_text("\n".join(sum_lines) + "\n", encoding="utf-8")
manifest = {
    "runtime": "llama.cpp",
    "license": "MIT",
    "os": os_name,
    "arch": arch_name,
    "version": version,
    "files": files,
}
manifest_file.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
print(f"Staged {len(files)} file(s) to {out_dir} (llama-server version: {version})")
PYEOF
