<img src="assets/icon.svg" width="64" height="64" alt="">

# Remap75

> [!WARNING]
> **This project is completely vibe coded.** Every line (the Rust, the contract, the WebHID
> driver, the site, the release and deploy scripts, and this README) was written by an AI
> coding agent (Claude) from plain-language requests, without a human writing or line-by-line
> reviewing the code. It has been tested against a real AULA F75 and a simulated keyboard, but
> treat it as such: read before you trust it, and remember that **Fn+Esc held for ~5 s** resets
> the keyboard to factory settings if anything goes wrong.

> [!IMPORTANT]
> **Not affiliated with AULA.** This is an independent, unofficial project. It is not made,
> endorsed, sponsored or supported by AULA or the keyboard's manufacturer. See
> [Disclaimer](#disclaimer).

**Remap75** is a configurator for the **AULA 75** keyboard (the AULA F75), in the spirit of VIA/Vial:
remap keys, make one key send a combination (⌘C, ⇧⌘4…), use the Fn layer, build macros, and set
lighting and sleep. It talks to the keyboard through its **2.4 GHz USB receiver** or, as a
fallback, the **USB-C cable**. Everything is written to the keyboard itself, so it keeps working
on any computer, even over Bluetooth.

One codebase ships three ways:

| | Where | How it reaches the keyboard |
|---|---|---|
| **Mac app** | Download (signed, notarized DMG) | hidapi over IOKit, in-process Rust |
| **Web app** | Chrome / Edge / Opera, nothing to install | WebHID, driven from Rust compiled to WebAssembly |
| **Linux app** | Built from source | hidapi over hidraw (needs the udev rule) |

- Landing page and download: **https://remap75.app**
- Web app: **https://web.remap75.app**

The rest of this file is how it's built.

---

## Contents

1. [Architecture at a glance](#architecture-at-a-glance)
2. [How Exact is used](#how-exact-is-used)
3. [The data crate: talking to the keyboard](#the-data-crate-talking-to-the-keyboard)
4. [The browser path: WebHID through a replay journal](#the-browser-path-webhid-through-a-replay-journal)
5. [The keyboard protocol](#the-keyboard-protocol)
6. [What the app does](#what-the-app-does)
7. [Repository layout](#repository-layout)
8. [Developing](#developing)
9. [Releasing the Mac app](#releasing-the-mac-app)
10. [Deploying the web app and site (Cloudflare)](#deploying-the-web-app-and-site-cloudflare)
11. [The landing site](#the-landing-site)
12. [Known limits and gotchas](#known-limits-and-gotchas)
13. [Contributing](#contributing)
14. [Disclaimer](#disclaimer)

---

## Architecture at a glance

```
                  ┌──────────────────────────── app.contract ────────────────────────────┐
                  │  routes, screens, components, mutations, actions (the whole UI)      │
                  └───────────────────────────────┬──────────────────────────────────────┘
                                                  │ baked into a plan by Exact at build time
                                                  ▼
      ┌──────────────────── Exact host (one per platform) ─────────────────────┐
      │  macOS: AppKit       Linux: native painter      Web: DOM + wasm        │
      └───────────────────────────────┬────────────────────────────────────────┘
                                      │ every query/mutation is a named "source"
                                      ▼
            ┌──────────────── data/ (crate aula75-data) ─────────────────┐
            │  source.rs   Exact DataSource: parses calls, runs them      │
            │  service.rs  connection state, keymap, lighting, macros     │
            │  session.rs  acknowledged writes, reassembled reads         │
            │  protocol.rs framing, checksums, LED block, key codes       │
            │  ┌────────────── Wire (trait) ──────────────┐               │
            │  │ hid.rs     hidapi  (macOS, Linux)        │               │
            │  │ sim.rs     packet-level simulated board  │               │
            │  │ remote.rs  replay journal → WebHID (web) │               │
            │  └──────────────────────────────────────────┘               │
            └─────────────────────────────────────────────────────────────┘
                                      │ web only: one I/O step per round trip
                                      ▼
                       modules/web/index.js  (navigator.hid)
```

The UI is declared once in `app.contract`. The keyboard logic lives once, in Rust, in
`data/`. Only the bottom layer, the `Wire` that moves bytes, differs per platform.

---

## How Exact is used

[Exact](https://github.com/expo/exact) (Expo's "exact2") builds native apps from a declarative
**contract** plus a **data module**. This project uses it like this.

### The contract: `app.contract`

`app.contract` is the entire UI:

- **Routes**: four tabs, `device "/"`, `keys "/keys"` (with a key picker at
  `/keys/:layer/:index`), `lighting "/lighting"` and `macros "/macros"` (with an editor at
  `/macros/:id`). On the Mac they appear as a sidebar.
- **Shapes**: typed records the data module returns (`Status`, `Reply`, `Lighting`, `KeyCap`, …).
- **Components**: `DeviceScreen`, `KeysScreen`, `KeyPicker`, `LightingForm`, `MacrosScreen` and
  `MacroEditor`.
- **Mutations**: each declares what to refresh afterwards. For example,
  `mutation assigned as shape Reply refreshes keymap then afterAssign` rewrites the keymap and
  then navigates back.
- **Actions**: what controls do, for example
  `action assign(l, index, target, mods) → send assigned = assignKey(...)`.

`app.test.contract` holds the UI tests. They drive the real UI against the simulated keyboard,
for example "Caps → Esc shows `Esc` with `was Caps` under it".

At build time Exact **bakes** the contract into a binary plan (`app.plan`). Each host crate
includes it with `include_bytes!(concat!(env!("OUT_DIR"), "/app.plan"))`.

### The data module: a Rust `DataSource`

Exact apps normally use `app.ts` as their data module. This one uses a **Rust crate** instead,
because Exact's Linux host can't run TypeScript and the keyboard needs native HID access
anyway. `data/src/source.rs` implements Exact's `DataSource` trait for `Aula`:

- `query(source, args)` answers static tables at once: the lighting effects, the palette and
  the sleep options.
- `answer(source, args)` handles every other source: `deviceStatus`, `keymap`, `assignKey`,
  `restoreLayer`, `lighting`, `setLighting`, `setSleep`, `macroList`, `addMacro`, `addStep`,
  `uploadMacros`, `requestAccess`, `useDemo`, `reloadKeys` and the rest of the macro calls. On
  native hosts the call runs on a background job and the UI is told when it finishes. On the web
  it returns `Answer::Later(Request::native(...))`; see the
  [browser path](#the-browser-path-webhid-through-a-replay-journal).
- `parse(store, source, args, outcome)` takes the reply to a `Later` request and either
  finishes or asks for another step.

### An authored Cargo workspace

Exact normally generates a Rust workspace. This repo **authors its own** (`Cargo.toml` at the
root), so Exact uses it as is:

| Crate | Role |
|---|---|
| `data/` (`aula75-data`) | The keyboard: protocol, sessions, service, the `DataSource` |
| `apple/` | macOS host: links `exact-apple` with the data crate and the baked plan |
| `linux/` | Linux host: same, with Exact's Linux painter |
| `web/` | Web host: the data crate compiled to `wasm32-unknown-unknown` |

Each host crate's `build.rs` uses Exact's `contract` CLI and `exact-bake` to compile
`app.contract` into the plan, then `include!`s Exact's generated `entry.rs`. The workspace
patches several crates (`taffy`, `parley`, `fontique`, `wgpu-hal`, `accesskit_windows`) to
Exact's vendored copies. Those paths, and the `exact-*` dependencies, point at the Exact
checkout (see [Developing](#developing)).

`rust-toolchain.toml` pins Rust **1.97.0** with the `wasm32-unknown-unknown` target.

### The page module: `modules/web/index.js`

Exact lets a web app ship a JavaScript **page module**. Its `later(request)` is called for any
`Request::native(...)` a data source makes. This app's page module is the WebHID driver. It
exports:

- `connect(context)`: records whether it runs under Exact's test agent (then it answers
  "demo") and listens for USB disconnects.
- `later(request)`: performs one keyboard I/O step and returns the extended journal.

### Exact verbs this project uses

| Command | What it does |
|---|---|
| `bun exact.mjs mac` | Build the macOS app (`bun run mac` also signs it with the Developer ID and opens it) |
| `bun exact.mjs linux` | Build the Linux app |
| `bun exact.mjs web-build --out dist` | Build the web app into `dist/` (`bun run build`) |
| `bun exact.mjs serve dist` | Serve it locally (`bun run start`) |
| `bun exact.mjs test web` | Run `app.test.contract` against the simulated keyboard |
| `bun exact.mjs agent web …` | Drive the UI headlessly (used for the site's screenshots) |
| `bun ~/dev/exact/scripts/exact.mjs release <path>` | Hardened-runtime signing, notarization and a DMG (wrapped by `scripts/release.sh`) |

`app.json` is the manifest: app id `com.remap75.app`, name "Remap75", minimum macOS
14.0, a 1180×760 window (900×560 minimum), and hosts `macos`, `linux` and `web`.

### A local patch to Exact (required for the web build)

Exact's JavaScript web target (`host/web-js/rust-data.js`) originally sent a Rust data
source's `Request::native` to the HTTP executor, which refused it ("outside the app's grants
(net.fetch)"). This project needs those requests routed to the page module, so the Exact
checkout carries a small patch:

- `host/web-js/rust-data.js`: `install(data, sources, load, modules)` takes the page modules.
  Its `pageLater` sends `exact-native:` requests to
  `pageModule({agent: clock.agent}).later(JSON.parse(body))` and returns the result as a 200
  JSON reply.
- `host/web-js/build.mjs`: passes `() => import('./native.js')` to `install` when the app has
  page modules.

**Re-apply this patch if you update Exact.** Without it the web app can't reach the keyboard.
The Mac and Linux builds don't need it.

---

## The data crate: talking to the keyboard

`data/` is plain Rust with no UI. It has 20 unit tests (`cargo test -p aula75-data`).

| File | Responsibility |
|---|---|
| `protocol.rs` | Packet framing for both links, checksums, the LED settings block, key-code encoding (`Link::{Receiver, Wired}`) |
| `session.rs` | The `Wire` trait and `Session`: acknowledged, retried packet writes; multi-packet reads reassembled by index; "asleep" detection |
| `hid.rs` | Finds the receiver/cable by USB id and interface, opens it **shared** (typing keeps working), and handles the macOS Input Monitoring permission |
| `sim.rs` | `SimKeyboard`: a packet-level simulation of the F75, used by the tests, by `AULA_SIMULATE=1` and by "Try it with a simulated keyboard" |
| `layout.rs` | The F75's 75% layout: rows, key indexes, factory codes, labels |
| `keycodes.rs` | Everything a key can become, with Mac names (⌘ Cmd, ⌥ Opt, …) |
| `macros.rs` | Macro steps (type text, combination, wait), play modes, the keyboard's macro buffer format |
| `service.rs` | Connection state machine, keymap read/assign/restore, lighting, sleep, macros, factory backups, `cap_label` for keycaps |
| `source.rs` | The Exact `DataSource` (`Aula`) |
| `remote.rs` | The replay journal and `RemoteWire` for the browser |

Things worth knowing:

- **Safety before writes.** Before writing a layer, the service reads it and checks it looks
  like an F75's. The first time it reads each layer it saves a copy (`backup-*`), and "Restore"
  writes that copy back.
- **Link preference.** Status is polled every 2.5 s. If both the receiver and the cable are
  present, the cable wins (it's faster). If the receiver is there but the keyboard is asleep or
  out of range, the app says so instead of failing.
- **Files.** Macros and backups live in `~/Library/Application Support/Remap75/` on
  macOS, `~/.config/remap75/` on Linux, and browser `localStorage` (keys prefixed
  `aula75:`) on the web. Native writes go through a temporary file and a rename. A folder from
  before the rename (`AULA 75 Control` / `aula75-control`) is moved to the new place on first
  launch, so macros and saved factory layers carry over.
- **Diagnostics.** Creating `~/Library/Application Support/Remap75/diag/enabled` (or
  `bun run mac:diag`) turns on extra diagnostic commands. They are off in the browser.

---

## The browser path: WebHID through a replay journal

A web page can't block on USB, but the service and protocol code are written as ordinary
blocking Rust. Rather than port them to async, the web build **replays** each call:

1. The UI asks for, say, `assignKey(0, 41, "esc", 0)`. `source.rs` starts a run with an empty
   journal (`remote::begin`).
2. The service runs normally. When it reaches I/O, `RemoteWire` calls `remote::io(op)`.
   - If the journal already has a result for this op, it's returned and the run continues.
   - Otherwise the op is recorded as **pending** and the run stops with the `NEED` error.
     Every later op also returns `NEED`, so a run that stopped early **never writes**.
3. `remote::finish()` reports the pending op. The source returns
   `Answer::Later(Request::native({aula: 1, op, journal}))`.
4. Exact (through the patch above) hands that to `modules/web/index.js` `later()`, which runs
   the single step over WebHID and returns `{journal: [...journal, [op, result]]}`.
5. `parse` replays the same call with the longer journal, and so on until a run finishes
   without asking. That result goes to the UI.

If a replay asks for a different op than the journal recorded (state changed in between), the
journal is cut at that point and the step is asked again.

The page module's steps:

| Step | Meaning |
|---|---|
| `device` | Which keyboard is open: `{mode: "device", link, product}`, `"none"`, `"unsupported"` (no WebHID) or `"demo"` (under the test agent, or `?demo` in the URL) |
| `connect` | Chrome's device picker; needs the click that asked for it |
| `read cmd sub len` | A whole read of one command, returned as hex |
| `write cmd sub data delay` | A whole write of one command |
| `get name` / `put name data` | The app's files, in `localStorage` |

Results are `{ok: value}` or `{err: message}`. `"asleep"` maps back to the protocol's
asleep error, and other errors are prefixed `USB:` so the service drops the session and
reconnects. The page module is a line-by-line port of `session.rs`/`protocol.rs` framing for
both links, and it queues steps so a status poll never interleaves with a key write.

WebHID filters: receiver `{vendorId: 0x3554, productId: 0xfa09, usagePage: 0xff02}`, cable
`{vendorId: 0x258a, productId: 0x010c}`.

---

## The keyboard protocol

| | USB id | Interface | Transport |
|---|---|---|---|
| 2.4 GHz receiver | `3554:fa09` | interface 1, usage page `0xFF02` | 20-byte report `0x13`, checksummed, acknowledged per packet, 14 data bytes per packet |
| USB-C cable | `258a:010c` | interface 1 (vendor page) | 520-byte feature report `0x06`, 512 data bytes per frame |
| Bluetooth | `3554:fa08` | — | Not used: config traffic disrupts typing. Settings written over the other links still apply |

The receiver's config interface also carries your keystrokes, which is why:

- **macOS** asks for **Input Monitoring**. Choose *Allow access* in the app, turn it on in System
  Settings → Privacy & Security → Input Monitoring, then reopen the app.
- **Linux** needs the udev rule:
  `sudo cp udev/70-remap75.rules /etc/udev/rules.d/ && sudo udevadm control --reload-rules && sudo udevadm trigger`,
  then replug. Chrome on Linux needs it too.

Command tables, the LED block, key codes and the macro buffer are documented in
[`docs/PROTOCOL.md`](docs/PROTOCOL.md), with the community reverse-engineering sources they
come from.

If a write ever leaves the keyboard in a bad state, holding **Fn+Esc** for about 5 seconds
resets it to factory settings.

---

## What it does

- **Keys**: a drawn 75% board for the base and Fn layers. Click a key, and the whole keyboard
  appears again as the menu of what it can become (letters, modifiers, media, navigation,
  macros, Fn, nothing), with ⌃ ⇧ ⌥ ⌘ toggles for combinations. The change is written at once
  and the keycap updates to show its new label, with "was …" underneath.
- **Lighting**: 18 built-in effects, brightness 0–9, speed 0–4, palette colour, applied live,
  plus the sleep timer.
- **Macros**: steps (type text, press a combination, wait), played once, while held, or as a
  toggle, then written to the keyboard's macro memory.
- **Restore**: puts the factory base layer, or the saved Fn layer, back.
- **Simulated keyboard**: try everything without hardware ("Try it with a simulated keyboard",
  `?demo` on the web, `AULA_SIMULATE=1` natively).

---

## Repository layout

| Path | What it is |
|---|---|
| `app.contract`, `app.test.contract` | The UI and its tests (Exact) |
| `app.json` | Exact manifest: id, name, hosts, macOS window and minimum OS |
| `exact.mjs` | Exact's per-app launcher (generated, don't edit) |
| `Cargo.toml`, `rust-toolchain.toml`, `.cargo/` | The authored Rust workspace and pinned toolchain |
| `data/` | The keyboard crate (see above); `data/examples/probe.rs` is a read-only hardware check |
| `apple/`, `linux/`, `web/` | One host crate per platform |
| `modules/web/index.js` | The WebHID page module |
| `site/` | The landing page: Vite + React + Tailwind v4 + shadcn/ui |
| `deploy/app.wrangler.jsonc`, `deploy/site.wrangler.jsonc` | Cloudflare Workers configs |
| `scripts/release.sh`, `scripts/deploy.sh` | Release and deploy |
| `udev/70-remap75.rules` | Linux permissions for the receiver and cable |
| `docs/PROTOCOL.md` | Protocol notes and sources |
| `assets/icon.svg` | App icon |

---

## Developing

Requirements:

- **Exact** checked out at `~/dev/exact` (override with `EXACT2=<path>`), with its toolchain:
  `bun ~/dev/exact/scripts/exact.mjs setup` (`setup --check` lists what a native build lacks).
  The web build needs the [local patch](#a-local-patch-to-exact-required-for-the-web-build).
- **Bun**, **Rust** via rustup (`~/.cargo/bin` on your `PATH`), **Xcode** for the Mac build.
- The workspace's `[patch]` and `exact-*` dependency paths are absolute paths to the Exact
  checkout. Adjust them if yours lives elsewhere.

```sh
bun run mac                     # build, sign with the Developer ID, open the Mac app
bun exact.mjs mac --run         # build and open without the signing identity
bun exact.mjs linux             # build the Linux app (on Linux)
bun run build && bun run start  # build the web app into dist/ and serve it
bun exact.mjs test web          # UI tests against the simulated keyboard
cargo test -p aula75-data       # protocol, layout, macro, service and replay tests
cargo run -p aula75-data --example probe   # read-only check of a real keyboard
AULA_SIMULATE=1 …               # run a native build against the simulated keyboard
```

Signing the dev build with the same Developer ID each time means macOS keeps the Input
Monitoring permission across rebuilds.

---

## Releasing the Mac app

```sh
xcrun notarytool store-credentials aula75-notary --apple-id <Apple ID> --team-id <TEAM ID>  # once
bun run release
```

`scripts/release.sh`:

1. Runs Exact's `release` verb with `EXACT_DEVELOPER_ID` and `EXACT_NOTARY_PROFILE`. This
   builds a production app (arm64), signs it with the hardened runtime and a timestamp,
   notarizes it as a zip, staples the ticket and builds a DMG.
2. Exact doesn't sign or notarize the DMG itself, so the script then signs the DMG, submits it
   to `notarytool`, staples it, and checks both the DMG and the app with `spctl`.

Output: `target/dist/aula-75-keyboard-software/Remap75.dmg` (about 9 MB), plus a dSYM.

---

## Deploying the web app and site (Cloudflare)

```sh
npx wrangler login   # once
bun run deploy
```

`scripts/deploy.sh` publishes **two Cloudflare Workers with static assets**:

1. **`remap75-app`** at **web.remap75.app**, the web app. `exact web-build --out dist`, then
   `wrangler deploy -c deploy/app.wrangler.jsonc`, with `not_found_handling:
   "single-page-application"` so deep links like `/keys` load the app. The web app must sit at
   the **root** of its origin (Exact's web build hardcodes `<base href="/">`), which is why it
   has its own subdomain and Worker rather than living under the site at `/app`.
2. **`remap75`** at **remap75.app** (and www.remap75.app), the landing page. It builds `site/`
   with `VITE_APP_URL=https://web.remap75.app/`, copies `site/dist` to `target/site`, adds the
   notarized DMG as `/download/Remap75.dmg` (under the 25 MiB per-file limit), and runs
   `wrangler deploy -c deploy/site.wrangler.jsonc`.

The domains are Cloudflare **custom domains** set in the `routes` of each config (the
`remap75.app` zone is in the same Cloudflare account). Cloudflare creates the DNS records and
certificates on deploy. `workers_dev` and `preview_urls` are off, so the apps are only served
on those domains.

To ship an update: `bun run release` (only when the Mac app changed), then `bun run deploy`.

Deploys are run **by hand from a Mac**, not from GitHub. Cloudflare's Git builds can't build this
project: they don't have the Exact checkout (with its [local patch](#a-local-patch-to-exact-required-for-the-web-build)),
the Rust toolchain, or the Developer ID and notary credentials the DMG needs.

---

## The landing site

`site/` is a standalone Vite + React + TypeScript app styled with **Tailwind CSS v4** and
**shadcn/ui** (Button, Card, Badge, Accordion and Separator in `site/src/components/ui`).

- `site/src/App.tsx`: the page sections (hero, how it works, features, picker, two ways to use
  it, FAQ).
- `site/src/index.css`: Tailwind plus the shadcn theme tokens. Light and dark follow the system.
- `site/public/img/`: screenshots taken from the real UI with `exact agent web` against the
  simulated keyboard.

```sh
cd site && bun install && bun run dev
```

---

## Known limits and gotchas

- The Mac build is **Apple silicon only** (arm64) and needs **macOS 14+**.
- The web app needs **WebHID**: Chrome, Edge or Opera. Safari and Firefox can't reach it.
- Bluetooth can't change settings. Use the receiver or the cable.
- Headless Chrome with `--virtual-time-budget` stalls the web app at "Looking for the
  keyboard…". Test it in a real browser tab.
- Updating Exact means re-applying the
  [web patch](#a-local-patch-to-exact-required-for-the-web-build).

---

## Contributing

Issues and pull requests are welcome. This started as a vibe-coded project, so a second pair
of eyes on any part of it helps make it better.

- **Found a bug or something confusing?**
  [Open an issue](https://github.com/rosman21/aula75-remapper/issues/new). Say whether you used
  the Mac app, the web app or Linux, and whether you were on the receiver or the cable. Paste
  what the Keyboard tab said too.
- **Have a fix or an idea?**
  [Open a pull request](https://github.com/rosman21/aula75-remapper/pulls). Keep it focused on
  one change. Run `cargo test -p aula75-data` and `bun exact.mjs test web` first. If it touches
  the protocol, add a test against the simulated keyboard (`data/src/sim.rs`).
- **Have a different AULA keyboard or firmware?** Reports of what works and what doesn't on other
  models or firmware versions are especially useful. A read-only `cargo run -p aula75-data
  --example probe` output is a good start.
- **Want something new?** Open an issue describing it, for example a per-key lighting editor,
  Intel Mac builds, or a Windows build.

---

## Disclaimer

**Remap75 is not affiliated with AULA.** It is an independent, unofficial project. It
is not made, endorsed, sponsored, reviewed or supported by AULA, its parent company, or the
F75's manufacturer. For help with the keyboard itself (warranty, firmware, hardware faults),
contact AULA. Don't take problems with this app to them.

- **Names.** "AULA", "AULA 75" and "AULA F75" are used only to say which keyboard this app
  works with. They belong to their owners, and using them here doesn't mean any connection to
  or approval from them.
- **No vendor code.** The app doesn't contain or redistribute AULA's driver or firmware. What it
  knows about the keyboard's protocol comes from community reverse engineering, credited in
  [`docs/PROTOCOL.md`](docs/PROTOCOL.md). The code here is its own implementation of those
  facts.
- **Use at your own risk.** The software is provided as is, without warranty. It writes
  settings to your keyboard's memory. If something goes wrong, holding **Fn+Esc** for about 5
  seconds restores factory settings, but you are responsible for your own hardware. This
  project is also [vibe coded](#aula-75-control).

---

## License

[MIT](LICENSE). See also [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).
