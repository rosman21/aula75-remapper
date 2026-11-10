#!/usr/bin/env bash
# Publish AULA 75 Control to Cloudflare: the web app (aula75-app) and the landing page with the
# Mac download (aula75). Needs `npx wrangler login` once. Run `bun run release` first for the DMG.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "== building the web app"
bun exact.mjs web-build --out dist

echo "== publishing the web app"
out=$(npx --yes wrangler@4 deploy -c deploy/app.wrangler.jsonc 2>&1 | tee /dev/stderr)
app_url=${AULA_APP_URL:-$(grep -oE 'https://aula75-app\.[a-z0-9.-]+' <<<"$out" | head -1)}
[ -n "$app_url" ] || { echo "couldn't read the web app's URL from wrangler; set AULA_APP_URL" >&2; exit 1; }

echo "== building the landing page (web app at $app_url)"
(cd site && bun install --frozen-lockfile && VITE_APP_URL="${app_url}/" bun run build)
rm -rf target/site && mkdir -p target/site
cp -R site/dist/. target/site/
dmg=$(ls target/dist/*/"AULA 75 Control.dmg" 2>/dev/null | head -1 || true)
if [ -n "$dmg" ]; then
  mkdir -p target/site/download && cp "$dmg" target/site/download/AULA-75-Control.dmg
else
  echo "warning: no notarized DMG yet (bun run release); the Download button will 404" >&2
fi

echo "== publishing the landing page"
npx --yes wrangler@4 deploy -c deploy/site.wrangler.jsonc
