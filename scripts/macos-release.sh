#!/bin/sh
# DMG (dist-macos/) und ZIP/EXE (dist-windows/) als gemeinsames Release `v<version>` in nrzGit
# veröffentlichen (PLATFORM=macos|windows: nur eine Plattform mit altem Tag). Die installierten
# Apps finden es von selbst (Update-Hinweis im Fenster).
#
#   NRZGIT_TOKEN=nrza_… scripts/macos-release.sh [release-notes.md]
#
# Optional: NRZGIT_TARGET (Branch für den Tag, Standard: aktueller Branch), NRZGIT_URL (Standard http://100.78.107.162:8390), NRZGIT_REPO (nicorenezweers/nrzCode).
# Vorher scripts/macos.sh ausführen. Braucht curl und jq.
set -eu
cd "$(dirname "$0")/.."

: "${NRZGIT_TOKEN:?NRZGIT_TOKEN fehlt (nrzGit → Einstellungen → Zugriffstoken)}"
url=${NRZGIT_URL:-http://100.78.107.162:8390}
repo=${NRZGIT_REPO:-nicorenezweers/nrzCode}
owner=${repo%%/*}
name=${repo#*/}

version=$(sed -n 's/^version = "\(.*\)"/\1/p' macos/Cargo.toml | head -1)
# Ein gemeinsames Release für macOS und Windows (Tag `v<version>`); die Apps suchen sich ihre
# Datei selbst (DMG bzw. EXE). Mit PLATFORM=macos|windows nur eine Plattform (altes Schema).
platform=${PLATFORM:-all}
case "$platform" in
  all) dists="dist-macos dist-windows"; tag="v$version"; label="macOS und Windows" ;;
  macos) dists=dist-macos; tag="macos-$version"; label="macOS" ;;
  windows) dists=dist-windows; tag="windows-$version"; label="Windows" ;;
  *) echo "PLATFORM: all, macos oder windows" >&2; exit 1 ;;
esac
# Tag auf den aktuellen Branch (muss gepusht sein), mit NRZGIT_TARGET überschreibbar
target=${NRZGIT_TARGET:-$(git rev-parse --abbrev-ref HEAD)}
set -- "${1:-}"
files=""
sums=$(mktemp)
for d in $dists; do
  found=$(ls "$d"/nrzCode-"$version"-* 2>/dev/null || true)
  [ -n "$found" ] || { echo "Keine Dateien für $version in $d/ – zuerst scripts/macos.sh bzw. scripts/windows.sh" >&2; exit 1; }
  files="$files $found"
  for f in $found; do grep " $(basename "$f")\$" "$d/SHA256SUMS" >> "$sums" || true; done
done
sumdir=$(mktemp -d)
mv "$sums" "$sumdir/SHA256SUMS"

if [ -n "$1" ]; then
  notes=$(cat "$1")
else
  notes="# nrzCode $version

**macOS:** DMG öffnen und **nrzCode** in **Programme** ziehen. Beim ersten Start: Rechtsklick auf die App → **Öffnen** (nicht von Apple notarisiert).

**Windows:** **nrzCode-$version-Setup.exe** ausführen (Installer) – oder die ZIP entpacken und **nrzCode.exe** starten.

Installierte Apps finden neue Versionen selbst und zeigen einen Hinweis mit Knopf **Aktualisieren**."
fi

# MCP-Aufruf an nrzGit (JSON-RPC über HTTP); gibt den Text des Ergebnisses aus
mcp() {
  tool=$1
  args=$2
  jq -n --arg tool "$tool" --argjson args "$args" '{jsonrpc:"2.0",id:1,method:"tools/call",params:{name:$tool,arguments:$args}}' |
    curl -sS --fail -X POST "$url/mcp" \
      -H "Authorization: Bearer $NRZGIT_TOKEN" \
      -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
      --data-binary @- |
    sed -n 's/^data: //p; /^{/p' | head -1 |
    jq -er 'if .error then error(.error.message) elif .result.isError then error(.result.content[0].text) else .result.content[0].text end'
}

base=$(jq -n --arg o "$owner" --arg r "$name" '{owner:$o,repo:$r}')

existing=$(mcp get_release_by_tag "$(printf '%s' "$base" | jq --arg t "$tag" '. + {tag:$t}')" 2>/dev/null || true)
if [ -n "$existing" ]; then
  echo "Release $tag gibt es schon (id $(printf '%s' "$existing" | jq .id)) – zuerst löschen oder Version erhöhen." >&2
  exit 1
fi

echo "Release $tag anlegen (Entwurf) …"
created=$(mcp create_release "$(printf '%s' "$base" | jq --arg t "$tag" --arg n "$notes" --arg v "nrzCode $version ($label)" --arg g "$target" \
  '. + {tag_name:$t, target:$g, title:$v, body:$n, is_draft:true}')")
id=$(printf '%s' "$created" | jq -r .id)

for f in $files "$sumdir/SHA256SUMS"; do
  echo "Hochladen: $(basename "$f") ($(du -h "$f" | cut -f1)) …"
  tmp=$(mktemp)
  base64 -i "$f" | tr -d '\n' > "$tmp"
  printf '%s' "$base" | jq --argjson id "$id" --arg n "$(basename "$f")" --rawfile c "$tmp" \
    '{jsonrpc:"2.0",id:1,method:"tools/call",params:{name:"upload_release_asset",arguments:(. + {id:$id,name:$n,content_base64:$c})}}' > "$tmp.json"
  curl -sS --fail -X POST "$url/mcp" -H "Authorization: Bearer $NRZGIT_TOKEN" \
    -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
    --data-binary @"$tmp.json" | sed -n 's/^data: //p; /^{/p' | head -1 |
    jq -er 'if .error then error(.error.message) elif .result.isError then error(.result.content[0].text) else "ok" end' >/dev/null
  rm -f "$tmp" "$tmp.json"
done

echo "Veröffentlichen …"
mcp publish_release "$(printf '%s' "$base" | jq --argjson id "$id" '. + {id:$id}')" >/dev/null
echo "Fertig: $url/$repo/releases/tag/$tag"
