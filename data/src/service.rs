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
                        // A 1u modifier shows only its symbol, so it fits.
                        let mut l = k.label.to_string();
                        if k.width <= 1.0 && l.starts_with(['⌃', '⌘', '⌥', '⇧']) {
                            l = l.chars().take(1).collect();
                        }
                        json!({ "target": target, "slug": slug(&target), "label": l, "width": k.width, "gap": k.gap })
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
    keycodes::describe(code, names)
}

fn reply(ok: bool, message: impl Into<String>) -> Json {
    json!({ "ok": ok, "message": message.into() })
}

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
            if last == data {
                break;
            }
        }
        let kept = last == data;
        let diff: Vec<String> = (0..p::MATRIX_LEN)
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

    /// `Keymap { ok, message, rows: [{ id, keys: [KeyCap] }] }`.
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
        let rows: Vec<Json> = layout::ROWS
            .iter()
            .enumerate()
            .map(|(r, row)| {
                let keys: Vec<Json> = row
                    .iter()
                    .map(|k| {
                        let code = p::code_at(&data, k.index as usize);
                        let (target, mods) = keycodes::target_of(code);
                        let changed = if layer == p::LAYER_BASE { code != k.factory } else { code != [0; 4] };
                        json!({
                            "index": k.index, "label": k.label, "action": keycodes::describe(code, &names),
                            "cap": cap_label(code, &names),
                            "target": target, "mods": mods, "width": k.width, "gap": k.gap, "changed": changed,
                        })
                    })
                    .collect();
                json!({ "id": r, "keys": keys })
            })
            .collect();
        json!({ "ok": true, "message": "", "rows": rows })
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
    fn keys_take_combos_and_macros() {
        let mut s = sim();
        assert_eq!(s.assign(0, 3, "k:41", 0)["ok"], true); // Caps → Esc
        assert_eq!(s.keymap(0)["rows"][3]["keys"][0]["action"], "Esc");
        assert_eq!(s.keymap(0)["rows"][3]["keys"][0]["changed"], true);
        assert_eq!(s.assign(1, 9, "k:6", p::MOD_LGUI)["ok"], true); // Fn+A → ⌘C
        assert_eq!(s.keymap(1)["rows"][3]["keys"][1]["action"], "⌘C");
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
            remote::begin(journal);
            let j = f(s);
            let Some((op, mut entries)) = remote::finish() else { return (j, steps) };
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
