#!/bin/sh
# nrzCode als native macOS-App bauen: Oberfläche (scripts/ui.sh), Programm (macos/, Rust),
# Bundle `nrzCode.app` und DMG nach dist-macos/.
#
#   scripts/macos.sh              # für diese Architektur (arm64 oder x86_64)
#   scripts/macos.sh universal    # arm64 + x86_64 in einem Programm (braucht beide Rust-Ziele)
#   SKIP_UI=1 scripts/macos.sh    # Oberfläche nicht neu bauen
#
# Ergebnis: dist-macos/nrzCode.app, dist-macos/nrzCode-<version>-<arch>.dmg, dist-macos/SHA256SUMS
# Veröffentlichen: scripts/macos-release.sh
set -eu
cd "$(dirname "$0")/.."
root=$(pwd)
PATH="$HOME/.cargo/bin:$PATH"
export PATH

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app_version=$(sed -n 's/^version = "\(.*\)"/\1/p' macos/Cargo.toml | head -1)
if [ "$version" != "$app_version" ]; then
  echo "Version in Cargo.toml ($version) und macos/Cargo.toml ($app_version) ist verschieden" >&2
  exit 1
fi

if [ "${SKIP_UI:-}" != "1" ] || [ ! -f ui/dist/public/index.html ]; then
  scripts/ui.sh
fi

mode=${1:-native}
cd macos
case "$mode" in
  universal)
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    cargo build --release --locked --target aarch64-apple-darwin
    cargo build --release --locked --target x86_64-apple-darwin
    mkdir -p target/universal
    lipo -create -output target/universal/nrzcode-macos \
      target/aarch64-apple-darwin/release/nrzcode-macos target/x86_64-apple-darwin/release/nrzcode-macos
    bin=target/universal/nrzcode-macos
    arch=universal
    ;;
  *)
    cargo build --release --locked
    bin=target/release/nrzcode-macos
    case "$(uname -m)" in arm64) arch=arm64 ;; *) arch=x86_64 ;; esac
    ;;
esac
cd "$root"

out=dist-macos
app="$out/nrzCode.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "macos/$bin" "$app/Contents/MacOS/nrzcode-macos"
sed "s/__VERSION__/$version/g" macos/resources/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Symbol aus static/icon.png
iconset=$(mktemp -d)/nrzCode.iconset
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
  sips -z $s $s static/icon.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d static/icon.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/nrzCode.icns"
rm -rf "$(dirname "$iconset")"

# Signieren. Mit SIGN_IDENTITY="Developer ID Application: …" richtig (Apple-Entwicklerkonto).
# Sonst mit einem eigenen, immer gleichen Zertifikat „nrzCode Local Signing“: macOS merkt sich
# erteilte Rechte (Ordner, Mikrofon, Automation …) nach der Signatur – bei ad-hoc-Signatur ändert
# sie sich mit jedem Build, und nach jedem Update fehlen die Rechte wieder („Operation not
# permitted“). Das Zertifikat entsteht beim ersten Mal in einem eigenen Schlüsselbund unter
# NRZCODE_SIGN_DIR (Vorgabe: ~/Library/Application Support/nrzCode-signing, nie im Repository).
# SIGN_IDENTITY=- signiert wie früher ad hoc.
local_identity() {
  name="nrzCode Local Signing"
  real_home=$(eval echo "~$(id -un)")
  dir="${NRZCODE_SIGN_DIR:-$real_home/Library/Application Support/nrzCode-signing}"
  kc="$dir/signing.keychain-db"
  pwfile="$dir/password"
  command -v security >/dev/null && command -v openssl >/dev/null || return 1
  if [ ! -f "$kc" ] || [ ! -f "$pwfile" ]; then
    mkdir -p "$dir" && chmod 700 "$dir" || return 1
    rm -f "$kc"
    pw=$(openssl rand -hex 24) || return 1
    tmp=$(mktemp -d)
    cat > "$tmp/cert.cnf" <<'CNF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = nrzCode Local Signing
O = nrzStudio
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
CNF
    openssl req -x509 -newkey rsa:2048 -sha256 -days 7300 -nodes -keyout "$tmp/key.pem" -out "$tmp/cert.pem" -config "$tmp/cert.cnf" 2>/dev/null || { rm -rf "$tmp"; return 1; }
    # OpenSSL 3+: altes PKCS#12-Format, das `security import` lesen kann (LibreSSL kennt -legacy nicht)
    openssl pkcs12 -export -legacy -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -name "$name" -out "$tmp/id.p12" -passout "pass:$pw" 2>/dev/null \
      || openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -name "$name" -out "$tmp/id.p12" -passout "pass:$pw" 2>/dev/null \
      || { rm -rf "$tmp"; return 1; }
    # create-keychain trägt den Schlüsselbund in die Suchliste ein – danach wie vorher (er wird nur
    # über --keychain benutzt, sonst fragte macOS nach einem Neustart nach seinem Passwort)
    before=$(security list-keychains -d user)
    security create-keychain -p "$pw" "$kc" >/dev/null 2>&1 || { rm -rf "$tmp"; return 1; }
    eval "security list-keychains -d user -s $before" >/dev/null 2>&1 || true
    security set-keychain-settings "$kc"
    security unlock-keychain -p "$pw" "$kc"
    security import "$tmp/id.p12" -k "$kc" -P "$pw" -T /usr/bin/codesign >/dev/null || { rm -rf "$tmp"; rm -f "$kc"; return 1; }
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$pw" "$kc" >/dev/null 2>&1 || true
    rm -rf "$tmp"
    printf '%s' "$pw" > "$pwfile"
    chmod 600 "$pwfile"
    echo "Neues Signatur-Zertifikat „${name}“ in ${dir} angelegt (aufheben – sonst vergisst macOS die Rechte beim nächsten Update)."
  fi
  security unlock-keychain -p "$(cat "$pwfile")" "$kc" || return 1
  sign_keychain="$kc"
  sign_name="$name"
}

if [ -n "${SIGN_IDENTITY:-}" ] && [ "$SIGN_IDENTITY" != "-" ]; then
  codesign --force --deep --options runtime --timestamp --entitlements macos/resources/nrzCode.entitlements --sign "$SIGN_IDENTITY" "$app"
elif [ "${SIGN_IDENTITY:-}" != "-" ] && local_identity; then
  codesign --force --deep --entitlements macos/resources/nrzCode.entitlements --keychain "$sign_keychain" --sign "$sign_name" "$app"
else
  [ "${SIGN_IDENTITY:-}" = "-" ] || echo "Warnung: kein eigenes Zertifikat – ad hoc signiert (macOS fragt nach Updates erneut nach Rechten)." >&2
  codesign --force --deep --sign - "$app"
fi
codesign --verify --deep --strict "$app"
codesign -d -r- "$app" 2>&1 | grep designated || true

# DMG: App plus Verknüpfung auf /Programme zum Hineinziehen
dmg="$out/nrzCode-$version-$arch.dmg"
stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname "nrzCode $version" -srcfolder "$stage" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$dmg" >/dev/null
rm -rf "$stage"

(cd "$out" && shasum -a 256 "$(basename "$dmg")" > SHA256SUMS)
echo "Fertig: $app"
echo "        $dmg ($(du -h "$dmg" | cut -f1))"
