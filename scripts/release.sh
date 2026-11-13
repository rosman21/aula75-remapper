#!/usr/bin/env bash
# Release Remap75 from this Mac, start to finish (no CI service involved):
#
#   bun run release <version> "<one line of what changed>"      e.g. bun run release 0.2.0 "Flash Mac keys"
#
#   1. checks: a clean tree, a version later than the current one, a free tag
#   2. tests: the data crate and the app's contract tests
#   3. sets the version (Cargo.toml's workspace version, which the app shows and the bundle carries)
#   4. builds the Mac app: Developer ID signed, hardened, notarized and stapled, app and DMG
#   5. writes latest.json, which installed apps read to learn there is an update
#   6. commits "Release <version>" and tags v<version>
#   7. publishes, after asking: the site with the DMG and latest.json (Cloudflare), the web app,
#      the commit and tag (GitHub), and a GitHub release with the DMG
#
# Flags: --dry-run   steps 1-3 and 5 only, then puts the version back (nothing built or published)
#        --no-publish stop after step 6; `bun run release --publish` publishes that build later
#        --yes        publish without asking
#
# Needs once: xcrun notarytool store-credentials aula75-notary --apple-id <id> --team-id 3MHQRSS4YC
#             npx wrangler login       gh auth login
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
ID=${EXACT_DEVELOPER_ID:-9EFD946791746019C89A4C171FF03948FB321ADE}
PROFILE=${EXACT_NOTARY_PROFILE:-aula75-notary}
SITE=https://remap75.app
out=target/dist/aula-75-keyboard-software

dry=0 publish=ask only_publish=0 version="" notes=""
for a in "$@"; do
  case "$a" in
    --dry-run) dry=1 ;;
    --no-publish) publish=no ;;
    --yes) publish=yes ;;
    --publish) only_publish=1 ;;
    -*) echo "unknown flag $a" >&2; exit 2 ;;
    *) if [ -z "$version" ]; then version=${a#v}; elif [ -z "$notes" ]; then notes=$a; else echo "too many arguments" >&2; exit 2; fi ;;
  esac
done

current=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$current" ] || { echo "no workspace version in Cargo.toml" >&2; exit 1; }

publish_release() { # the build in $out, at the version in Cargo.toml
  local v=$current
  [ -f "$out/latest.json" ] && grep -q "\"version\": \"$v\"" "$out/latest.json" \
    || { echo "no release build of $v in $out (run: bun run release <version>)" >&2; exit 1; }
  git rev-parse -q --verify "refs/tags/v$v" >/dev/null || { echo "no tag v$v" >&2; exit 1; }
  echo "== publishing the site, the download and the web app"
  bash scripts/deploy.sh
  echo "== pushing the commit and tag"
  git push origin HEAD "v$v"
  echo "== GitHub release v$v"
  gh release create "v$v" "$out/Remap75-$v.dmg" --title "Remap75 $v" \
    --notes "$(sed -n 's/^ *"notes": "\(.*\)",\{0,1\}$/\1/p' "$out/latest.json")"
  echo "== checking what installed apps will read"
  curl -fsS "$SITE/latest.json"; echo
  echo "released: Remap75 $v"
}

if [ "$only_publish" = 1 ]; then
  publish_release
  exit 0
fi

# ---------------------------------------------------------------- 1. checks
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage: bun run release <version like 0.2.0> \"<what changed>\"" >&2; exit 2; }
[ -n "$notes" ] || { echo "say what changed, in one line: bun run release $version \"<what changed>\"" >&2; exit 2; }
[ "$(printf '%s\n%s\n' "$current" "$version" | sort -V | tail -1)" = "$version" ] && [ "$version" != "$current" ] \
  || { echo "$version is not later than the current $current" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "commit or stash your changes first:" >&2; git status --short >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then echo "tag v$version already exists" >&2; exit 1; fi
echo "== releasing Remap75 $version (from $current, branch $(git branch --show-current))"

# ---------------------------------------------------------------- 2. tests
echo "== tests"
(cd data && cargo test --quiet)
bun exact.mjs test

# ---------------------------------------------------------------- 3. version
restore() { git checkout -q -- Cargo.toml Cargo.lock; echo "the version is back at $current" >&2; }
trap restore ERR INT
sed -i '' "/^\[workspace\.package\]/,/^\[/s/^version = \".*\"/version = \"$version\"/" Cargo.toml
cargo update --workspace --offline --quiet
(cd data && cargo test --quiet update::) # the app's own idea of its version compiles and compares

write_feed() { # $1: where
  VERSION=$version NOTES=$notes URL="$SITE/download/Remap75.dmg" python3 - "$1" <<'PY'
import json, os, sys, datetime
json.dump({"version": os.environ["VERSION"], "url": os.environ["URL"], "notes": os.environ["NOTES"],
           "released": datetime.date.today().isoformat()}, open(sys.argv[1], "w"), indent=2, ensure_ascii=False)
open(sys.argv[1], "a").write("\n")
PY
}

if [ "$dry" = 1 ]; then
  write_feed target/latest.dry-run.json
  echo "== dry run: latest.json would be"; cat target/latest.dry-run.json
  trap - ERR INT; restore
  echo "dry run finished: nothing built, committed or published"
  exit 0
fi

# ---------------------------------------------------------------- 4. build
echo "== building, signing and notarizing the app"
# Exact notarizes and staples the app, then tries to staple the DMG it never submitted (that
# step fails): carry on as long as the app itself came back notarized.
EXACT_DEVELOPER_ID=$ID EXACT_NOTARY_PROFILE=$PROFILE bun "${EXACT2:-$HOME/dev/exact}/scripts/exact.mjs" release "$PWD" || true
xcrun stapler validate "$out/Remap75.app" >/dev/null || { echo "the app isn't notarized; see above" >&2; false; }
built=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$out/Remap75.app/Contents/Info.plist")
[ "$built" = "$version" ] || { echo "the built app says version $built, not $version" >&2; false; }

echo "== signing and notarizing the DMG"
codesign --force --timestamp --sign "$ID" "$out/Remap75.dmg"
xcrun notarytool submit "$out/Remap75.dmg" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$out/Remap75.dmg"
spctl -a -t open --context context:primary-signature -vv "$out/Remap75.dmg"
spctl -a -vv "$out/Remap75.app"

# ---------------------------------------------------------------- 5. feed
cp "$out/Remap75.dmg" "$out/Remap75-$version.dmg"
write_feed "$out/latest.json"

# ---------------------------------------------------------------- 6. commit and tag
git add Cargo.toml Cargo.lock
git commit -q -m "Release $version" -m "$notes"
git tag -a "v$version" -m "Remap75 $version: $notes"
trap - ERR INT
current=$version
echo "ready: $out/Remap75.dmg, committed and tagged v$version"

# ---------------------------------------------------------------- 7. publish
if [ "$publish" = ask ]; then
  read -r -p "Publish Remap75 $version to remap75.app and GitHub now? [y/N] " answer
  [[ "$answer" =~ ^[Yy] ]] && publish=yes || publish=no
fi
if [ "$publish" = yes ]; then
  publish_release
else
  echo "not published. When ready: bun run release --publish"
fi
