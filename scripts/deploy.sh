#!/usr/bin/env bash
# Publish Remap75 to Cloudflare: the web app (remap75-app, https://web.remap75.app) and the
# landing page with the Mac download (remap75, https://remap75.app). Needs `npx wrangler login` once. Run `bun run release` first for the DMG.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "== building the web app"
bun exact.mjs web-build --out dist

echo "== publishing the web app"
npx --yes wrangler@4 deploy -c deploy/app.wrangler.jsonc
# The web app's address (its custom domain in deploy/app.wrangler.jsonc).
app_url=${REMAP75_APP_URL:-https://web.remap75.app}

echo "== building the landing page (web app at $app_url)"
(cd site && bun install --frozen-lockfile && VITE_APP_URL="${app_url}/" bun run build)
rm -rf target/site && mkdir -p target/site
cp -R site/dist/. target/site/
dmg=$(ls target/dist/*/"Remap75.dmg" 2>/dev/null | head -1 || true)
if [ -n "$dmg" ]; then
  mkdir -p target/site/download && cp "$dmg" target/site/download/Remap75.dmg
else
  echo "warning: no notarized DMG yet (bun run release); the Download button will 404" >&2
fi

echo "== publishing the landing page"
npx --yes wrangler@4 deploy -c deploy/site.wrangler.jsonc
