import { StrictMode } from "react"
import { createRoot, hydrateRoot } from "react-dom/client"
import "./index.css"
import App from "./App"

const root = document.getElementById("root")!
const app = (
  <StrictMode>
    <App />
  </StrictMode>
)
// The build writes the page's HTML into #root (prerender.mjs); `vite dev` leaves it empty.
if (root.hasChildNodes()) hydrateRoot(root, app)
else createRoot(root).render(app)
