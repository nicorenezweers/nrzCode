#!/bin/sh
# Oberfläche (Dioxus, ui/) bauen und nach ui/dist/public legen – build.rs bettet sie ins
# Programm ein. Vor jedem Release bzw. Programm-Build ausführen (der Docker-Build macht das selbst).
# Braucht: rustup mit Ziel wasm32-unknown-unknown und die Dioxus-CLI (`dx`, Version 0.7).
# `dx bundle` räumt seinen Zwischenordner nicht auf und kopiert alles daraus mit; darum in einen
# frischen Ordner bauen und mit rsync --delete übernehmen (sonst landen alte WASM-Dateien im Programm).
set -eu
cd "$(dirname "$0")/../ui"

PATH="$HOME/.cargo/bin:$PATH"
export PATH

rm -rf target/dx/nrzcode-ui/release/web/public
out="target/ui-bundle-$(date +%s)"
dx bundle --web --release --out-dir "$out"
mkdir -p dist/public
if command -v rsync >/dev/null; then
  rsync -a --delete "$out/public/" dist/public/
else
  # Windows (Git Bash) ohne rsync: Ordner ersetzen
  rm -rf dist/public
  cp -R "$out/public" dist/public
fi
rm -rf "$out"
echo "Oberfläche gebaut: ui/dist/public ($(du -sh dist/public | cut -f1))"
