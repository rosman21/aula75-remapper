// After `vite build`: write the page's HTML into dist/index.html and the sitemap beside it.
import { readFileSync, writeFileSync, rmSync } from "node:fs"
import { html } from "./dist-ssr/entry-server.js"

const SITE = "https://remap75.app"
const page = readFileSync("dist/index.html", "utf8")
const empty = '<div id="root"></div>'
if (!page.includes(empty)) throw new Error("dist/index.html has no empty #root to fill")
const body = html()
if (!body.includes("<h1")) throw new Error("the prerendered page has no <h1>")
writeFileSync("dist/index.html", page.replace(empty, () => `<div id="root">${body}</div>`))

const today = new Date().toISOString().slice(0, 10)
writeFileSync(
  "dist/sitemap.xml",
  `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>${SITE}/</loc>
    <lastmod>${today}</lastmod>
  </url>
</urlset>
`,
)
rmSync("dist-ssr", { recursive: true, force: true })
console.log(`prerendered index.html (${body.length} bytes of page) and sitemap.xml (${today})`)
