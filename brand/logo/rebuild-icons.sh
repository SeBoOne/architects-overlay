#!/usr/bin/env bash
# Regeneriert alle App-/Tray-Icons aus der Master-Quelle brand/logo/logo.svg.
#
# Pipeline: logo.svg → 1024×1024 PNG (transparent, Emblem mittig ~10 % Padding)
#           → `pnpm tauri icon` (icons/*.png, icon.ico, icon.icns, StoreLogo…)
#           → Tray-Renderings (32/64/128 px, 80 % Emblem-Anteil)
#
# Nach dem Lauf: `git status src-tauri/icons` prüfen und tray.rs/Doku anpassen,
# falls der Icon-Dateiname geändert wurde. Benötigt: rsvg-convert, magick,
# pnpm (tauri-cli). Ausführen im Repo-Root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/brand/logo/logo.svg"
OUT="$ROOT/brand/logo/derived"
ICONS="$ROOT/src-tauri/icons"

mkdir -p "$OUT"

# 1) 1024×1024 Master-Icon (Emblem auf 80 % Fläche, mittig, transparent)
rsvg-convert -w 819 -h 819 --keep-aspect-ratio "$SRC" -o "$OUT/.emblem-819.png"
magick "$OUT/.emblem-819.png" -background none -gravity center \
    -extent 1024x1024 "$OUT/app-icon-1024.png"
rm -f "$OUT/.emblem-819.png"

# 2) Alle Plattform-Größen via tauri icon
(cd "$ROOT" && pnpm tauri icon "$OUT/app-icon-1024.png" -o src-tauri/icons >/dev/null)
rm -rf "$ICONS/android" "$ICONS/ios"   # Desktop-App: Mobile-Größen nicht benötigt

# 3) Tray-Icon-Größen (32 px wird in tray.rs per include_bytes! eingebettet)
for s in 32 64 128; do
    magick "$OUT/app-icon-1024.png" -resize $((s * 80 / 100))x$((s * 80 / 100)) \
        -background none -gravity center -extent "${s}x${s}" "$OUT/tray-$s.png"
    cp "$OUT/tray-$s.png" "$ICONS/tray-drake-$s.png"
done

echo "OK — Icons regeneriert:"
ls -la "$OUT/app-icon-1024.png" "$ICONS"/tray-drake-*.png
