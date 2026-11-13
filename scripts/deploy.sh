#!/usr/bin/env bash
# Publish Remap75 to Cloudflare: the web app (remap75-app, https://web.remap75.app) and the
# landing page with the Mac download (remap75, https://remap75.app). Needs `npx wrangler login` once. Run `bun run release` first for the DMG.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "== building the web app"
bun exact.mjs web-build --out dist
# Link previews: Exact's page has none, so add the tags (deploy/web-meta.html) after its viewport.
python3 - <<'PY'
import re
page = open("dist/index.html").read()
meta = open("deploy/web-meta.html").read()
page = re.sub(r'<link rel="icon"[^>]*>', "", page, count=1)  # Exact's grey placeholder icon
page, n = re.subn(r'(<meta name="viewport"[^>]*>)', lambda m: m.group(1) + "\n" + meta, page, count=1)
assert n == 1, "no viewport tag in dist/index.html to put the link-preview tags after"
open("dist/index.html", "w").write(page)
PY

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
  # What installed apps read to learn of an update (written by `bun run release <version>`):
  # published in the same deploy as the DMG it names.
  feed="$(dirname "$dmg")/latest.json"
  if [ -f "$feed" ]; then cp "$feed" target/site/latest.json; else echo "warning: no latest.json beside the DMG; installed apps won't hear of this build" >&2; fi
else
  echo "warning: no notarized DMG yet (bun run release); the Download button will 404" >&2
fi

echo "== publishing the landing page"
npx --yes wrangler@4 deploy -c deploy/site.wrangler.jsonc
