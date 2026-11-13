//! The data source the Contract view asks. Every answer that touches the keyboard runs on
//! the host's worker as a continuation (USB I/O never blocks the UI); its JSON reply comes
//! back to [`DataSource::parse`], which turns it into the declared shape. On the web the
//! browser's page module reaches the keyboard (WebHID), one replayed step at a time
//! ([`crate::remote`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use exact_runner::{Answer, DataError, DataSource, Outcome, Request, Response, Store, Value};
use serde_json::Value as Json;

use crate::protocol as p;
use crate::service::Service;

/// One source call, owned, so it can cross to the worker.
#[derive(Clone, Debug)]
enum Call {
    Status,
    RequestAccess,
    UseDemo,
    Lighting,
    SetLighting(u8, u8, u8, u8),
    SetSleep(u32),
    Keymap(u8),
    Assign(u8, u8, String, u8),
    Restore(u8),
    SetOsMode(u8),
    MacKeys,
    KeyboardMode,
    Targets,
    Macros,
    AddMacro(String),
    UpdateMacro(usize, String, String),
    AddStep(usize, String, String, u8, u8, u32),
    RemoveStep(usize, usize),
    DeleteMacro(usize),
    UploadMacros,
    Reload,
}

fn num(args: &[Value], i: usize) -> f64 {
    args.get(i).and_then(Value::as_number).unwrap_or(0.0)
}
fn byte(args: &[Value], i: usize) -> u8 {
    num(args, i).clamp(0.0, 255.0) as u8
}
fn text(args: &[Value], i: usize) -> String {
    args.get(i)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

impl Call {
    fn parse(source: &str, a: &[Value]) -> Result<Call, DataError> {
        Ok(match source {
            "deviceStatus" => Call::Status,
            "requestAccess" => Call::RequestAccess,
            "useDemo" => Call::UseDemo,
            "lighting" => Call::Lighting,
            "setLighting" => Call::SetLighting(byte(a, 0), byte(a, 1), byte(a, 2), byte(a, 3)),
            "setSleep" => Call::SetSleep(num(a, 0).max(0.0) as u32),
            "keymap" => Call::Keymap(byte(a, 0)),
            "assignKey" => Call::Assign(byte(a, 0), byte(a, 1), text(a, 2), byte(a, 3)),
            "restoreLayer" => Call::Restore(byte(a, 0)),
            "setOsMode" => Call::SetOsMode(byte(a, 0)),
            "macKeys" => Call::MacKeys,
            "keyboardMode" => Call::KeyboardMode,
            "targets" => Call::Targets,
            "macroList" => Call::Macros,
            "addMacro" => Call::AddMacro(text(a, 0)),
            "updateMacro" => Call::UpdateMacro(num(a, 0) as usize, text(a, 1), text(a, 2)),
            "addStep" => Call::AddStep(
                num(a, 0) as usize,
                text(a, 1),
                text(a, 2),
                byte(a, 3),
                byte(a, 4),
                num(a, 5).max(0.0) as u32,
            ),
            "removeStep" => Call::RemoveStep(num(a, 0) as usize, num(a, 1) as usize),
            "deleteMacro" => Call::DeleteMacro(num(a, 0) as usize),
            "uploadMacros" => Call::UploadMacros,
            "reloadKeys" => Call::Reload,
            // Static tables: answered now, in `query`.
            _ => return Err(DataError::UnknownSource(source.to_string())),
        })
    }

    fn run(self, s: &mut Service) -> Json {
        match self {
            Call::Status => s.status(),
            Call::RequestAccess => s.request_access(),
            Call::UseDemo => s.use_demo(),
            Call::Lighting => s.lighting(),
            Call::SetLighting(m, b, sp, c) => s.set_lighting(m, b, sp, c),
            Call::SetSleep(secs) => s.set_sleep(secs),
            Call::Keymap(layer) => s.keymap(layer),
            Call::Assign(layer, index, target, mods) => s.assign(layer, index, &target, mods),
            Call::Restore(layer) => s.restore(layer),
            Call::SetOsMode(mode) => s.set_os_mode(mode),
            Call::MacKeys => s.mac_keys(),
            Call::KeyboardMode => s.keyboard_mode(),
            Call::Targets => s.targets(),
            Call::Macros => s.macro_list(),
            Call::AddMacro(name) => s.add_macro(&name),
            Call::UpdateMacro(id, name, mode) => s.update_macro(id, &name, &mode),
            Call::AddStep(id, kind, t, usage, mods, ms) => {
                s.add_step(id, &kind, &t, usage, mods, ms)
            }
            Call::RemoveStep(id, step) => s.remove_step(id, step),
            Call::DeleteMacro(id) => s.delete_macro(id),
            Call::UploadMacros => s.upload_macros(),
            Call::Reload => s.reload(),
        }
    }
}

/// JSON (objects in declared field order) to a Contract value.
pub fn to_value(j: &Json) -> Value {
    match j {
        Json::Null => Value::Unit,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        Json::String(s) => Value::str(s),
        Json::Array(xs) => Value::list(xs.iter().map(to_value).collect()),
        Json::Object(fields) => Value::record(fields.values().map(to_value).collect()),
    }
}

/// The app's data source.
pub struct Aula {
    service: Arc<Mutex<Service>>,
    /// The browser: each call replays through the page module.
    remote: bool,
    calls: HashMap<u64, Call>,
    next: u64,
}

impl Default for Aula {
    fn default() -> Self {
        if cfg!(target_arch = "wasm32") {
            return Aula::remote();
        }
        let simulate = std::env::var_os("AULA_SIMULATE").is_some_and(|v| v != "0");
        // A simulated keyboard keeps nothing on disk.
        let dir = if simulate {
            None
        } else {
            Service::default_dir()
        };
        Aula {
            remote: false,
            service: Arc::new(Mutex::new(Service::new(simulate, dir))),
            calls: HashMap::new(),
            next: 0,
        }
    }
}

impl Aula {
    /// The source in a browser: the keyboard through the page module.
    pub fn remote() -> Aula {
        Aula {
            remote: true,
            service: Arc::new(Mutex::new(Service::new_remote())),
            calls: HashMap::new(),
            next: 0,
        }
    }

    /// Run `call` against the journal so far: its value, or the page module's next step.
    fn replay(&self, call: Call, journal: Vec<(Json, Json)>) -> Answer {
        let mut s = self.service.lock().unwrap_or_else(|e| e.into_inner());
        let layers = s.layer_cache();
        crate::remote::begin(journal);
        let j = call.run(&mut s);
        match crate::remote::finish() {
            None => Answer::Now(to_value(&j)),
            Some((op, journal)) => {
                s.rewind(layers);
                let body = serde_json::json!({ "aula": 1, "op": op, "journal": journal });
                Answer::Later(Request::native(serde_json::to_vec(&body).unwrap_or_default()))
            }
        }
    }

    fn effects() -> Json {
        Json::Array(
            p::EFFECTS
                .iter()
                .map(|e| serde_json::json!({ "id": e.id, "name": e.name, "speed": e.speed, "colour": e.colour }))
                .collect(),
        )
    }

    fn palette() -> Json {
        Json::Array(
            p::PALETTE
                .iter()
                .enumerate()
                .map(|(i, (name, rgb))| {
                    serde_json::json!({ "id": i, "name": name, "css": format!("rgb({}, {}, {})", rgb[0], rgb[1], rgb[2]) })
                })
                .collect(),
        )
    }

    fn sleeps() -> Json {
        Json::Array(
            p::SLEEP
                .iter()
                .map(|(secs, _)| {
                    let label = match secs {
                        0 => "Never".to_string(),
                        s if s % 60 == 0 => format!("{} min", s / 60),
                        s => format!("{} min {} s", s / 60, s % 60),
                    };
                    serde_json::json!({ "seconds": secs, "label": label })
                })
                .collect(),
        )
    }

}

impl DataSource for Aula {
    fn app_id(&self) -> &str {
        "com.remap75.app"
    }

    /// Static tables now; the keyboard only at run time.
    fn query(&mut self, source: &str, _args: &[Value]) -> Result<Value, DataError> {
        match source {
            "effects" => Ok(to_value(&Self::effects())),
            "palette" => Ok(to_value(&Self::palette())),
            "sleepChoices" => Ok(to_value(&Self::sleeps())),
            "pickerLayout" => Ok(to_value(&crate::service::picker_layout())),
            _ => Err(DataError::Unavailable("asked at run time".into())),
        }
    }

    fn answer(
        &mut self,
        _store: &mut Store,
        source: &str,
        args: &[Value],
    ) -> Result<Answer, DataError> {
        if matches!(
            source,
            "effects" | "palette" | "sleepChoices" | "pickerLayout"
        ) {
            return self.query(source, args).map(Answer::Now);
        }
        let call = Call::parse(source, args)?;
        if self.remote {
            return Ok(self.replay(call, Vec::new()));
        }
        self.next += 1;
        self.calls.insert(self.next, call);
        Ok(Answer::Later(Request::continuation(self.next)))
    }

    fn continuation(&mut self, token: u64) -> Option<Box<dyn FnOnce() -> Outcome + Send>> {
        let call = self.calls.remove(&token)?;
        let service = Arc::clone(&self.service);
        Some(Box::new(move || {
            let mut s = service.lock().unwrap_or_else(|e| e.into_inner());
            let body = serde_json::to_vec(&call.run(&mut s)).unwrap_or_default();
            Outcome::Response(Response {
                status: 200,
                headers: Vec::new(),
                body,
            })
        }))
    }

    fn discard(&mut self, token: u64) {
        self.calls.remove(&token);
    }

    fn parse(
        &mut self,
        _store: &mut Store,
        source: &str,
        args: &[Value],
        outcome: Outcome,
    ) -> Result<Answer, DataError> {
        match outcome {
            Outcome::Response(r) => {
                let j: Json = serde_json::from_slice(&r.body).map_err(|e| {
                    DataError::Unavailable(format!("{source}: unreadable reply: {e}"))
                })?;
                if self.remote {
                    // The page did one more step: run the call again with it.
                    let journal: Vec<(Json, Json)> =
                        serde_json::from_value(j["journal"].clone()).map_err(|e| {
                            DataError::Unavailable(format!("{source}: unreadable page reply: {e}"))
                        })?;
                    return Ok(self.replay(Call::parse(source, args)?, journal));
                }
                Ok(Answer::Now(to_value(&j)))
            }
            Outcome::Failed { message, .. } => Err(DataError::Unavailable(message)),
            _ => Err(DataError::Unavailable(format!(
                "{source}: unexpected reply"
            ))),
        }
    }
}
