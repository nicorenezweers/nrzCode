#!/bin/sh
# nrzCodeMobile bauen: Oberfläche (Dioxus, mobile/ui → mobile/ui/dist/public) und Server
# (mobile/, Oberfläche eingebettet).
#
#   scripts/mobile.sh            # Server für diesen Rechner (mobile/target/release/nrzcode-mobile)
#   scripts/mobile.sh linux      # statische Linux-Programme für Umbrel (amd64, arm64) nach mobile/dist/
#   SKIP_UI=1 scripts/mobile.sh  # Oberfläche nicht neu bauen
#
# Für `linux`: rustup-Ziele x86_64-unknown-linux-musl und aarch64-unknown-linux-musl, zig und
# cargo-zigbuild. Veröffentlichen für Umbrel: scripts/mobile-umbrel.sh.
set -eu
cd "$(dirname "$0")/.."
PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH:/opt/homebrew/bin"
export PATH

if [ "${SKIP_UI:-}" != "1" ] || [ ! -f mobile/ui/dist/public/index.html ]; then
  (
    cd mobile/ui
    rm -rf target/dx/nrzcode-mobile-ui/release/web/public
    out="target/ui-bundle-$(date +%s)"
    dx bundle --web --release --out-dir "$out"
    mkdir -p dist/public
    rsync -a --delete "$out/public/" dist/public/
    rm -rf "$out"
  )
  echo "Oberfläche gebaut: mobile/ui/dist/public ($(du -sh mobile/ui/dist/public | cut -f1))"
fi

cd mobile
if [ "${1:-}" = "linux" ]; then
  mkdir -p dist
  for pair in amd64:x86_64-unknown-linux-musl arm64:aarch64-unknown-linux-musl; do
    arch=${pair%%:*}
    target=${pair#*:}
    cargo zigbuild --release --target "$target"
    gzip -9 -c "target/$target/release/nrzcode-mobile" > "dist/nrzcode-mobile-$arch.gz"
  done
  (cd dist && shasum -a 256 nrzcode-mobile-amd64.gz nrzcode-mobile-arm64.gz > SHA256SUMS)
  echo "Fertig: mobile/dist/ ($(ls dist | tr '\n' ' '))"
else
  cargo build --release
  echo "Fertig: mobile/target/release/nrzcode-mobile"
fi
