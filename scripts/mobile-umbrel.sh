#!/bin/sh
# nrzCodeMobile für Umbrel veröffentlichen: die Programme aus mobile/dist/ (scripts/mobile.sh linux)
# kommen in das Repository nrzCodeMobile auf nrzGit (Zweig `umbrel`) und bekommen den Tag
# `umbrel-<version>`. Die App `nrz-code-mobile` im nrzStore lädt sie von dort (hooks/pre-start);
# nrzWorkflow führt den Store beim neuen Tag selbst nach.
#
#   scripts/mobile-umbrel.sh "Was diese Version bringt"
#
# Optional: NRZGIT_URL (Standard http://100.78.107.162:8390), NRZGIT_REPO (nicorenezweers/nrzCodeMobile).
set -eu
cd "$(dirname "$0")/.."
root=$(pwd)
url=${NRZGIT_URL:-http://100.78.107.162:8390}
repo=${NRZGIT_REPO:-nicorenezweers/nrzCodeMobile}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' mobile/Cargo.toml | head -1)
notes=${1:-"nrzCodeMobile $version"}
tag="umbrel-$version"
for f in nrzcode-mobile-amd64.gz nrzcode-mobile-arm64.gz SHA256SUMS; do
  [ -f "mobile/dist/$f" ] || { echo "mobile/dist/$f fehlt – zuerst scripts/mobile.sh linux" >&2; exit 1; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git clone -q "$url/$repo.git" "$work/repo"
cd "$work/repo"
if git ls-remote --exit-code --heads origin umbrel >/dev/null 2>&1; then
  git checkout -q umbrel
else
  git checkout -q --orphan umbrel
  git rm -rq . 2>/dev/null || true
fi
if git ls-remote --exit-code --tags origin "$tag" >/dev/null 2>&1; then
  echo "Tag $tag gibt es schon – Version in mobile/Cargo.toml erhöhen" >&2
  exit 1
fi
cp "$root"/mobile/dist/nrzcode-mobile-amd64.gz "$root"/mobile/dist/nrzcode-mobile-arm64.gz "$root"/mobile/dist/SHA256SUMS .
cat > README.md <<MD
# nrzCodeMobile $version für Umbrel

Programme für die App \`nrz-code-mobile\` im nrzStore (statisch gebaut, amd64 und arm64).
\`hooks/pre-start\` lädt sie über den Tag \`$tag\` von hier. Quellcode: \`mobile/\` im Repository
[nrzCode]($url/nicorenezweers/nrzCode).
MD
git add -A
git -c user.name="$(git -C "$root" config user.name || echo nrzCode)" -c user.email="$(git -C "$root" config user.email || echo nrzcode@localhost)" commit -qm "nrzCodeMobile $version für Umbrel (amd64, arm64)"
git -c user.name="$(git -C "$root" config user.name || echo nrzCode)" -c user.email="$(git -C "$root" config user.email || echo nrzcode@localhost)" tag -a "$tag" -m "$notes"
git push -q origin umbrel "$tag"
echo "Veröffentlicht: $url/$repo/src/tag/$tag"
