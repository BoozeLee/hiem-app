#!/usr/bin/env bash
# post-build: patch package .desktop files with NVIDIA Wayland env fix
set -euo pipefail

find target/release/bundle -name "HIEM.desktop" 2>/dev/null | while read -r f; do
  sed -i 's|^Exec=hiem-app$|Exec=env __NV_DISABLE_EXPLICIT_SYNC=1 hiem-app|g' "$f"
  echo "patched: $f"
done
