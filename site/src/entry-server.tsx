// The page as HTML, for the build to write into index.html (prerender.mjs), so search engines
// and link previews read the whole page without running its script.
import { StrictMode } from "react"
import { renderToString } from "react-dom/server"
import App from "./App"

export function html(): string {
  return renderToString(
    <StrictMode>
      <App />
    </StrictMode>,
  )
}
