#!/bin/sh
# nrzCode als Windows-App (x64) bauen – Querbau von macOS/Linux mit mingw-w64:
#   brew install mingw-w64   (bzw. apt install mingw-w64)
#   rustup target add x86_64-pc-windows-gnu
#
#   scripts/windows.sh            # Oberfläche + Programm
#   SKIP_UI=1 scripts/windows.sh  # Oberfläche nicht neu bauen
#
# Unter Windows selbst geht es auch (Git Bash): mingw-w64 (z. B. WinLibs, Variante msvcrt) und
# NSIS in den PATH legen, `rustup target add x86_64-pc-windows-gnu`, Dioxus-CLI `dx` 0.7.
#
# Ergebnis in dist-windows/:
#   nrzCode-<version>-Setup.exe  Installer (Startmenü, Deinstallieren; braucht makensis: brew install makensis)
#   nrzCode-<version>-x64.zip    ohne Installer (nrzCode.exe, entpacken und starten)
#   nrzCode-<version>-x64.exe    für die automatischen Updates der installierten App
#   SHA256SUMS
# Veröffentlichen: PLATFORM=windows scripts/macos-release.sh
set -eu
cd "$(dirname "$0")/.."
root=$(pwd)
PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
export PATH

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app_version=$(sed -n 's/^version = "\(.*\)"/\1/p' macos/Cargo.toml | head -1)
if [ "$version" != "$app_version" ]; then
  echo "Version in Cargo.toml ($version) und macos/Cargo.toml ($app_version) ist verschieden" >&2
  exit 1
fi
command -v x86_64-w64-mingw32-gcc >/dev/null || { echo "mingw-w64 fehlt (brew install mingw-w64)" >&2; exit 1; }

if [ "${SKIP_UI:-}" != "1" ] || [ ! -f ui/dist/public/index.html ]; then
  scripts/ui.sh
fi

target=x86_64-pc-windows-gnu
(cd macos && cargo build --release --locked --target $target)

out=dist-windows
rm -rf "$out"
mkdir -p "$out/nrzCode"
exe="${CARGO_TARGET_DIR:-macos/target}/$target/release/nrzcode-macos.exe"
# WebView2Loader.dll steckt im Programm (macos/src/webview2_loader.rs) und darf nicht in der
# Importtabelle stehen – sonst startet nrzCode.exe ohne die DLL daneben gar nicht erst.
objdump=$(command -v x86_64-w64-mingw32-objdump || command -v objdump) || { echo "objdump fehlt" >&2; exit 1; }
imports=$("$objdump" -p "$exe" | sed -n 's/^[[:space:]]*DLL Name: //p')
[ -n "$imports" ] || { echo "Importtabelle von $exe nicht lesbar" >&2; exit 1; }
if echo "$imports" | grep -qi webview2loader; then
  echo "WebView2Loader.dll steht in der Importtabelle von $exe – fehlt eine Funktion in webview2_loader.rs?" >&2
  exit 1
fi
cp "$exe" "$out/nrzCode/nrzCode.exe"
cat > "$out/nrzCode/LIESMICH.txt" <<TXT
nrzCode $version für Windows

nrzCode.exe starten (einzelne Datei, keine weiteren DLLs nötig).
Daten: %APPDATA%\\nrzCode   Protokoll: %LOCALAPPDATA%\\nrzCode\\Logs
Updates installiert die App selbst.

Beim ersten Start fragt Windows SmartScreen evtl. nach (die App ist nicht signiert):
"Weitere Informationen" → "Trotzdem ausführen".
TXT
if command -v zip >/dev/null; then
  (cd "$out" && zip -qr "nrzCode-$version-x64.zip" nrzCode)
else
  # Windows ohne zip: bsdtar des Systems schreibt ZIP mit /-Pfaden
  (cd "$out" && "${SYSTEMROOT:-C:/Windows}/System32/tar.exe" -a -c -f "nrzCode-$version-x64.zip" nrzCode)
fi
cp "$out/nrzCode/nrzCode.exe" "$out/nrzCode-$version-x64.exe"
# Installer (NSIS). Der Name enthält bewusst kein „x64“: Apps bis 1.0.28 nehmen fürs Update die
# erste EXE mit „x64“ im Namen – das muss das Programm bleiben, nicht der Installer.
if command -v makensis >/dev/null; then
  abs=$(pwd)/$out
  # Unter Windows erwartet makensis Windows-Pfade und „/D“ statt „-D“ stört Git Bash nicht
  if command -v cygpath >/dev/null; then abs=$(cygpath -w "$abs"); sep='\'; else sep=/; fi
  makensis -V2 "-DVERSION=$version" "-DEXE=$abs${sep}nrzCode${sep}nrzCode.exe" "-DOUT=$abs${sep}nrzCode-$version-Setup.exe" macos/resources/installer.nsi
else
  echo "makensis fehlt – kein Installer (brew install makensis)" >&2
fi
rm -rf "$out/nrzCode"
if command -v shasum >/dev/null; then sum="shasum -a 256"; else sum=sha256sum; fi
(cd "$out" && $sum nrzCode-*.zip nrzCode-*.exe | sed 's/ \*/  /' > SHA256SUMS)
echo "Fertig: $out/nrzCode-$version-x64.zip ($(du -h "$out/nrzCode-$version-x64.zip" | cut -f1))"
echo "        $out/nrzCode-$version-x64.exe"
[ -f "$out/nrzCode-$version-Setup.exe" ] && echo "        $out/nrzCode-$version-Setup.exe"
