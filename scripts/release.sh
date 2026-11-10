#!/usr/bin/env bash
# Build AULA 75 Control for download: Developer ID signed, hardened, notarized and stapled,
# as an app and as a DMG (target/dist/aula-75-keyboard-software/). Needs the notarytool keychain
# profile once: xcrun notarytool store-credentials aula75-notary --apple-id <id> --team-id 3MHQRSS4YC
set -uo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
ID=${EXACT_DEVELOPER_ID:-9EFD946791746019C89A4C171FF03948FB321ADE}
PROFILE=${EXACT_NOTARY_PROFILE:-aula75-notary}
out=target/dist/aula-75-keyboard-software

# Exact notarizes and staples the app, then tries to staple the DMG it never submitted (that
# step fails): carry on as long as the app itself came back notarized.
EXACT_DEVELOPER_ID=$ID EXACT_NOTARY_PROFILE=$PROFILE bun "${EXACT2:-$HOME/dev/exact}/scripts/exact.mjs" release "$PWD"
set -e
xcrun stapler validate "$out/AULA 75 Control.app" >/dev/null || { echo "the app isn't notarized; see above" >&2; exit 1; }

echo "== signing and notarizing the DMG"
codesign --force --timestamp --sign "$ID" "$out/AULA 75 Control.dmg"
xcrun notarytool submit "$out/AULA 75 Control.dmg" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$out/AULA 75 Control.dmg"
spctl -a -t open --context context:primary-signature -vv "$out/AULA 75 Control.dmg"
spctl -a -vv "$out/AULA 75 Control.app"
echo "ready: $out/AULA 75 Control.dmg (bun run deploy publishes it)"
