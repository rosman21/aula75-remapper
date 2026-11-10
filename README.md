<img src="assets/icon.svg" width="64" height="64" alt="">

# AULA 75 Control

A native macOS (and Linux) app for the **AULA F75** keyboard, the "AULA 75", in the spirit of VIA/Vial:
remap keys, make keys send combinations (⌘C, ⇧⌘4…), use the Fn layer, record macros, and set
lighting and sleep. It talks to the keyboard through its **2.4 GHz USB receiver** or, as a
fallback, the **USB-C cable**. Everything is written to the keyboard itself, so it keeps working
on any computer.

Built with [Exact](https://github.com/expo/exact): the UI is `app.contract`, which is drawn
with AppKit on the Mac and painted natively on Linux. The keyboard logic is a Rust data crate
(`data/`) that uses hidapi (IOKit on macOS, hidraw on Linux).

## How it connects

| | USB id | Interface | Transport |
|---|---|---|---|
| 2.4 GHz receiver | `3554:fa09` | interface 1, usage page `0xFF02` | 20-byte report `0x13`, checksummed, acknowledged per packet |
| USB-C cable | `258a:010c` | interface 1 | 520-byte feature report `0x06` |
| Bluetooth | `3554:fa08` | — | view only (config traffic disrupts typing) |

The app checks every 2.5 s. If the receiver and cable are both plugged in, it prefers the cable.
If the receiver is present but the keyboard is asleep or out of range, the app says so.
The receiver's config interface also carries keystrokes, so:

- **macOS** asks for **Input Monitoring**. Choose *Allow access* in the app, turn it on in System
  Settings → Privacy & Security → Input Monitoring, then reopen the app. The device is opened
  **shared**, never exclusively, so typing keeps working.
- **Linux** needs the udev rule: `sudo cp udev/70-aula75.rules /etc/udev/rules.d/ && sudo udevadm control --reload-rules && sudo udevadm trigger`, then replug.

## What it does

- **Keys**: a drawn 75% board for the base and Fn layers. Click a key to set it to a letter, a
  key with modifiers held (a combination), a modifier, a media key, a macro, Fn or nothing.
  Before any write, the app reads the layer and checks it looks like an F75's, and it keeps a
  copy of each layer the first time it reads it. *Restore* puts the factory base layer, or the
  saved Fn layer, back.
- **Lighting**: 18 built-in effects, brightness 0–9, speed 0–4, the palette colour, and the
  sleep timer.
- **Macros**: steps (type text, press a combination, wait), played once, while held, or as a
  toggle. They are kept in `~/Library/Application Support/AULA 75 Control/macros.json`
  (`~/.config/aula75-control/` on Linux) and written to the keyboard's macro memory.

If a write ever leaves the keyboard in a bad state, holding **Fn+Esc** for about 5 seconds
resets it to factory settings.

## Build and run

You need the Exact checkout (this app uses `~/dev/exact`; `EXACT2=<path>` overrides it) and its
toolchain (`bun ~/dev/exact/scripts/exact.mjs setup`).

```sh
bun exact.mjs mac --run        # build and open the macOS app
bun exact.mjs linux            # build the Linux app (on Linux)
bun exact.mjs test web         # the UI tests, against a simulated keyboard
cargo test -p aula75-data      # protocol, layout, macro and service tests
cargo run -p aula75-data --example probe   # read-only check of the real keyboard
AULA_SIMULATE=1 …              # run any native build against the simulated keyboard
```

## Layout

| Path | What it is |
|---|---|
| `app.contract`, `app.test.contract` | The view and its tests |
| `data/src/protocol.rs` | Framing, checksums, the LED block, key codes |
| `data/src/session.rs` | Acknowledged writes, reassembled reads, keyboard jobs |
| `data/src/hid.rs` | Finding and opening the receiver/cable, the macOS permission |
| `data/src/sim.rs` | A packet-level simulated keyboard, used by the tests and the web build |
| `data/src/layout.rs`, `keycodes.rs`, `macros.rs` | The F75's keys, what a key can do, and macros |
| `data/src/service.rs`, `source.rs` | The connection and the data source the view asks |
| `apple/`, `linux/`, `web/` | One host crate per platform |
| `docs/PROTOCOL.md` | Protocol notes and sources |
