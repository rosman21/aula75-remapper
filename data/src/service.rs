//! The app's keyboard service: owns the connection and answers each source with JSON in
//! the field order its Contract shape declares (records are positional).
//!
//! Every method may block on USB, so the data source runs them on the host's worker.

use std::path::PathBuf;

use serde_json::{json, Value as Json};

use crate::keycodes::{self, Target};
use crate::layout;
use crate::macros::{self, Macro, Mode, Step};
use crate::protocol::{self as p};
use crate::remote::{self, NEED};
use crate::session::{Session, ASLEEP};
use crate::sim::SimKeyboard;

/// What the app knows about the keyboard and what it keeps.
pub struct Service {
    simulate: bool,
    /// The keyboard through the browser's page module (WebHID), files in its local storage.
    remote: bool,
    #[cfg(not(target_arch = "wasm32"))]
    api: Option<hidapi::HidApi>,
    session: Option<Session>,
    product: String,
    ready: bool,
    /// Online checks missed in a row: the radio drops one now and then, so a few misses
    /// don't count as the keyboard going away.
    misses: u32,
    /// Bumped on every (re)connection, so views read the keyboard again.
    epoch: u32,
    macros: Vec<Macro>,
    loaded: bool,
    dir: Option<PathBuf>,
    /// Each key layer as last read back from the keyboard on this connection.
    layers: std::collections::HashMap<u8, Vec<u8>>,
    /// The OS mode and whether Mac keys are set up, as last read, and how many times it has
    /// answered [`Service::keyboard_mode`] since (see there).
    mode: Option<(u8, bool, u32)>,
    /// Whether the keyboard held the app's latest Mac keys when [`Service::mode`] was read.
    mac_keys_latest: bool,
}

/// A target id as a test id: `k:6` → `k-6`.
pub fn slug(id: &str) -> String {
    id.replace(':', "-")
}

/// The board to pick a key's new action from: `[PickRow { id, keys: [PickKey { target, slug, label, width, gap }] }]`.
pub fn picker_layout() -> Json {
    Json::Array(
        layout::ROWS
            .iter()
            .enumerate()
            .map(|(r, row)| {
                let keys: Vec<Json> = row
                    .iter()
                    .map(|k| {
                        let target = keycodes::target_of(k.factory).0;
                        // The knob is drawn in its place, right of F12: picking it gives a key the
                        // knob's own press, which is also listed with the special actions.
                        let knob = k.index == layout::KNOB_INDEX;
                        // A 1u modifier shows only its symbol, so it fits.
                        let mut l = k.label.to_string();
                        if k.width <= 1.0 && l.starts_with(['⌃', '⌘', '⌥', '⇧']) {
                            l = l.chars().take(1).collect();
                        }
                        let id = if knob { "knob".to_string() } else { slug(&target) };
                        json!({ "target": target, "slug": id, "label": l, "width": k.width, "gap": k.gap })
                    })
                    .collect();
                json!({ "id": r, "keys": keys })
            })
            .collect(),
    )
}

/// What a key does, written short enough for its keycap: the label of the key it now acts as
/// (`⌥ Opt`, `Esc`), a short modifier name (`⌘ Cmd R`), or the full description.
pub fn cap_label(code: p::Code, names: &[String]) -> String {
    if let Some(k) = layout::ROWS.iter().flat_map(|r| r.iter()).find(|k| k.factory == code && code != [0; 4]) {
        return k.label.to_string();
    }
    if let [0, mods, _, 0] = code {
        let short = [
            (0x08, "⌘ Cmd"), (0x04, "⌥ Opt"), (0x01, "⌃ Ctrl"), (0x02, "⇧ Shift"),
            (0x80, "⌘ Cmd R"), (0x40, "⌥ Opt R"), (0x10, "⌃ Ctrl R"), (0x20, "⇧ Shift R"),
        ];
        if let Some((_, l)) = short.iter().find(|(bit, _)| *bit == mods) {
            return (*l).to_string();
        }
    }
    if let Some(short) = short_cap(code) {
        return short.to_string();
    }
    keycodes::describe(code, names)
}

/// A Mac top-row function, short enough for a 1u keycap.
fn short_cap(code: p::Code) -> Option<&'static str> {
    const SHORT: [&str; 12] =
        ["Bri−", "Bri+", "Mission", "Search", "", "", "Prev", "Play", "Next", "Mute", "Vol−", "Vol+"];
    let i = MAC_TOP_ROW.iter().position(|c| *c == Some(code))?;
    Some(SHORT[i])
}

/// What Mac mode (Fn+E) makes the key at `index` do, where that differs from the key layers:
/// the mode is built into the keyboard and ignores what the layers hold for these keys. A
/// Mac's top row on F1-F12 with the F-key itself under Fn, and ⌘ and ⌥ changing places.
/// The keys named in [`MAC_MODE_ONLY`] have no code a layer could hold: theirs is the F-key's.
fn mac_mode_code(layer: u8, index: u8) -> Option<p::Code> {
    let factory = |i: u8| layout::key(i).map(|k| k.factory);
    if let Some(f) = layout::F_KEYS.iter().position(|f| *f == index) {
        return match layer {
            p::LAYER_BASE if mac_mode_only(layer, index).is_none() => MAC_TOP_ROW[f].or(factory(index)),
            p::LAYER_BASE | p::LAYER_FN => factory(index),
            _ => None,
        };
    }
    match (layer, index) {
        (p::LAYER_BASE, layout::LEFT_CMD_INDEX) => factory(layout::LEFT_OPT_INDEX),
        (p::LAYER_BASE, layout::LEFT_OPT_INDEX) => factory(layout::LEFT_CMD_INDEX),
        _ => None,
    }
}

/// Mac-mode keys of the top row whose keycaps are left empty: what the keyboard sends for
/// them isn't known, and on the owner's Mac (macOS 27) they do nothing. On an Apple keyboard
/// they are Mission Control, Launchpad and the keyboard light.
/// (F-key number, what it is called, its keycap).
const MAC_MODE_ONLY: [(usize, &str, &str); 4] = [
    (3, "its own key, which can't be set here", ""),
    (4, "its own key, which can't be set here", ""),
    (5, "its own key, which can't be set here", ""),
    (6, "its own key, which can't be set here", ""),
];

/// The name and keycap name of a base-layer key that only Mac mode can make (see [`MAC_MODE_ONLY`]).
fn mac_mode_only(layer: u8, index: u8) -> Option<(&'static str, &'static str)> {
    let f = layout::F_KEYS.iter().position(|f| *f == index)? + 1;
    let only = MAC_MODE_ONLY.iter().find(|o| o.0 == f)?;
    (layer == p::LAYER_BASE).then_some((only.1, only.2))
}

fn reply(ok: bool, message: impl Into<String>) -> Json {
    json!({ "ok": ok, "message": message.into() })
}

/// F1-F12 as a Mac's top row (`None`: left as the F-key).
const MAC_TOP_ROW: [Option<p::Code>; 12] = [
    Some(p::consumer_code(0x70)),                // F1 brightness down
    Some(p::consumer_code(0x6f)),                // F2 brightness up
    Some(p::key_code(0x52, p::MOD_LCTRL)),       // F3 Mission Control (⌃↑)
    Some(p::key_code(0x2c, p::MOD_LGUI)),        // F4 Spotlight (⌘Space)
    None,                                        // F5
    None,                                        // F6
    Some(p::consumer_code(0xb6)),                // F7 previous track
    Some(p::consumer_code(0xcd)),                // F8 play / pause
    Some(p::consumer_code(0xb5)),                // F9 next track
    Some(p::consumer_code(0xe2)),                // F10 mute
    Some(p::consumer_code(0xea)),                // F11 volume down
    Some(p::consumer_code(0xe9)),                // F12 volume up
];

impl Service {
    /// A service over the real keyboard, or the simulator when `simulate`.
    pub fn new(simulate: bool, dir: Option<PathBuf>) -> Service {
        Service {
            simulate,
            remote: false,
            #[cfg(not(target_arch = "wasm32"))]
            api: None,
            session: None,
            product: String::new(),
            ready: false,
            misses: 0,
            epoch: 0,
            macros: Vec::new(),
            loaded: false,
            dir,
            layers: std::collections::HashMap::new(),
            mode: None,
            mac_keys_latest: false,
        }
    }

    /// A service that reaches the keyboard through the browser (see [`crate::remote`]).
    pub fn new_remote() -> Service {
        Service {
            remote: true,
            ..Service::new(false, None)
        }
    }

    /// Where the app keeps its files: `~/Library/Application Support/Remap75` on macOS,
    /// `$XDG_CONFIG_HOME/remap75` (or `~/.config/…`) on Linux. A folder from before the rename
    /// (`AULA 75 Control`, `aula75-control`) is moved there the first time, so macros and the
    /// saved factory layers carry over.
    pub fn default_dir() -> Option<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let (dir, old) = if cfg!(target_os = "macos") {
            let support = home.join("Library/Application Support");
            (support.join("Remap75"), support.join("AULA 75 Control"))
        } else {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"));
            (config.join("remap75"), config.join("aula75-control"))
        };
        if !dir.exists() && old.is_dir() {
            // Best effort: if the move fails the app starts with an empty folder.
            let _ = std::fs::rename(&old, &dir);
        }
        Some(dir)
    }

    // -------------------------------------------------------------- files

    /// One of the app's files: from its folder, or the browser's local storage on the web.
    fn read_file(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        if self.remote {
            return remote::get_file(name);
        }
        Ok(self.dir.as_ref().and_then(|d| std::fs::read(d.join(name)).ok()))
    }

    /// Keep one of the app's files (replacing it whole).
    fn write_file(&self, name: &str, data: &[u8]) -> Result<(), String> {
        if self.remote {
            return remote::put_file(name, data);
        }
        let Some(dir) = &self.dir else { return Ok(()) };
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join(name)).map_err(|e| e.to_string())
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        match self.read_file("macros.json") {
            // The page hasn't answered yet: load again on the next run.
            Err(_) => {}
            Ok(text) => {
                self.loaded = true;
                if let Some(text) = text {
                    self.macros = serde_json::from_slice(&text).unwrap_or_default();
                }
            }
        }
    }

    fn save(&self) -> Result<(), String> {
        self.write_file(
            "macros.json",
            &serde_json::to_vec_pretty(&self.macros).unwrap_or_default(),
        )
    }

    /// Append a line to `activity.log`, so a failed write can be diagnosed afterwards.
    fn log(&self, line: &str) {
        let Some(dir) = &self.dir else { return };
        let _ = std::fs::create_dir_all(dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("activity.log"))
        {
            use std::io::Write;
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let _ = writeln!(f, "{secs} {line}");
        }
    }

    fn backup_name(layer: u8) -> String {
        format!("backup-layer-{layer}.bin")
    }

    /// Keep the first layer we ever read, so "Restore" can always go back to it.
    fn backup(&self, layer: u8, data: &[u8]) {
        let name = Self::backup_name(layer);
        if let Ok(None) = self.read_file(&name) {
            let _ = self.write_file(&name, data);
        }
    }

    // -------------------------------------------------------------- connection

    fn drop_session(&mut self) {
        self.session = None;
        self.ready = false;
        self.layers.clear();
        self.mode = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scan_and_open(&mut self) -> Result<(Option<String>, crate::hid::Scan), String> {
        use crate::hid;
        if self.api.is_none() {
            self.api = Some(hid::api()?);
        }
        let scan = hid::scan(self.api.as_mut().expect("just set"))?;
        let still_there = match self.session.as_ref().map(Session::link) {
            Some(p::Link::Receiver) => scan.receiver,
            Some(p::Link::Wired) => scan.wired,
            None => true,
        };
        // Prefer the cable as soon as it appears.
        if !still_there
            || (scan.wired && self.session.as_ref().map(Session::link) == Some(p::Link::Receiver))
        {
            self.drop_session();
        }
        if self.session.is_none() && (scan.receiver || scan.wired) {
            if hid::access() == hid::Access::Denied {
                return Ok((Some("permission".into()), scan));
            }
            match hid::open(self.api.as_ref().expect("just set")) {
                Ok(Some(wire)) => {
                    self.product = wire.product.clone();
                    self.session = Some(Session::new(Box::new(wire)));
                }
                Ok(None) => {}
                Err(e) if e.contains("not permitted") || e.contains("Input Monitoring") => {
                    // Never asked: show macOS's own prompt once, which also lists the app
                    // under Input Monitoring.
                    static ASKED: std::sync::Once = std::sync::Once::new();
                    if hid::access() == hid::Access::Unknown {
                        ASKED.call_once(|| {
                            hid::request_access();
                        });
                    }
                    return Ok((Some("permission".into()), scan));
                }
                Err(e) => return Err(e),
            }
        }
        Ok((None, scan))
    }

    fn ensure_sim(&mut self) {
        if self.session.is_none() {
            self.product = "Simulated AULA F75".into();
            self.session = Some(Session::new(Box::new(SimKeyboard::default())));
        }
    }

    /// Where the connection stands: `Status { state, link, title, detail, product, epoch }`.
    pub fn status(&mut self) -> Json {
        let j = self.status_now();
        if self.diag_on() {
            if let Some(dir) = self.dir.as_ref().map(|d| d.join("diag")) {
                let _ = std::fs::create_dir_all(&dir);
                #[cfg(not(target_arch = "wasm32"))]
                let access = format!("{:?}", crate::hid::access());
                #[cfg(target_arch = "wasm32")]
                let access = String::new();
                let _ = std::fs::write(dir.join("status.txt"), format!("{j}\naccess={access}"));
            }
        }
        j
    }

    fn status_now(&mut self) -> Json {
        let status = |state: &str,
                      link: &str,
                      title: &str,
                      detail: &str,
                      product: &str,
                      epoch: u32| {
            json!({ "state": state, "link": link, "title": title, "detail": detail, "product": product, "epoch": epoch })
        };
        #[allow(unused_mut)] // only the USB path finds Bluetooth
        let mut bluetooth = false;
        if self.simulate {
            self.ensure_sim();
        } else if self.remote {
            if let Some((state, title, detail)) = self.remote_open() {
                return status(state, "", title, detail, "", self.epoch);
            }
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            match self.scan_and_open() {
                Err(e) => {
                    self.drop_session();
                    return status(
                        "error",
                        "",
                        "Couldn't reach the keyboard",
                        &e,
                        "",
                        self.epoch,
                    );
                }
                Ok((Some(_), _)) => {
                    self.drop_session();
                    return status(
                        "permission",
                        "receiver",
                        "Allow Input Monitoring",
                        "The receiver also carries your keystrokes, so macOS asks before an app may open it. Choose \
                         Allow access, turn on Remap75 in System Settings → Privacy & Security → Input \
                         Monitoring, then reopen the app.",
                        "",
                        self.epoch,
                    );
                }
                Ok((None, scan)) => bluetooth = scan.bluetooth,
            }
            #[cfg(target_arch = "wasm32")]
            self.ensure_sim();
        }
        let Some(session) = self.session.as_mut() else {
            self.ready = false;
            return if bluetooth {
                status(
                    "bluetooth",
                    "",
                    "Connected over Bluetooth only",
                    "Bluetooth can't change settings. Plug in the 2.4 GHz receiver (switch the keyboard to 2.4G) or \
                     the USB-C cable (switch it to wired).",
                    "",
                    self.epoch,
                )
            } else {
                status(
                    "searching",
                    "",
                    "Plug in the receiver",
                    "Plug the 2.4 GHz receiver into a USB port and set the keyboard's switch to 2.4G. Or connect the \
                     USB-C cable with the switch on wired.",
                    "",
                    self.epoch,
                )
            };
        };
        let link = session.link();
        let online = match session.online() {
            Ok(true) => {
                self.misses = 0;
                Ok(true)
            }
            Ok(false) if self.ready && self.misses < 4 => {
                self.misses += 1;
                Ok(true)
            }
            other => other,
        };
        match online {
            Ok(true) => {
                if !self.ready {
                    self.ready = true;
                    self.epoch += 1;
                    self.layers.clear();
                }
                let title = match link {
                    p::Link::Receiver => "Connected through the 2.4 GHz receiver",
                    p::Link::Wired => "Connected by USB-C cable",
                };
                let detail = if self.simulate {
                    "This is a simulated keyboard: nothing is sent to real hardware."
                } else {
                    "Changes are written to the keyboard and keep working on any computer."
                };
                let product = self.product.clone();
                self.diagnose();
                status("ready", link.id(), title, detail, &product, self.epoch)
            }
            Ok(false) => {
                self.ready = false;
                status(
                    "asleep",
                    link.id(),
                    "The receiver is plugged in, but the keyboard isn't answering",
                    "Press any key to wake the keyboard, and check that its switch is on 2.4G.",
                    &self.product.clone(),
                    self.epoch,
                )
            }
            // The page hasn't answered yet: change nothing.
            Err(e) if e == NEED => status("searching", "", "", "", "", self.epoch),
            Err(e) => {
                self.drop_session();
                status(
                    "error",
                    link.id(),
                    "The connection dropped",
                    &e,
                    "",
                    self.epoch,
                )
            }
        }
    }

    /// The keyboard the browser has open: `None` when there is one (the session is set up),
    /// otherwise the status to show (state, title, detail).
    fn remote_open(&mut self) -> Option<(&'static str, &'static str, &'static str)> {
        let device = match remote::io(json!({ "op": "device" })) {
            Ok(d) => d,
            Err(e) if e == NEED => return Some(("searching", "", "")),
            Err(_) => {
                self.drop_session();
                return Some(("error", "Couldn't reach the keyboard", "The browser refused to open it. Unplug it, plug it back in and choose Connect keyboard again."));
            }
        };
        match device["mode"].as_str() {
            Some("demo") => {
                self.simulate = true;
                self.drop_session();
                self.ensure_sim();
                None
            }
            Some("unsupported") => {
                self.drop_session();
                Some((
                    "unsupported",
                    "This browser can't reach USB devices",
                    "Open this page in Chrome, Edge or Opera on a computer, or download the Mac app. You can also try the app with a simulated keyboard.",
                ))
            }
            Some("device") => {
                let link = if device["link"] == "wired" { p::Link::Wired } else { p::Link::Receiver };
                if self.session.as_ref().map(Session::link) != Some(link) {
                    self.drop_session();
                    self.session = Some(Session::new(Box::new(remote::RemoteWire { link })));
                }
                self.product = device["product"].as_str().unwrap_or("AULA F75").to_string();
                None
            }
            _ => {
                self.drop_session();
                Some((
                    "connect",
                    "Connect your keyboard",
                    "Plug in the 2.4 GHz receiver (keyboard switch on 2.4G) or the USB-C cable (switch on wired). Then choose Connect keyboard and pick it in Chrome's list.",
                ))
            }
        }
    }

    /// Try the app with the simulated keyboard (the web, without a keyboard).
    pub fn use_demo(&mut self) -> Json {
        self.simulate = true;
        self.drop_session();
        self.ensure_sim();
        reply(true, "")
    }

    /// Diagnostics are on while `diag/enabled` exists in the app's folder (or `AULA_DIAG=1`).
    fn diag_on(&self) -> bool {
        !self.remote && std::env::var_os("AULA_DIAG").is_some_and(|v| v != "0")
            || self
                .dir
                .as_ref()
                .is_some_and(|d| d.join("diag/enabled").exists())
    }

    /// Developer diagnostics, only with `AULA_DIAG=1`: runs the lines of `diag/commands.txt`
    /// in the app's folder against the real keyboard and appends what it answered to
    /// `diag/results.txt`. (`read <layer>`, `led`, `assign <layer> <index> <target> <mods>`,
    /// `restore <layer>`, `reload`.)
    fn diagnose(&mut self) {
        if !self.diag_on() {
            return;
        }
        let Some(dir) = self.dir.as_ref().map(|d| d.join("diag")) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(dir.join("commands.txt")) else {
            return;
        };
        let _ = std::fs::remove_file(dir.join("commands.txt"));
        let mut out = String::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let w: Vec<&str> = line.split_whitespace().collect();
            let n = |i: usize| w.get(i).and_then(|x| x.parse::<u8>().ok()).unwrap_or(0);
            let result = match w[0] {
                "read" => {
                    self.layers.remove(&n(1));
                    match self.session().and_then(|s| s.keymap(n(1))) {
                        Ok(d) => {
                            let names = self.names();
                            (0..p::MATRIX_LEN)
                                .filter(|i| p::code_at(&d, *i) != [0; 4])
                                .map(|i| {
                                    let label = layout::key(i as u8).map_or("?", |k| k.label);
                                    format!(
                                        "#{i} {label}={:02x?} {}",
                                        p::code_at(&d, i),
                                        keycodes::describe(p::code_at(&d, i), &names)
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n  ")
                        }
                        Err(e) => format!("error: {e}"),
                    }
                }
                "led" => match self.session().and_then(Session::led_block) {
                    Ok(b) => b
                        .iter()
                        .map(|x| format!("{x:02x}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    Err(e) => format!("error: {e}"),
                },
                "copy" => match self.read_layer(n(1)).and_then(|d| self.write_layer(n(2), &d, None)) {
                    Ok(()) => format!("layer {} copied to {}", n(1), n(2)),
                    Err(e) => format!("error: {e}"),
                },
                "clear" => match self.write_layer(n(1), &vec![0u8; p::KEYMAP_LEN], None) {
                    Ok(()) => format!("layer {} cleared", n(1)),
                    Err(e) => format!("error: {e}"),
                },
                "osmode" => match self.session().and_then(|s| {
                    if w.len() > 1 { s.set_os_mode(n(1)) } else { s.os_mode() }
                }) {
                    Ok(m) => format!("os mode {m}"),
                    Err(e) => format!("error: {e}"),
                },
                "assign" => self
                    .assign(n(1), n(2), w.get(3).copied().unwrap_or("none"), n(4))
                    .to_string(),
                "restore" => self.restore(n(1)).to_string(),
                "reload" => self.reload().to_string(),
                other => format!("unknown command {other}"),
            };
            out.push_str(&format!("> {line}\n  {result}\n"));
        }
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("results.txt"))
        {
            use std::io::Write;
            let _ = f.write_all(out.as_bytes());
        }
    }

    /// The layers read so far. A browser run that stops for the page puts them back
    /// ([`Service::rewind`]), so every replay of a call starts from the same state and asks
    /// for the same I/O in the same order.
    pub fn layer_cache(&self) -> std::collections::HashMap<u8, Vec<u8>> {
        self.layers.clone()
    }

    /// Put the layers read back as they were when the run started.
    pub fn rewind(&mut self, layers: std::collections::HashMap<u8, Vec<u8>>) {
        self.layers = layers;
    }

    /// Ask macOS for Input Monitoring, or (in the browser) pick the keyboard in Chrome's list.
    pub fn request_access(&mut self) -> Json {
        if self.remote && !self.simulate {
            return match remote::io(json!({ "op": "connect" })) {
                Ok(r) => {
                    self.drop_session();
                    reply(
                        r["ok"].as_bool().unwrap_or(false),
                        r["message"].as_str().unwrap_or_default(),
                    )
                }
                Err(e) => reply(false, e),
            };
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !self.simulate && !crate::hid::request_access() {
            // Already refused (macOS shows its prompt only once): open the setting itself.
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("/usr/bin/open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
                .spawn();
            return reply(
                false,
                "System Settings is open at Input Monitoring. If Remap75 (or AULA 75 Control, its old name) is already on there, that entry is from an older build: select it, click −, add the app again with +, then quit and reopen the app.",
            );
        }
        self.drop_session();
        reply(true, "")
    }

    /// Run `f`, again (up to three more times) when the radio dropped a packet.
    fn retry<T>(
        &mut self,
        mut f: impl FnMut(&mut Session) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut attempt = 0;
        loop {
            match f(self.session()?) {
                Err(e) if e == ASLEEP && attempt < 3 => {
                    attempt += 1;
                    self.session()?.pause(300);
                }
                other => return other,
            }
        }
    }

    fn session(&mut self) -> Result<&mut Session, String> {
        if !self.ready {
            return Err("The keyboard isn't connected.".into());
        }
        self.session
            .as_mut()
            .ok_or_else(|| "The keyboard isn't connected.".to_string())
    }

    fn failed(&mut self, e: String) -> String {
        if e != ASLEEP {
            // A USB error: reconnect on the next status check.
            if e.starts_with("USB") {
                self.drop_session();
            }
        }
        e
    }

    // -------------------------------------------------------------- lighting

    fn lighting_json(ok: bool, message: &str, l: Option<p::Lighting>) -> Json {
        let l = l.unwrap_or(p::Lighting {
            mode: 0,
            brightness: 0,
            speed: 0,
            colour: 0,
            sleep: 0,
        });
        json!({
            "ok": ok, "message": message, "mode": l.mode, "brightness": l.brightness,
            "speed": l.speed, "colour": l.colour, "sleep": l.sleep,
        })
    }

    /// `Lighting { ok, message, mode, brightness, speed, colour, sleep }`.
    pub fn lighting(&mut self) -> Json {
        match self.session().and_then(Session::lighting) {
            Ok(l) => Self::lighting_json(true, "", Some(l)),
            Err(e) => {
                let e = self.failed(e);
                Self::lighting_json(false, &e, None)
            }
        }
    }

    /// Select an effect.
    pub fn set_lighting(&mut self, mode: u8, brightness: u8, speed: u8, colour: u8) -> Json {
        match self.retry(|s| s.set_lighting(mode, brightness, speed, colour)) {
            Ok(_) => reply(true, ""),
            Err(e) => {
                let e = self.failed(e);
                reply(false, e)
            }
        }
    }

    /// Set the sleep timer.
    pub fn set_sleep(&mut self, seconds: u32) -> Json {
        match self.retry(|s| s.set_sleep(seconds)) {
            Ok(_) => reply(true, ""),
            Err(e) => {
                let e = self.failed(e);
                reply(false, e)
            }
        }
    }

    // -------------------------------------------------------------- keys

    fn names(&self) -> Vec<String> {
        self.macros.iter().map(|m| m.name.clone()).collect()
    }

    fn fetch(&mut self, layer: u8) -> Result<Vec<u8>, String> {
        if let Some(d) = self.layers.get(&layer) {
            return Ok(d.clone());
        }
        let d = self.session()?.keymap(layer)?;
        self.layers.insert(layer, d.clone());
        Ok(d)
    }

    fn read_layer(&mut self, layer: u8) -> Result<Vec<u8>, String> {
        let base = self.fetch(p::LAYER_BASE)?;
        if !layout::base_plausible(&base) {
            self.layers.clear();
            return Err("The keyboard's key layout doesn't look like an AULA F75's, so editing is off to keep it safe.".into());
        }
        let data = if layer == p::LAYER_BASE {
            base.clone()
        } else {
            self.fetch(layer)?
        };
        self.backup(p::LAYER_BASE, &base);
        self.backup(layer, &data);
        Ok(data)
    }

    /// Write a layer (and the macros), then read it back until the keyboard reports what was
    /// written: the view shows what the keyboard holds, never what we hoped it holds.
    fn write_layer(&mut self, layer: u8, data: &[u8], macros: Option<&[u8]>) -> Result<(), String> {
        self.layers.remove(&layer);
        let signed = p::signed_layer(data);
        let data = signed.as_slice();
        let mut attempt = 0;
        loop {
            match self.session()?.write_keymap(layer, data, macros) {
                Ok(()) => break,
                // The 2.4 GHz link drops a packet now and then: send the whole layer again.
                Err(e) if e == ASLEEP && attempt < 3 => {
                    attempt += 1;
                    self.log(&format!("write layer {layer}: no answer, retry {attempt}"));
                    self.session()?.pause(300);
                }
                Err(e) => {
                    self.log(&format!("write layer {layer}: failed: {e}"));
                    return Err(e);
                }
            }
        }
        let session = self.session()?;
        let mut last = Vec::new();
        for attempt in 0..5u64 {
            session.pause(80 * attempt);
            last = session.keymap(layer)?;
            if p::same_keys(&last, data) {
                break;
            }
        }
        let kept = p::same_keys(&last, data);
        let diff: Vec<String> = (0..p::MATRIX_LEN - 1)
            .filter(|i| p::code_at(data, *i) != p::code_at(&last, *i))
            .map(|i| {
                format!(
                    "#{i} wrote {:02x?} read {:02x?}",
                    p::code_at(data, i),
                    p::code_at(&last, i)
                )
            })
            .collect();
        self.log(&format!(
            "write layer {layer}: {}",
            if kept {
                "kept".to_string()
            } else {
                diff.join(", ")
            }
        ));
        self.layers.insert(layer, last);
        if kept {
            Ok(())
        } else {
            Err("The keyboard acknowledged the change but still reports the old keys. Press a key to wake it and try again.".into())
        }
    }

    /// Forget what was read, so the next view reads the keyboard again.
    pub fn reload(&mut self) -> Json {
        self.layers.clear();
        reply(true, "")
    }

    /// `Keymap { ok, message, rows: [{ id, keys: [KeyCap] }] }`: what each key does now. In Mac
    /// mode that is the mode's own top row and ⌘/⌥ (see [`mac_mode_code`]), marked `builtIn`,
    /// whatever the layer holds for those keys.
    pub fn keymap(&mut self, layer: u8) -> Json {
        self.load();
        let data = match self.read_layer(layer) {
            Ok(d) => d,
            Err(e) => {
                let e = self.failed(e);
                return json!({ "ok": false, "message": e, "rows": [] });
            }
        };
        let names = self.names();
        let os = self.session().and_then(Session::os_mode);
        let mac = os == Ok(p::OS_MAC);
        // Turning the knob is built into the keyboard and has no slot in a layer: shown, not set.
        let (turn, turn_cap) = match (layer, &os) {
            (p::LAYER_BASE, Ok(p::OS_WINDOWS)) => ("turning it changes the volume", "↻ Vol"),
            (p::LAYER_BASE, Ok(p::OS_MAC)) => ("turning it does nothing in Mac mode", "↻ off"),
            _ => ("", ""),
        };
        let rows: Vec<Json> = layout::ROWS
            .iter()
            .enumerate()
            .map(|(r, row)| {
                let keys: Vec<Json> = row
                    .iter()
                    .map(|k| {
                        let built_in = mac.then(|| mac_mode_code(layer, k.index)).flatten();
                        let code = built_in.unwrap_or_else(|| p::code_at(&data, k.index as usize));
                        let (target, mods) = keycodes::selected(code);
                        let action = match keycodes::shortcut_of(code) {
                            Some((_, name)) => format!("{name} ({})", keycodes::describe(code, &names)),
                            None => keycodes::describe(code, &names),
                        };
                        let changed = built_in.is_none()
                            && if layer == p::LAYER_BASE { code != k.factory } else { code != [0; 4] };
                        let (action, cap) = match built_in.and(mac_mode_only(layer, k.index)) {
                            Some((name, short)) => (name.to_string(), short.to_string()),
                            None => (action, cap_label(code, &names)),
                        };
                        json!({
                            "index": k.index, "label": k.label, "action": action,
                            "cap": cap,
                            "target": target, "mods": mods, "width": k.width, "gap": k.gap, "changed": changed,
                            "builtIn": built_in.is_some(),
                            "turn": if k.index == layout::KNOB_INDEX { turn } else { "" },
                            "turnCap": if k.index == layout::KNOB_INDEX { turn_cap } else { "" },
                        })
                    })
                    .collect();
                json!({ "id": r, "keys": keys })
            })
            .collect();
        json!({ "ok": true, "message": "", "rows": rows })
    }

    /// How many times in a row [`Service::keyboard_mode`] answers from what it last read while
    /// the keyboard reports no change: at four asks a second, the keyboard is still read every
    /// 2.5 s, in case a change notice was lost.
    const MODE_ASKS_PER_READ: u32 = 10;

    /// `KeyboardMode { mode, macKeys, macKeysLatest, live }`: the OS mode the keyboard is in now (Fn+W / Fn+E /
    /// Fn+Q change it on the keyboard, so the Keys screen keeps asking), and whether Mac keys
    /// are set up (Windows mode, and Fn+E can't leave it). `macKeysLatest` says they are the
    /// keys this version of the app flashes ([`Service::mac_keys`]); when it is false with
    /// `macKeys` true, an older set or the owner's own changes are there. `mode` is "" when unknown.
    ///
    /// `live` is true when the keyboard's own change notices can be heard (the receiver, see
    /// [`p::WIRELESS_CHANGED`]): asking is then free unless something changed, so the view asks
    /// several times a second and the mode follows the keyboard at once. Otherwise every ask
    /// reads the keyboard, and the view asks every few seconds.
    pub fn keyboard_mode(&mut self) -> Json {
        self.load();
        let live = !self.remote && self.session().is_ok_and(|s| s.listens());
        if live {
            let changed = self.session().and_then(Session::take_changed).unwrap_or(true);
            if let Some((os, mac_keys, asks)) = self.mode {
                if !changed && asks < Self::MODE_ASKS_PER_READ {
                    self.mode = Some((os, mac_keys, asks + 1));
                    return json!({ "mode": p::os_mode_id(os), "macKeys": mac_keys,
                        "macKeysLatest": self.mac_keys_latest, "live": true });
                }
            }
        }
        let os = self.session().and_then(Session::os_mode);
        let mac_keys = os == Ok(p::OS_WINDOWS)
            && self
                .read_layer(p::LAYER_FN)
                .is_ok_and(|f| p::code_at(&f, layout::E_INDEX as usize) == p::os_mode_key(p::OS_WINDOWS));
        self.mac_keys_latest = mac_keys && self.holds_latest_mac_keys();
        // What this read itself drained is already in the answer.
        self.mode = match (&os, live) {
            (Ok(os), true) if self.session().and_then(Session::take_changed).is_ok() => Some((*os, mac_keys, 0)),
            _ => None,
        };
        json!({ "mode": os.map_or("", p::os_mode_id), "macKeys": mac_keys,
            "macKeysLatest": self.mac_keys_latest, "live": live })
    }

    /// Put the keyboard in an OS mode, as Fn+W / Fn+E / Fn+Q do. Mac mode sends media keys on
    /// F1-F12 and swaps ⌘ and ⌥ over the stored layers, so remapped keys only work in Windows
    /// mode. Leaving Mac mode for Windows mode swaps the left ⌘ and ⌥ keys in the base layer
    /// (when they are still as shipped), so they stay where Mac mode had them.
    pub fn set_os_mode(&mut self, mode: u8) -> Json {
        self.load();
        match self.switch_os_mode(mode) {
            Ok(()) => reply(true, ""),
            Err(e) => {
                let e = self.failed(e);
                reply(false, e)
            }
        }
    }

    fn switch_os_mode(&mut self, mode: u8) -> Result<(), String> {
        let before = self.session()?.os_mode()?;
        let after = self.session()?.set_os_mode(mode)?;
        self.log(&format!("os mode {before} -> {after}"));
        if after != mode {
            return Err("The keyboard didn't switch modes. Press a key to wake it and try again.".into());
        }
        if before == p::OS_MAC && mode == p::OS_WINDOWS {
            let (cmd, opt) = (layout::LEFT_CMD_INDEX as usize, layout::LEFT_OPT_INDEX as usize);
            let mut base = self.read_layer(p::LAYER_BASE)?;
            let factory = |i: usize| layout::key(i as u8).map(|k| k.factory);
            if Some(p::code_at(&base, cmd)) == factory(cmd) && Some(p::code_at(&base, opt)) == factory(opt) {
                let (c, o) = (p::code_at(&base, cmd), p::code_at(&base, opt));
                p::set_code(&mut base, cmd, o);
                p::set_code(&mut base, opt, c);
                self.write_layer(p::LAYER_BASE, &base, None)?;
            }
        }
        self.layers.clear();
        Ok(())
    }

    /// Flash Mac keys: Mac mode's F-row and ⌘/⌥ swap are built into the firmware and ignore
    /// every key layer, so remaps can't work there (and on a current macOS some of its top row
    /// does nothing). This puts the keyboard in Windows mode and writes the app's latest Mac
    /// keys into the layers, whatever those keys held: ⌘ and ⌥ in their Mac places,
    /// [`MAC_TOP_ROW`] on F1-F12 with the F-key itself under Fn, and Fn+E switching to Windows
    /// mode rather than Mac mode. In Windows mode the knob's turn is the volume. Every other
    /// key keeps what it has.
    pub fn mac_keys(&mut self) -> Json {
        self.load();
        match self.set_up_mac_keys(true) {
            Ok(()) => reply(true, ""),
            Err(e) => {
                let e = self.failed(e);
                reply(false, e)
            }
        }
    }

    /// The Mac keys as (layer, index, code): what [`Service::mac_keys`] writes.
    fn mac_keyset() -> Vec<(u8, u8, p::Code)> {
        let factory = |i: u8| layout::key(i).map_or([0; 4], |k| k.factory);
        let (cmd, opt) = (layout::LEFT_CMD_INDEX, layout::LEFT_OPT_INDEX);
        let mut v = vec![
            (p::LAYER_BASE, cmd, factory(opt)),
            (p::LAYER_BASE, opt, factory(cmd)),
            (p::LAYER_FN, layout::E_INDEX, p::os_mode_key(p::OS_WINDOWS)),
        ];
        for (f, code) in layout::F_KEYS.iter().zip(MAC_TOP_ROW) {
            v.push((p::LAYER_BASE, *f, code.unwrap_or(factory(*f))));
            if code.is_some() {
                v.push((p::LAYER_FN, *f, factory(*f)));
            }
        }
        v
    }

    fn holds_latest_mac_keys(&mut self) -> bool {
        let (Ok(base), Ok(fn_layer)) = (self.read_layer(p::LAYER_BASE), self.read_layer(p::LAYER_FN)) else {
            return false;
        };
        Self::mac_keyset()
            .iter()
            .all(|(layer, i, code)| p::code_at(if *layer == p::LAYER_BASE { &base } else { &fn_layer }, *i as usize) == *code)
    }

    /// Windows mode with the Mac keys in the layers. `flash` writes every one of them; without
    /// it (a key changed while in Mac mode) keys the owner already changed are left as they are.
    fn set_up_mac_keys(&mut self, flash: bool) -> Result<(), String> {
        let before = self.session()?.os_mode()?;
        if before != p::OS_WINDOWS {
            let after = self.session()?.set_os_mode(p::OS_WINDOWS)?;
            self.log(&format!("os mode {before} -> {after}"));
            if after != p::OS_WINDOWS {
                return Err("The keyboard didn't switch modes. Press a key to wake it and try again.".into());
            }
        }
        let factory = |i: u8| layout::key(i).map_or([0; 4], |k| k.factory);
        let mut base = self.read_layer(p::LAYER_BASE)?;
        let mut fn_layer = self.read_layer(p::LAYER_FN)?;
        let (base_was, fn_was) = (base.clone(), fn_layer.clone());
        let (cmd, opt) = (layout::LEFT_CMD_INDEX as usize, layout::LEFT_OPT_INDEX as usize);
        // ⌘ and ⌥ move together or not at all.
        let modifiers = flash || (p::code_at(&base, cmd) == factory(cmd as u8) && p::code_at(&base, opt) == factory(opt as u8));
        for (layer, index, code) in Self::mac_keyset() {
            let i = index as usize;
            let write = match (layer, i) {
                (p::LAYER_BASE, i) if i == cmd || i == opt => modifiers,
                (p::LAYER_BASE, _) => flash || p::code_at(&base, i) == factory(index),
                // A top-row key the owner changed keeps its Fn entry too.
                (_, _) if layout::F_KEYS.contains(&index) => {
                    flash || p::code_at(&base_was, i) == factory(index)
                }
                _ => true,
            };
            if write {
                p::set_code(if layer == p::LAYER_BASE { &mut base } else { &mut fn_layer }, i, code);
            }
        }
        if base != base_was {
            self.write_layer(p::LAYER_BASE, &base, None)?;
        }
        if fn_layer != fn_was {
            self.write_layer(p::LAYER_FN, &fn_layer, None)?;
        }
        self.layers.clear();
        Ok(())
    }

    fn macro_buffer(&self) -> Result<Vec<u8>, String> {
        macros::buffer(&self.macros)
    }

    /// Point `layer`'s key `index` at `target` (with `mods` held, for a key).
    pub fn assign(&mut self, layer: u8, index: u8, target: &str, mods: u8) -> Json {
        self.load();
        if layout::key(index).is_none() {
            return reply(false, "No such key.");
        }
        let Some(mut code) = keycodes::code_for(target, mods) else {
            return reply(false, "Unknown action.");
        };
        if code[0] == 3 {
            let Some(m) = self.macros.get(code[3] as usize) else {
                return reply(false, "No such macro.");
            };
            code = p::macro_code(code[3], m.mode.byte(), 1);
        }
        if layer == p::LAYER_BASE && index == 53 && code != p::FN_CODE {
            return reply(
                false,
                "Keep Fn on the base layer: without it the Fn layer can't be reached.",
            );
        }
        let buffer = match self.macro_buffer() {
            Ok(b) => b,
            Err(e) => return reply(false, e),
        };
        // A change made in Mac or Android mode would be stored and then ignored (the mode is
        // built into the firmware), so leave it first: Mac mode for Mac keys, Android for Windows.
        let leave = match self.session().and_then(Session::os_mode) {
            Ok(p::OS_MAC) => self.set_up_mac_keys(false),
            Ok(p::OS_ANDROID) => self.switch_os_mode(p::OS_WINDOWS),
            _ => Ok(()),
        };
        if let Err(e) = leave {
            let e = self.failed(e);
            return reply(false, e);
        }
        let result = self.read_layer(layer).and_then(|mut data| {
            p::set_code(&mut data, index as usize, code);
            let uses_macros = code[0] == 3 && !buffer.is_empty();
            self.write_layer(layer, &data, uses_macros.then_some(buffer.as_slice()))
        });
        match result {
            Ok(()) => reply(true, ""),
            Err(e) => {
                let e = self.failed(e);
                reply(false, e)
            }
        }
    }

    /// Put a layer back: the base layer as it shipped, or the copy taken the first time
    /// this app read it.
    pub fn restore(&mut self, layer: u8) -> Json {
        let saved = self
            .read_file(&Self::backup_name(layer))
            .ok()
            .flatten()
            .filter(|d| d.len() == p::KEYMAP_LEN);
        let data = match (layer, saved) {
            (_, Some(d)) => d,
            (p::LAYER_BASE, None) => layout::factory_base(),
            _ => return reply(false, "There's no saved copy of this layer yet."),
        };
        if layer == p::LAYER_BASE && !layout::base_plausible(&data) {
            return reply(
                false,
                "The saved copy doesn't look right, so it wasn't written.",
            );
        }
        match self.write_layer(layer, &data, None) {
            Ok(()) => reply(true, ""),
            Err(e) => {
                let e = self.failed(e);
                reply(false, e)
            }
        }
    }

    /// The targets that aren't keys on the board (those are picked from [`picker_layout`]),
    /// grouped: `[TargetGroup { name, targets: [Target { id, label, group, slug }] }]`.
    pub fn targets(&mut self) -> Json {
        self.load();
        // Keys on the board are picked from it; every modifier is listed, left and right.
        let on_board: Vec<String> = layout::keys()
            .filter(|k| k.index != layout::KNOB_INDEX)
            .map(|k| keycodes::target_of(k.factory).0)
            .filter(|id| !id.starts_with("m:"))
            .collect();
        let all = keycodes::targets(&self.names());
        let mut groups: Vec<(&str, Vec<&Target>)> = Vec::new();
        for t in all.iter().filter(|t| !on_board.contains(&t.id)) {
            match groups.iter_mut().find(|g| g.0 == t.group) {
                Some(g) => g.1.push(t),
                None => groups.push((t.group, vec![t])),
            }
        }
        Json::Array(
            groups
                .into_iter()
                .map(|(name, ts)| {
                    let ts: Vec<Json> = ts
                        .iter()
                        .map(|t| json!({ "id": t.id, "label": t.label, "group": t.group, "slug": slug(&t.id) }))
                        .collect();
                    json!({ "name": if name == "Editing and navigation" { "More keys" } else { name }, "targets": ts })
                })
                .collect(),
        )
    }

    // -------------------------------------------------------------- macros

    /// `[MacroInfo { id, name, summary, mode, steps: [StepInfo] }]`.
    pub fn macro_list(&mut self) -> Json {
        self.load();
        Json::Array(
            self.macros
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let steps: Vec<Json> = m
                        .steps
                        .iter()
                        .enumerate()
                        .map(|(j, s)| {
                            let (kind, text, usage, mods, ms) = match s {
                                Step::Text { text } => ("text", text.clone(), 0, 0, 0),
                                Step::Combo { usage, mods } => ("combo", String::new(), *usage, *mods, 0),
                                Step::Wait { ms } => ("wait", String::new(), 0, 0, *ms),
                            };
                            let one = Macro { name: String::new(), steps: vec![s.clone()], mode: Mode::Once };
                            json!({ "id": j, "kind": kind, "text": text, "usage": usage, "mods": mods, "ms": ms,
                                    "label": macros::summary(&one) })
                        })
                        .collect();
                    json!({ "id": i, "name": m.name, "summary": macros::summary(m), "mode": m.mode.id(), "steps": steps })
                })
                .collect(),
        )
    }

    fn edit(&mut self, f: impl FnOnce(&mut Vec<Macro>) -> Result<(), String>) -> Json {
        self.load();
        let before = self.macros.clone();
        if let Err(e) = f(&mut self.macros) {
            self.macros = before;
            return reply(false, e);
        }
        if let Err(e) = self.macro_buffer() {
            self.macros = before;
            return reply(false, e);
        }
        match self.save() {
            Ok(()) => reply(true, ""),
            Err(e) => {
                self.macros = before;
                reply(false, format!("Couldn't save the macros: {e}"))
            }
        }
    }

    /// Add a macro named `name`.
    pub fn add_macro(&mut self, name: &str) -> Json {
        let name = name.trim().to_string();
        self.edit(|ms| {
            if name.is_empty() {
                return Err("Give the macro a name.".into());
            }
            if ms.len() >= macros::MAX_MACROS {
                return Err(format!(
                    "The keyboard keeps up to {} macros.",
                    macros::MAX_MACROS
                ));
            }
            ms.push(Macro {
                name,
                steps: Vec::new(),
                mode: Mode::Once,
            });
            Ok(())
        })
    }

    /// Rename macro `id` and set how its key plays it.
    pub fn update_macro(&mut self, id: usize, name: &str, mode: &str) -> Json {
        let name = name.trim().to_string();
        self.edit(|ms| {
            let m = ms.get_mut(id).ok_or("No such macro.")?;
            if name.is_empty() {
                return Err("Give the macro a name.".into());
            }
            m.name = name;
            m.mode = Mode::parse(mode);
            Ok(())
        })
    }

    /// Append a step to macro `id`.
    pub fn add_step(
        &mut self,
        id: usize,
        kind: &str,
        text: &str,
        usage: u8,
        mods: u8,
        ms: u32,
    ) -> Json {
        let step = match kind {
            "text" if text.is_empty() => {
                return reply(false, "Type the text the macro should type.")
            }
            "text" => {
                let bad = macros::untypable(text);
                if !bad.is_empty() {
                    return reply(
                        false,
                        format!("A macro can't type “{bad}”: only US-keyboard characters work."),
                    );
                }
                Step::Text {
                    text: text.to_string(),
                }
            }
            "combo" if usage == 0 && mods == 0 => {
                return reply(false, "Choose a key or a modifier.")
            }
            "combo" => Step::Combo { usage, mods },
            "wait" if ms == 0 || ms > 60_000 => {
                return reply(false, "Wait between 1 ms and 60 seconds.")
            }
            "wait" => Step::Wait { ms },
            _ => return reply(false, "Unknown step."),
        };
        self.edit(|all| {
            all.get_mut(id).ok_or("No such macro.")?.steps.push(step);
            Ok(())
        })
    }

    /// Remove step `step` of macro `id`.
    pub fn remove_step(&mut self, id: usize, step: usize) -> Json {
        self.edit(|all| {
            let m = all.get_mut(id).ok_or("No such macro.")?;
            if step >= m.steps.len() {
                return Err("No such step.".into());
            }
            m.steps.remove(step);
            Ok(())
        })
    }

    /// Delete macro `id`; keys that played it are disabled, later macros' keys follow their
    /// macro's new place.
    pub fn delete_macro(&mut self, id: usize) -> Json {
        let r = self.edit(|all| {
            if id >= all.len() {
                return Err("No such macro.".into());
            }
            all.remove(id);
            Ok(())
        });
        if r["ok"] != true || !self.ready {
            return r;
        }
        let renumber = |data: &mut Vec<u8>| {
            for k in layout::keys() {
                let c = p::code_at(data, k.index as usize);
                if c[0] == 3 {
                    let i = c[3] as usize;
                    let next = if i == id {
                        [0; 4]
                    } else if i > id {
                        [3, c[1], c[2], c[3] - 1]
                    } else {
                        c
                    };
                    p::set_code(data, k.index as usize, next);
                }
            }
        };
        self.sync_layers(renumber)
    }

    /// Write every layer's macro keys and the macro buffer to the keyboard.
    pub fn upload_macros(&mut self) -> Json {
        self.load();
        // Nothing plays a macro: nothing on the keyboard needs rewriting.
        let uses = |s: &mut Self, layer| {
            s.read_layer(layer)
                .map(|d| layout::keys().any(|k| p::code_at(&d, k.index as usize)[0] == 3))
        };
        match (uses(self, p::LAYER_BASE), uses(self, p::LAYER_FN)) {
            (Ok(false), Ok(false)) => return reply(true, ""),
            (Err(e), _) | (_, Err(e)) => {
                let e = self.failed(e);
                return reply(false, e);
            }
            _ => {}
        }
        let modes: Vec<u8> = self.macros.iter().map(|m| m.mode.byte()).collect();
        self.sync_layers(move |data| {
            for k in layout::keys() {
                let c = p::code_at(data, k.index as usize);
                if c[0] == 3 {
                    if let Some(mode) = modes.get(c[3] as usize) {
                        p::set_code(data, k.index as usize, p::macro_code(c[3], *mode, 1));
                    }
                }
            }
        })
    }

    fn sync_layers(&mut self, f: impl Fn(&mut Vec<u8>)) -> Json {
        let buffer = match self.macro_buffer() {
            Ok(b) => b,
            Err(e) => return reply(false, e),
        };
        let result = (|| {
            for layer in [p::LAYER_FN, p::LAYER_BASE] {
                let mut data = self.read_layer(layer)?;
                f(&mut data);
                let macros = (layer == p::LAYER_BASE).then_some(buffer.as_slice());
                self.write_layer(layer, &data, macros)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => reply(true, ""),
            Err(e) => {
                let e = self.failed(e);
                reply(false, e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim() -> Service {
        let mut s = Service::new(true, None);
        assert_eq!(s.status()["state"], "ready");
        s
    }

    #[test]
    fn the_simulator_connects_and_reads() {
        let mut s = sim();
        assert_eq!(s.status()["epoch"], 1);
        let l = s.lighting();
        assert_eq!(l["ok"], true);
        assert_eq!(l["mode"], 3);
        let k = s.keymap(0);
        assert_eq!(k["ok"], true);
        assert_eq!(k["rows"][0]["keys"][0]["label"], "Esc");
        assert_eq!(k["rows"][0]["keys"][0]["action"], "Esc");
        assert_eq!(k["rows"][0]["keys"][0]["changed"], false);
    }

    #[test]
    fn the_mode_is_answered_from_memory_until_something_changes() {
        let mut s = sim();
        let m = s.keyboard_mode();
        assert_eq!((&m["mode"], &m["live"]), (&json!("mac"), &json!(true)));
        assert_eq!(s.mode, Some((p::OS_MAC, false, 0)));
        // Asked again with nothing changed: no read, up to the limit, then a read.
        for asks in 1..=Service::MODE_ASKS_PER_READ {
            assert_eq!(s.keyboard_mode()["mode"], "mac");
            assert_eq!(s.mode, Some((p::OS_MAC, false, asks)));
        }
        assert_eq!(s.keyboard_mode()["mode"], "mac");
        assert_eq!(s.mode, Some((p::OS_MAC, false, 0)));
        // A write is a change: the next ask reads the keyboard.
        assert_eq!(s.set_os_mode(p::OS_WINDOWS)["ok"], true);
        assert_eq!(s.keyboard_mode()["mode"], "windows");
        assert_eq!(s.mode, Some((p::OS_WINDOWS, false, 0)));
    }

    #[test]
    fn mac_mode_shows_its_own_top_row_and_modifiers() {
        let mut s = sim();
        let key = |s: &mut Service, layer: u8, row: usize, i: usize| s.keymap(layer)["rows"][row]["keys"][i].clone();
        // Base layer: a Mac's top row, not F1-F12, and ⌥ ⌘ in their Mac places.
        let f1 = key(&mut s, 0, 0, 1);
        assert_eq!((&f1["label"], &f1["action"], &f1["cap"]), (&json!("F1"), &json!("Brightness Down"), &json!("Bri−")));
        assert_eq!((&f1["builtIn"], &f1["changed"], &f1["target"]), (&json!(true), &json!(false), &json!("c:112")));
        assert_eq!(key(&mut s, 0, 0, 12)["action"], "Volume Up");
        let f4 = key(&mut s, 0, 0, 4);
        assert_eq!((&f4["action"], &f4["cap"], &f4["target"]), (&json!("its own key, which can't be set here"), &json!(""), &json!("k:61")));
        assert_eq!(key(&mut s, 0, 0, 3)["cap"], "");
        assert_eq!(key(&mut s, 0, 0, 5)["cap"], "");
        assert_eq!(key(&mut s, 0, 0, 6)["cap"], "");
        assert_eq!(key(&mut s, 0, 5, 1)["action"], "Left Option");
        assert_eq!(key(&mut s, 0, 5, 2)["action"], "Left Command");
        assert_eq!(key(&mut s, 0, 0, 0)["builtIn"], false);
        // Fn layer: the F-keys themselves.
        let f12 = key(&mut s, 1, 0, 12);
        assert_eq!((&f12["action"], &f12["builtIn"]), (&json!("F12"), &json!(true)));
        assert_eq!((&key(&mut s, 1, 0, 5)["action"], &key(&mut s, 1, 0, 5)["builtIn"]), (&json!("F5"), &json!(true)));
        // Windows mode shows the layers as they are.
        assert_eq!(s.set_os_mode(p::OS_WINDOWS)["ok"], true);
        let f1 = key(&mut s, 0, 0, 1);
        assert_eq!((&f1["action"], &f1["builtIn"]), (&json!("F1"), &json!(false)));
    }

    #[test]
    fn flashing_mac_keys_writes_the_latest_set_over_what_was_there() {
        let mut s = sim();
        let key = |s: &mut Service, layer: u8, row: usize, i: usize| s.keymap(layer)["rows"][row]["keys"][i].clone();
        // A key changed in Mac mode sets Mac keys up around it: the change is kept, so the
        // set is not the latest one.
        assert_eq!(s.assign(0, 78, "s:33:10", 0)["ok"], true);
        let m = s.keyboard_mode();
        assert_eq!((&m["mode"], &m["macKeys"], &m["macKeysLatest"]), (&json!("windows"), &json!(true), &json!(false)));
        assert_eq!(s.assign(0, 3, "k:41", 0)["ok"], true); // Caps Lock as Escape
        // Flashing writes every Mac key and nothing else.
        assert_eq!(s.mac_keys()["ok"], true);
        let m = s.keyboard_mode();
        assert_eq!((&m["macKeys"], &m["macKeysLatest"]), (&json!(true), &json!(true)));
        assert_eq!(key(&mut s, 0, 0, 12)["action"], "Volume Up");
        assert_eq!(key(&mut s, 1, 0, 12)["action"], "F12");
        assert_eq!(key(&mut s, 0, 0, 3)["action"], "Mission Control (⌃Up)");
        assert_eq!(key(&mut s, 0, 0, 5)["action"], "F5");
        assert_eq!(key(&mut s, 0, 5, 1)["action"], "Left Option");
        assert_eq!(key(&mut s, 0, 3, 0)["action"], "Esc");
        // From plain Windows mode too.
        let mut s = sim();
        assert_eq!(s.set_os_mode(p::OS_WINDOWS)["ok"], true);
        assert_eq!(s.keyboard_mode()["macKeysLatest"], false);
        assert_eq!(s.mac_keys()["ok"], true);
        assert_eq!(s.keyboard_mode()["macKeysLatest"], true);
        assert_eq!(key(&mut s, 0, 5, 2)["action"], "Left Command");
    }

    #[test]
    fn the_knobs_turn_is_shown_with_its_press() {
        let mut s = sim();
        let knob = |s: &mut Service, layer: u8| s.keymap(layer)["rows"][0]["keys"][13].clone();
        assert_eq!(knob(&mut s, 0)["turn"], "turning it does nothing in Mac mode");
        assert_eq!(s.mac_keys()["ok"], true);
        let k = knob(&mut s, 0);
        assert_eq!((&k["turn"], &k["turnCap"]), (&json!("turning it changes the volume"), &json!("↻ Vol")));
        assert_eq!(knob(&mut s, 1)["turn"], "");
        assert_eq!(s.keymap(0)["rows"][0]["keys"][12]["turn"], "");
    }

    #[test]
    fn the_knobs_press_is_shown_and_can_be_changed() {
        let mut s = sim();
        assert_eq!(s.set_os_mode(p::OS_WINDOWS)["ok"], true);
        let knob = |s: &mut Service| s.keymap(0)["rows"][0]["keys"][13].clone();
        let k = knob(&mut s);
        assert_eq!((&k["index"], &k["label"], &k["action"]), (&json!(84), &json!("Knob"), &json!("Next lighting effect")));
        assert_eq!((&k["target"], &k["changed"]), (&json!("x:29"), &json!(false)));
        assert_eq!(s.assign(0, layout::KNOB_INDEX, "c:226", 0)["ok"], true); // Mute
        let k = knob(&mut s);
        assert_eq!((&k["action"], &k["changed"]), (&json!("Mute"), &json!(true)));
        assert_eq!(s.assign(0, layout::KNOB_INDEX, "x:29", 0)["ok"], true); // and back
        assert_eq!(knob(&mut s)["changed"], false);
        // It is offered with the special actions, and drawn on the picker's board right of F12.
        let special = s.targets()[0].clone();
        assert!(special["targets"].as_array().unwrap().iter().any(|t| t["id"] == "x:29"));
        let top = picker_layout()[0]["keys"].clone();
        assert_eq!((&top[13]["label"], &top[13]["target"], &top[13]["slug"]), (&json!("Knob"), &json!("x:29"), &json!("knob")));
    }

    #[test]
    fn keys_take_combos_and_macros() {
        let mut s = sim();
        assert_eq!(s.assign(0, 3, "k:41", 0)["ok"], true); // Caps → Esc
        assert_eq!(s.keymap(0)["rows"][3]["keys"][0]["action"], "Esc");
        assert_eq!(s.keymap(0)["rows"][3]["keys"][0]["changed"], true);
        assert_eq!(s.assign(1, 9, "k:6", p::MOD_LGUI)["ok"], true); // Fn+A → ⌘C
        assert_eq!(s.keymap(1)["rows"][3]["keys"][1]["action"], "Copy (⌘C)");
        assert_eq!(s.keymap(1)["rows"][3]["keys"][1]["cap"], "⌘C");
        assert_eq!(s.assign(1, 15, "s:33:10", 0)["ok"], true); // Fn+S → screenshot of an area
        let k = s.keymap(1);
        assert_eq!(k["rows"][3]["keys"][2]["action"], "Selected area (⇧⌘4)");
        assert_eq!(k["rows"][3]["keys"][2]["target"], "s:33:10");
        assert_eq!(s.assign(0, 53, "k:4", 0)["ok"], false); // Fn stays
        assert_eq!(s.add_macro("Sign-off")["ok"], true);
        assert_eq!(s.add_step(0, "text", "Best, Ada", 0, 0, 0)["ok"], true);
        assert_eq!(s.add_step(0, "text", "naïve", 0, 0, 0)["ok"], false);
        assert_eq!(s.assign(1, 12, "macro:0", 0)["ok"], true);
        assert_eq!(s.keymap(1)["rows"][0]["keys"][1]["action"], "Sign-off");
        assert_eq!(s.delete_macro(0)["ok"], true);
        assert_eq!(s.keymap(1)["rows"][0]["keys"][1]["action"], "Disabled");
        assert_eq!(s.restore(0)["ok"], true);
        assert_eq!(s.keymap(0)["rows"][3]["keys"][0]["action"], "Caps Lock");
    }

    /// The browser's page module, played by the simulated keyboard: each step a call asks
    /// for is done and the call runs again, as the web host does.
    fn through_page(
        s: &mut Service,
        page: &mut Session,
        files: &mut std::collections::HashMap<String, String>,
        f: impl Fn(&mut Service) -> Json,
    ) -> (Json, usize) {
        let mut journal = Vec::new();
        for steps in 0.. {
            let layers = s.layer_cache();
            remote::begin(journal);
            let j = f(s);
            let Some((op, mut entries)) = remote::finish() else { return (j, steps) };
            s.rewind(layers);
            let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
            let n = |k: &str| op[k].as_u64().unwrap_or(0);
            let result = match op["op"].as_str().unwrap_or_default() {
                "device" => json!({ "ok": { "mode": "device", "link": "receiver", "product": "Page" } }),
                "read" => match page.read_raw(n("cmd") as u8, n("sub") as u8, n("len") as usize) {
                    Ok(b) => json!({ "ok": hex(&b) }),
                    Err(e) => json!({ "err": e }),
                },
                "write" => {
                    let data = remote::unhex(op["data"].as_str().unwrap_or_default());
                    match page.write_raw(n("cmd") as u8, n("sub") as u8, &data, n("delay")) {
                        Ok(()) => json!({ "ok": null }),
                        Err(e) => json!({ "err": e }),
                    }
                }
                "get" => json!({ "ok": files.get(op["name"].as_str().unwrap_or_default()) }),
                "put" => {
                    files.insert(op["name"].as_str().unwrap_or_default().into(), op["data"].as_str().unwrap_or_default().into());
                    json!({ "ok": null })
                }
                other => panic!("unexpected page op {other}"),
            };
            entries.push((op, result));
            journal = entries;
        }
        unreachable!()
    }

    #[test]
    fn the_browser_path_reads_and_writes_the_keyboard_step_by_step() {
        let mut s = Service::new_remote();
        let mut page = Session::new(Box::new(SimKeyboard::default()));
        let mut files = std::collections::HashMap::new();
        let (st, steps) = through_page(&mut s, &mut page, &mut files, |s| s.status());
        assert_eq!(st["state"], "ready", "{st}");
        assert_eq!(steps, 2); // device, then online
        let (k, _) = through_page(&mut s, &mut page, &mut files, |s| s.keymap(0));
        assert_eq!(k["rows"][3]["keys"][0]["action"], "Caps Lock");
        assert!(files.contains_key("backup-layer-0.bin"));
        let (r, _) = through_page(&mut s, &mut page, &mut files, |s| s.assign(0, 3, "k:41", 0));
        assert_eq!(r["ok"], true, "{r}");
        // What the keyboard holds now, read over packets.
        assert_eq!(p::code_at(&page.keymap(0).unwrap(), 3), [0, 0, 0, 41]);
        let (k, _) = through_page(&mut s, &mut page, &mut files, |s| s.keymap(0));
        assert_eq!(k["rows"][3]["keys"][0]["cap"], "Esc");
        let (m, _) = through_page(&mut s, &mut page, &mut files, |s| s.add_macro("Hello"));
        assert_eq!(m["ok"], true);
        assert!(files.contains_key("macros.json"));
        let (r, _) = through_page(&mut s, &mut page, &mut files, |s| s.restore(0));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(p::code_at(&page.keymap(0).unwrap(), 3), [0, 0, 0, 57]);
    }
}
